//! Il disegno con wgpu su DirectX 12.
//!
//! - Solo DX12 (`Backends::DX12`), scheda video ad alte prestazioni.
//! - Swapchain DXGI flip model; `desired_maximum_frame_latency = 1`: al massimo un frame in coda.
//!   wgpu aspetta il "frame latency waitable object" dentro `get_current_texture`, quindi il ciclo dei frame legge
//!   il mouse subito dopo, all'ultimo momento.
//! - Presentazione `Immediate` (vsync spento, tearing permesso) quando c'è, altrimenti `Fifo`; `Fifo` con il vsync.
//! - Niente buffer di profondità né multisample: una stanza vista da dentro, una sfera e dei rettangoli.
//! - A ogni frame vanno nella lista dei comandi solo 128 byte di costanti immediate; i rettangoli si riscrivono
//!   quando cambia il secondo mostrato o la risoluzione.
//! - Le sfere (fino a `MAX_SFERE`, disegnate in un'istanza ciascuna) stanno nel buffer delle uniformi con il loro
//!   tratto di moto: lo shader ne calcola il centro dal tempo di gioco, quindi il buffer si riscrive solo quando un
//!   tratto cambia (sfera colpita, cambio di direzione), non a ogni frame.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{DVec3, Mat4};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::aspetto::{self, ColoriBersaglio, Tema};
use crate::mirino::{self, Mirino};
pub use crate::scenari::MAX_SFERE;

/// Punteggio e precisione in percento del giro, per il disegno.
pub type DalVivo = Option<(u32, Option<u32>)>;

/// Mirino 8, tempo 16, conto alla rovescia 7, punteggio dal vivo fino a 42: c'è margine.
const MAX_RETT: usize = 128;
/// Descrittori di texture e buffer che gli shader possono vedere insieme: ne usiamo meno di dieci (le uniformi e
/// le texture di egui); 16 384 lasciano un margine largo e costano 0,5 MB.
const DESCRITTORI: u32 = 16_384;

/// Quello che cambia a ogni frame: costanti immediate (root constants), scritte nella lista dei comandi. Con un
/// buffer di uniformi `write_buffer` costava a ogni frame una copia in un buffer di appoggio e una lista di comandi
/// in più da mandare.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Immediati {
    vp: [[f32; 4]; 4],
    /// Tempo di gioco in secondi (per il moto delle sfere), indice della sfera guardata (-1 nessuna), libero, libero.
    tempo: [f32; 4],
    destra: [f32; 4],
    su: [f32; 4],
    schermo: [f32; 4],
}

/// I 128 byte che DirectX 12 dà alle costanti immediate in wgpu: `Immediati` ci deve stare.
const DIMENSIONE_IMMEDIATI: u32 = 128;
const _: () = assert!(std::mem::size_of::<Immediati>() as u32 <= DIMENSIONE_IMMEDIATI);

/// Una sfera per lo shader: il tratto di moto (`moto` = imbardata e beccheggio di partenza, velocità in radianti al
/// secondo; `altro` = istante di partenza in secondi di gioco, raggio, distanza, istante di comparsa in secondi di
/// gioco: prima la sfera non si disegna).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, PartialEq, Debug, Default)]
pub struct SferaGpu {
    pub moto: [f32; 4],
    pub altro: [f32; 4],
}

/// Quello che nel giro cambia di rado (l'occhio, i colori dei bersagli, il tema della stanza, la zona, i tratti delle
/// sfere): si riscrive solo quando cambia.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
struct Uniformi {
    occhio: [f32; 4],
    bersaglio_normale: [f32; 4],
    bersaglio_guardato: [f32; 4],
    bersaglio_colpito: [f32; 4],
    pavimento: [f32; 4],
    soffitto: [f32; 4],
    pareti: [f32; 4],
    linee: [f32; 4],
    /// Imbardata minima e massima, beccheggio minimo e massimo dei centri (radianti): i bordi dei rimbalzi.
    zona: [f32; 4],
    /// Le sfere in campo e, subito dopo l'ultima, quella del lampo.
    sfere: [SferaGpu; MAX_SFERE + 1],
}

/// Le sfere di un frame.
#[derive(Clone, Copy, Debug)]
pub struct Sfere {
    pub dati: [SferaGpu; MAX_SFERE],
    pub n: u32,
    pub zona: [f32; 4],
    /// Tempo di gioco in secondi.
    pub tempo_s: f32,
    /// La sfera sotto il mirino (il suo colore è "guardato").
    pub guardata: Option<u32>,
    /// La sfera appena colpita, ferma dove era, e l'opacità del lampo.
    pub lampo: Option<(SferaGpu, f32)>,
}

/// Un rettangolo del mirino o del tempo. `tipo`: 0 pieno, 1 barra del tempo, 2 anello (`anello` = raggi interno ed
/// esterno in pixel, centrato nel rettangolo).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct Rett {
    xywh: [f32; 4],
    colore: [f32; 4],
    tipo: f32,
    anello: [f32; 2],
}

/// Quello che serve per disegnare un frame di gioco.
pub struct Scena {
    pub vp: Mat4,
    pub occhio: DVec3,
    pub destra: DVec3,
    pub su: DVec3,
    /// Le sfere da disegnare; `None` nel menu e durante il conto alla rovescia.
    pub sfere: Option<Sfere>,
    /// Secondi rimasti e frazione (per il tempo in alto); `None` fuori dal giro.
    pub tempo: Option<(u32, f32)>,
    /// Il conto alla rovescia (3, 2, 1) al centro, prima del giro.
    pub conto: Option<u32>,
    /// Punteggio e precisione in percento, in alto a sinistra; `None` se spento o fuori dal giro.
    pub dal_vivo: DalVivo,
}

/// Il disegno di egui da aggiungere sopra la scena.
pub struct StratoEgui<'a> {
    pub primitive: &'a [egui::ClippedPrimitive],
    pub schermo: &'a egui_wgpu::ScreenDescriptor,
    /// Se le primitive sono nuove vanno caricate (`update_buffers`); se no si ridisegnano quelle di prima.
    pub nuove: bool,
}

/// Le misure della stanza: l'occhio sta al centro, ad altezza 1.7.
pub const STANZA: (f32, f32, f32) = (18.0, 10.0, 18.0);

// Segmenti accesi per cifra: a b c d e f g
const SEGMENTI: [u8; 10] = [
    0b1111110, 0b0110000, 0b1101101, 0b1111001, 0b0110011, 0b1011011, 0b1011111, 0b1110000, 0b1111111,
    0b1111011,
];

pub struct Grafica {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub scheda: wgpu::AdapterInfo,
    pub egui: egui_wgpu::Renderer,
    modi: Vec<wgpu::PresentMode>,
    p_stanza: wgpu::RenderPipeline,
    p_sfera: wgpu::RenderPipeline,
    p_rett: wgpu::RenderPipeline,
    b_stanza: wgpu::Buffer,
    vertici_stanza: u32,
    b_uniformi: wgpu::Buffer,
    uniformi_scritte: Option<Uniformi>,
    b_rett: wgpu::Buffer,
    gruppo: wgpu::BindGroup,
    rett: [Rett; MAX_RETT],
    n_rett: usize,
    /// Secondi, conto alla rovescia, dimensioni e punteggio dal vivo dell'ultima scrittura dei rettangoli; se
    /// cambiano si riscrivono.
    rett_scritti: (u32, u32, u32, u32, bool, DalVivo),
    scala: f32,
    mirino: Mirino,
    colori: ColoriBersaglio,
    tema: Tema,
}

/// Una faccia della stanza: i quattro angoli e la normale verso l'interno.
type Faccia = ([f32; 3], [f32; 3], [f32; 3], [f32; 3], [f32; 3]);

fn stanza() -> Vec<f32> {
    let (x, y, z) = STANZA;
    // Sei facce viste da dentro: posizione e normale verso l'interno.
    let facce: [Faccia; 6] = [
        ([-x, 0.0, -z], [x, 0.0, -z], [x, 0.0, z], [-x, 0.0, z], [0.0, 1.0, 0.0]),
        ([-x, y, -z], [-x, y, z], [x, y, z], [x, y, -z], [0.0, -1.0, 0.0]),
        ([-x, 0.0, -z], [-x, y, -z], [x, y, -z], [x, 0.0, -z], [0.0, 0.0, 1.0]),
        ([-x, 0.0, z], [x, 0.0, z], [x, y, z], [-x, y, z], [0.0, 0.0, -1.0]),
        ([-x, 0.0, -z], [-x, 0.0, z], [-x, y, z], [-x, y, -z], [1.0, 0.0, 0.0]),
        ([x, 0.0, -z], [x, y, -z], [x, y, z], [x, 0.0, z], [-1.0, 0.0, 0.0]),
    ];
    let mut out = Vec::with_capacity(36 * 6);
    for (a, b, c, d, n) in facce {
        for v in [a, b, c, a, c, d] {
            out.extend_from_slice(&v);
            out.extend_from_slice(&n);
        }
    }
    out
}

/// La modalità di presentazione per il vsync richiesto, fra quelle che la superficie offre.
pub fn scegli_presentazione(modi: &[wgpu::PresentMode], vsync: bool) -> wgpu::PresentMode {
    if vsync {
        return wgpu::PresentMode::Fifo;
    }
    [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox]
        .into_iter()
        .find(|m| modi.contains(m))
        .unwrap_or(wgpu::PresentMode::Fifo)
}

impl Grafica {
    pub fn new(finestra: Arc<Window>, vsync: bool) -> Result<Self, String> {
        crate::sistema::segna_memoria("finestra creata");
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        // Le etichette servono solo agli strumenti di debug della scheda video: nella release non si passano a
        // DirectX (wgpu le copierebbe a ogni encoder e render pass).
        if !cfg!(debug_assertions) {
            desc.flags |= wgpu::InstanceFlags::DISCARD_HAL_LABELS;
        }
        let istanza = wgpu::Instance::new(desc);
        let dimensioni = finestra.inner_size();
        let surface = istanza.create_surface(finestra).map_err(|e| format!("superficie: {e}"))?;
        let scheda = pollster::block_on(istanza.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("nessuna scheda video DirectX 12: {e}"))?;
        crate::sistema::segna_memoria("istanza DirectX 12 e scheda video");
        let (device, queue) = pollster::block_on(scheda.request_device(&wgpu::DeviceDescriptor {
            label: Some("aimpro"),
            // Su DirectX 12 wgpu le offre sempre.
            required_features: wgpu::Features::IMMEDIATES,
            required_limits: wgpu::Limits {
                max_immediate_size: DIMENSIONE_IMMEDIATI,
                // L'heap dei descrittori che gli shader vedono: wgpu lo crea grande quanto questo limite (1 000 000
                // di descrittori, circa 31 MB di memoria impegnata). Qui ci sono una manciata di bind group.
                max_non_sampler_bindings: DESCRITTORI,
                ..wgpu::Limits::downlevel_defaults().using_resolution(scheda.limits())
            },
            // Blocchi di memoria piccoli: con `Performance` i blocchi della memoria che la CPU vede partono da 64 MB
            // (4 MB con `MemoryUsage`) e il primo nasce già col dispositivo: misurato il 22/11, 60 MB in meno nel set
            // di lavoro. Qui le risorse si creano all'avvio, non nel giro: i blocchi grandi non servono.
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| format!("dispositivo: {e}"))?;
        crate::sistema::segna_memoria("dispositivo DirectX 12");

        let capacita = surface.get_capabilities(&scheda);
        // Formato non sRGB: i colori degli shader e di egui sono già in gamma.
        let formato = capacita
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
            .unwrap_or(capacita.formats[0]);
        let modi = capacita.present_modes.clone();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: formato,
            color_space: Default::default(),
            width: dimensioni.width.max(1),
            height: dimensioni.height.max(1),
            present_mode: scegli_presentazione(&modi, vsync),
            desired_maximum_frame_latency: 1,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        crate::sistema::segna_memoria("swapchain");

        let modulo = device.create_shader_module(wgpu::include_wgsl!("scena.wgsl"));
        let layout_gruppo = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniformi"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scena"),
            bind_group_layouts: &[Some(&layout_gruppo)],
            immediate_size: DIMENSIONE_IMMEDIATI,
        });
        let b_uniformi = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniformi"),
            size: std::mem::size_of::<Uniformi>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gruppo = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniformi"),
            layout: &layout_gruppo,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: b_uniformi.as_entire_binding() }],
        });

        let pipeline = |nome: &str,
                        vs: &str,
                        fs: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(nome),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &modulo,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &modulo,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: formato,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let p_stanza = pipeline(
            "stanza",
            "vs_stanza",
            "fs_stanza",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
            })],
            None,
        );
        let premoltiplicato = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let p_sfera = pipeline("sfera", "vs_sfera", "fs_sfera", &[], premoltiplicato);
        let p_rett = pipeline(
            "rettangoli",
            "vs_rett",
            "fs_rett",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Rett>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32, 3 => Float32x2],
            })],
            premoltiplicato,
        );

        crate::sistema::segna_memoria("shader e pipeline");
        let dati_stanza = stanza();
        let b_stanza = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("stanza"),
            contents: bytemuck::cast_slice(&dati_stanza),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let b_rett = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rettangoli"),
            size: (std::mem::size_of::<Rett>() * MAX_RETT) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let egui = egui_wgpu::Renderer::new(
            &device,
            formato,
            egui_wgpu::RendererOptions { dithering: false, ..Default::default() },
        );
        crate::sistema::segna_memoria("buffer e renderer di egui");

        Ok(Grafica {
            surface,
            device,
            queue,
            config,
            scheda: scheda.get_info(),
            egui,
            modi,
            p_stanza,
            p_sfera,
            p_rett,
            b_stanza,
            vertici_stanza: (dati_stanza.len() / 6) as u32,
            b_uniformi,
            uniformi_scritte: None,
            b_rett,
            gruppo,
            rett: [Rett::default(); MAX_RETT],
            n_rett: 0,
            rett_scritti: (u32::MAX, 0, 0, 0, false, None),
            scala: finestra_scala(),
            mirino: Mirino::default(),
            colori: ColoriBersaglio::default(),
            tema: Tema::Grafite,
        })
    }

    /// Mirino, colori dei bersagli e tema della stanza: vanno in scheda video al prossimo frame, una volta sola.
    pub fn imposta_aspetto(&mut self, m: Mirino, colori: ColoriBersaglio, tema: Tema) {
        if m != self.mirino {
            self.mirino = m;
            self.rett_scritti = (u32::MAX, 0, 0, 0, false, None);
        }
        self.colori = colori;
        self.tema = tema;
    }

    pub fn ridimensiona(&mut self, larghezza: u32, altezza: u32, scala: f32) {
        if larghezza == 0 || altezza == 0 {
            return;
        }
        self.config.width = larghezza;
        self.config.height = altezza;
        self.scala = scala.max(1.0);
        self.surface.configure(&self.device, &self.config);
        self.rett_scritti = (u32::MAX, 0, 0, 0, false, None);
    }

    pub fn imposta_vsync(&mut self, vsync: bool) {
        let modo = scegli_presentazione(&self.modi, vsync);
        if modo != self.config.present_mode {
            self.config.present_mode = modo;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn presentazione(&self) -> wgpu::PresentMode {
        self.config.present_mode
    }

    pub fn proporzione(&self) -> f64 {
        f64::from(self.config.width) / f64::from(self.config.height)
    }

    /// Il prossimo frame della swapchain (qui aspetta il "frame latency waitable object").
    pub fn acquisisci(&mut self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            _ => None,
        }
    }

    /// Disegna scena ed egui nel frame e lo presenta.
    pub fn disegna(
        &mut self,
        frame: wgpu::SurfaceTexture,
        scena: Option<&Scena>,
        strato: Option<StratoEgui>,
    ) {
        let vista = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let mut immediati = Immediati::zeroed();
        if let Some(s) = scena {
            let (secondi, frazione) = s.tempo.unwrap_or((0, 0.0));
            let guardata = s.sfere.and_then(|x| x.guardata).map_or(-1.0, |i| i as f32);
            let lampo = s.sfere.and_then(|x| x.lampo.map(|(_, a)| (x.n as f32, a)));
            // tempo = tempo di gioco, sfera guardata, opacità del lampo, indice dell'istanza del lampo (-1 nessuno).
            immediati = Immediati {
                vp: s.vp.to_cols_array_2d(),
                tempo: [
                    s.sfere.map_or(0.0, |x| x.tempo_s),
                    guardata,
                    lampo.map_or(0.0, |l| l.1),
                    lampo.map_or(-1.0, |l| l.0),
                ],
                destra: [s.destra.x as f32, s.destra.y as f32, s.destra.z as f32, 0.0],
                su: [s.su.x as f32, s.su.y as f32, s.su.z as f32, 0.0],
                schermo: [self.config.width as f32, self.config.height as f32, frazione, self.scala],
            };
            // Senza sfere (menu, conto alla rovescia) si tengono quelle di prima: niente scrittura in più. Lo stesso
            // per il posto del lampo quando il lampo è finito.
            let precedenti = self
                .uniformi_scritte
                .map_or(([0.0; 4], [SferaGpu::default(); MAX_SFERE + 1]), |u| (u.zona, u.sfere));
            let (zona, sfere) = s.sfere.map_or(precedenti, |x| {
                let mut a = precedenti.1;
                a[..MAX_SFERE].copy_from_slice(&x.dati);
                let n = (x.n as usize).min(MAX_SFERE);
                a[n] = x.lampo.map_or(precedenti.1[n], |l| l.0);
                (x.zona, a)
            });
            let stanza = self.tema.colori();
            let u = Uniformi {
                occhio: [s.occhio.x as f32, s.occhio.y as f32, s.occhio.z as f32, 0.0],
                bersaglio_normale: aspetto::vec4(self.colori.normale),
                bersaglio_guardato: aspetto::vec4(self.colori.guardato),
                bersaglio_colpito: aspetto::vec4(self.colori.colpito),
                pavimento: aspetto::vec4_f(stanza.pavimento),
                soffitto: aspetto::vec4_f(stanza.soffitto),
                pareti: aspetto::vec4_f(stanza.pareti),
                linee: aspetto::vec4_f(stanza.linee),
                zona,
                sfere,
            };
            if self.uniformi_scritte != Some(u) {
                self.queue.write_buffer(&self.b_uniformi, 0, bytemuck::bytes_of(&u));
                self.uniformi_scritte = Some(u);
            }
            let conto = s.conto.unwrap_or(0);
            let chiave =
                (secondi, conto, self.config.width, self.config.height, s.tempo.is_some(), s.dal_vivo);
            if chiave != self.rett_scritti {
                self.prepara_rettangoli(s.tempo.map(|t| t.0), s.conto, s.dal_vivo);
                self.queue.write_buffer(&self.b_rett, 0, bytemuck::cast_slice(&self.rett[..self.n_rett]));
                self.rett_scritti = chiave;
            }
        }

        let mut comandi_egui = Vec::new();
        if let Some(st) = &strato
            && st.nuove
        {
            comandi_egui =
                self.egui.update_buffers(&self.device, &self.queue, &mut encoder, st.primitive, st.schermo);
        }

        let fondo = self.tema.colori().fondo;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &vista,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(fondo[0]),
                            g: f64::from(fondo[1]),
                            b: f64::from(fondo[2]),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(s) = scena {
                pass.set_bind_group(0, &self.gruppo, &[]);
                pass.set_pipeline(&self.p_stanza);
                pass.set_immediates(0, bytemuck::bytes_of(&immediati));
                pass.set_vertex_buffer(0, self.b_stanza.slice(..));
                pass.draw(0..self.vertici_stanza, 0..1);
                if let Some(x) = s.sfere {
                    // Le sfere in campo e, se c'è, il lampo della sfera colpita: l'ultima istanza, quindi sopra le
                    // altre, con la sua opacità.
                    let n = x.n.min(MAX_SFERE as u32) + u32::from(x.lampo.is_some());
                    if n > 0 {
                        pass.set_pipeline(&self.p_sfera);
                        pass.draw(0..6, 0..n);
                    }
                }
                if self.n_rett > 0 {
                    pass.set_pipeline(&self.p_rett);
                    pass.set_vertex_buffer(0, self.b_rett.slice(..));
                    pass.draw(0..6, 0..self.n_rett as u32);
                }
            }
            if let Some(st) = &strato {
                let mut pass = pass.forget_lifetime();
                self.egui.render(&mut pass, st.primitive, st.schermo);
            }
        }

        if comandi_egui.is_empty() {
            self.queue.submit([encoder.finish()]);
        } else {
            comandi_egui.push(encoder.finish());
            self.queue.submit(comandi_egui);
        }
        self.queue.present(frame);
    }

    fn aggiungi(&mut self, x: f32, y: f32, w: f32, h: f32, colore: [f32; 4], tipo: f32) {
        if self.n_rett < MAX_RETT {
            self.rett[self.n_rett] = Rett { xywh: [x, y, w, h], colore, tipo, anello: [0.0; 2] };
            self.n_rett += 1;
        }
    }

    fn prepara_rettangoli(&mut self, secondi: Option<u32>, conto: Option<u32>, dal_vivo: DalVivo) {
        self.n_rett = 0;
        let s = self.scala;
        let (w, h) = (self.config.width as f32, self.config.height as f32);
        let cx = (w / 2.0).round();
        let cy = (h / 2.0).round();
        // Il mirino: la stessa geometria dell'anteprima del menu (`mirino::pezzi`).
        let mut pezzi = [mirino::Pezzo::default(); mirino::MAX_PEZZI];
        let n = mirino::pezzi(&self.mirino, cx, cy, s, &mut pezzi);
        for p in &pezzi[..n] {
            match p.anello {
                Some((interno, esterno)) => {
                    self.aggiungi(p.x, p.y, p.w, p.h, p.colore, 2.0);
                    self.rett[self.n_rett - 1].anello = [interno, esterno];
                }
                None => self.aggiungi(p.x, p.y, p.w, p.h, p.colore, 0.0),
            }
        }
        if let Some(sec) = secondi {
            // Il tempo in alto, a sette segmenti, e la barra che si accorcia.
            let ch = (30.0 * s).round();
            let cw = (17.0 * s).round();
            let t = (4.0 * s).round().max(2.0);
            let spazio = (7.0 * s).round();
            let y = (22.0 * s).round();
            if sec >= 100 {
                // Gli scenari dei file possono durare fino a 300 secondi: tre cifre, centrate.
                let largo = 3.0 * cw + 2.0 * (6.0 * s).round();
                self.numero(sec, (cx - largo / 2.0).round(), y, cw, ch, t, [0.92, 0.92, 0.92, 0.9]);
            } else {
                self.cifra(sec / 10, (cx - cw - spazio / 2.0).round(), y, cw, ch, t);
                self.cifra(sec % 10, (cx + spazio / 2.0).round(), y, cw, ch, t);
            }
            let lb = (220.0 * s).round();
            let hb = (3.0 * s).round().max(2.0);
            let yb = y + ch + (10.0 * s).round();
            let xb = (cx - lb / 2.0).round();
            self.aggiungi(xb, yb, lb, hb, [1.0, 1.0, 1.0, 0.18], 0.0);
            self.aggiungi(xb, yb, lb, hb, [1.0, 0.69, 0.13, 0.95], 1.0);
        }
        if let Some(n) = conto {
            // Il conto alla rovescia: una cifra grande sopra il mirino.
            let ch = (120.0 * s).round();
            let cw = (68.0 * s).round();
            let t = (14.0 * s).round().max(4.0);
            self.cifra(n % 10, (cx - cw / 2.0).round(), (cy - ch - 60.0 * s).round(), cw, ch, t);
        }
        if let Some((punti, precisione)) = dal_vivo {
            // In alto a sinistra: il punteggio grande e, sotto, la precisione in percento più piccola, in arancio.
            let x = (28.0 * s).round();
            let y = (22.0 * s).round();
            let (ch, cw, t) = ((30.0 * s).round(), (17.0 * s).round(), (4.0 * s).round().max(2.0));
            self.numero(punti, x, y, cw, ch, t, [0.92, 0.92, 0.92, 0.9]);
            if let Some(p) = precisione {
                let (ph, pw, pt) = ((18.0 * s).round(), (10.0 * s).round(), (3.0 * s).round().max(2.0));
                let yp = y + ch + (10.0 * s).round();
                let fine = self.numero(p, x, yp, pw, ph, pt, [1.0, 0.69, 0.13, 0.95]);
                // Il segno di percento: due quadratini e, fra loro, una diagonale a gradini.
                let arancio = [1.0, 0.69, 0.13, 0.95];
                let q = pt + 1.0;
                let xs = fine + (5.0 * s).round();
                let lw = pw + 2.0 * q;
                self.aggiungi(xs, yp, q, q, arancio, 0.0);
                self.aggiungi(xs + lw - q, yp + ph - q, q, q, arancio, 0.0);
                for i in 0..5 {
                    let f = i as f32 / 4.0;
                    let x = (xs + f * (lw - pt)).round();
                    let y = (yp + ph - pt - f * (ph - pt)).round();
                    self.aggiungi(x, y, pt, pt, arancio, 0.0);
                }
            }
        }
    }

    /// Un numero intero (al massimo 3 cifre, senza zeri davanti) da `x` verso destra; dice dove finisce.
    #[allow(clippy::too_many_arguments)]
    fn numero(&mut self, n: u32, x: f32, y: f32, w: f32, h: f32, t: f32, colore: [f32; 4]) -> f32 {
        let n = n.min(999);
        let cifre = if n >= 100 {
            3
        } else if n >= 10 {
            2
        } else {
            1
        };
        let passo = w + (6.0 * self.scala).round();
        let mut resto = n;
        for i in (0..cifre).rev() {
            self.cifra_colore(resto % 10, x + i as f32 * passo, y, w, h, t, colore);
            resto /= 10;
        }
        x + cifre as f32 * passo - (6.0 * self.scala).round()
    }

    fn cifra(&mut self, n: u32, x: f32, y: f32, w: f32, h: f32, t: f32) {
        self.cifra_colore(n, x, y, w, h, t, [0.92, 0.92, 0.92, 0.9]);
    }

    #[allow(clippy::too_many_arguments)]
    fn cifra_colore(&mut self, n: u32, x: f32, y: f32, w: f32, h: f32, t: f32, c: [f32; 4]) {
        let m = SEGMENTI[n as usize % 10];
        let meta = h / 2.0;
        let lato = meta - 1.5 * t;
        let segmenti = [
            (0b1000000, x + t, y, w - 2.0 * t, t),
            (0b0100000, x + w - t, y + t, t, lato),
            (0b0010000, x + w - t, y + meta + 0.5 * t, t, lato),
            (0b0001000, x + t, y + h - t, w - 2.0 * t, t),
            (0b0000100, x, y + meta + 0.5 * t, t, lato),
            (0b0000010, x, y + t, t, lato),
            (0b0000001, x + t, y + meta - 0.5 * t, w - 2.0 * t, t),
        ];
        for (bit, sx, sy, sw, sh) in segmenti {
            if m & bit != 0 {
                self.aggiungi(sx, sy, sw, sh, c, 0.0);
            }
        }
    }
}

fn finestra_scala() -> f32 {
    1.0
}

#[cfg(test)]
mod test {
    use super::*;

    /// Lo shader si legge, è valido e si traduce in HLSL come fa wgpu su DirectX 12: un errore si vede nei test, non
    /// all'avvio del gioco.
    #[test]
    fn lo_shader_e_valido_e_diventa_hlsl() {
        let modulo = naga::front::wgsl::parse_str(include_str!("scena.wgsl")).expect("WGSL");
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&modulo)
        .expect("shader valido");
        let opzioni = naga::back::hlsl::Options {
            immediates_target: Some(naga::back::hlsl::BindTarget::default()),
            ..Default::default()
        };
        let mut hlsl = String::new();
        naga::back::hlsl::Writer::new(&mut hlsl, &opzioni, &Default::default())
            .write(&modulo, &info, None)
            .expect("HLSL");
        assert!(hlsl.contains("vs_sfera") && hlsl.contains("fs_sfera"));
    }

    #[test]
    fn i_dati_dello_shader_hanno_le_misure_giuste() {
        assert_eq!(std::mem::size_of::<Immediati>(), 128);
        // occhio, 7 colori, zona e 9 sfere da 32 byte (8 più il lampo): come `Uniformi` in scena.wgsl.
        assert_eq!(std::mem::size_of::<Uniformi>(), 16 * 8 + 16 + 32 * (MAX_SFERE + 1));
    }
}
