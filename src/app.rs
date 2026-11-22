//! Il programma: finestra, stati (menu, giro, pausa, riepilogo) e il ciclo dei frame.
//!
//! Consumi:
//! - nel giro il ciclo gira a vuoto (`ControlFlow::Poll`) e disegna il più spesso possibile, oppure al ritmo del
//!   vsync o del limite di fps;
//! - nel menu, in pausa e nel riepilogo non c'è ciclo: si ridisegna solo quando egui lo chiede (un clic, un tasto,
//!   un movimento del mouse sul pannello), con `ControlFlow::Wait`;
//! - finestra non in primo piano o ridotta a icona: il giro va in pausa e non si disegna niente.
//!
//! Il frame del giro (`frame_giro`): limite di fps (se c'è) → `get_current_texture` (qui aspetta la swapchain) →
//! i clic, ognuno valutato con la visuale del suo istante → il movimento letto all'ultimo momento → matrici →
//! disegno → `present`. Il pannello F3, se aperto, si ricalcola dopo `present`, fuori dal tratto fra mouse e
//! schermo. Nel frame il nostro codice non alloca.

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::DVec3;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::monitor::{MonitorHandle, VideoModeHandle};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

use crate::aggiornamento::{self, Aggiornamento};
use crate::banco::Banco;
use crate::camera;
use crate::config::{self, Config};
use crate::giro::{Caso, Esito, Giro, Riepilogo};
use crate::grafica::{Grafica, Scena, StratoEgui};
use crate::ingresso::{Clic, Ingresso};
use crate::interfaccia::{self, Azione, InfoMenu};
use crate::latenza::{CAMPIONI_PANNELLO, Latenza, testo_pannello};
use crate::registro::Registro;
use crate::sensibilita::{fov_verticale, radianti_per_count};
use crate::sistema;
use crate::storico::{self, Confronto, GiroSalvato, Ora, Storico};
use crate::tempo;
use crate::{esporta, giro};

const OCCHIO: DVec3 = DVec3::new(0.0, 1.7, 0.0);
/// I frame al secondo, al massimo, nel menu, in pausa e nel riepilogo.
const FPS_MENU: f64 = 60.0;
/// Il lato massimo delle texture di egui, quindi la larghezza dell'atlante dei caratteri.
const LATO_ATLANTE: usize = 2048;
/// Il pannello della latenza nel giro si ricalcola 4 volte al secondo.
const AGGIORNA_PANNELLO_MS: f64 = 250.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stato {
    Menu,
    Giro,
    Pausa,
    Riepilogo(Riepilogo),
    Statistiche,
}

/// Lo strato egui disegnato sopra il giro (pannello della latenza), ricalcolato ogni tanto.
struct PannelloGiro {
    primitive: Vec<egui::ClippedPrimitive>,
    schermo: egui_wgpu::ScreenDescriptor,
    prossimo: i64,
    nuove: bool,
}

pub struct App {
    pub config: Config,
    percorso_config: std::path::PathBuf,
    avviso: Option<String>,
    pub finestra: Option<Arc<Window>>,
    pub grafica: Option<Grafica>,
    egui_ctx: egui::Context,
    egui_stato: Option<egui_winit::State>,
    pub ingresso: Arc<Ingresso>,
    pub giro: Giro,
    /// Tempi, distanze e overshoot dei bersagli del giro: la memoria è preparata qui, nel frame non alloca.
    registro: Registro,
    /// I giri salvati; il giro finito si aggiunge e si scrive in `fine_giro`. Si leggono in un thread partito
    /// all'avvio (`storico_in_arrivo`), così con migliaia di file la finestra non aspetta: vedi `storico()`.
    storico: Option<Storico>,
    storico_in_arrivo: Option<std::thread::JoinHandle<Storico>>,
    /// Quando è partito aimpro: i giri da qui in poi sono la sessione.
    avvio_ms: i64,
    /// L'ultimo giro rispetto al record di prima (nel riepilogo); `None` col banco, che non salva i giri.
    confronto: Option<Confronto>,
    avviso_storico: Option<String>,
    /// L'esito dell'ultimo "Esporta CSV", mostrato nella schermata Statistiche.
    esportato: Option<String>,
    pub latenza: Latenza,
    pub stato: Stato,
    imbardata: f64,
    beccheggio: f64,
    applicati: (i64, i64),
    rad_per_count: f64,
    fov_y: f64,
    origine: i64,
    clic: [Clic; 256],
    pub pannello_visibile: bool,
    pannello: Option<PannelloGiro>,
    pub in_primo_piano: bool,
    pub ridotta: bool,
    attesa: tempo::Attesa,
    prossima_scadenza: i64,
    prossimo_ridisegno: Option<Instant>,
    /// Impostazioni cambiate e non ancora scritte: si scrivono a mouse rilasciato, non a ogni frame di un
    /// trascinamento.
    da_salvare: bool,
    /// Il testo del pannello F3 fuori dal giro, con il numero di frame misurati da cui viene.
    testo_pannello_menu: Option<((u64, u64), String)>,
    ultimo_frame_menu: Option<Instant>,
    /// Frame disegnati dall'avvio (giro e menu).
    pub frame_disegnati: u64,
    /// Allocazioni nel frame del giro, da `get_current_texture` al ritorno di `present`: quelle dentro wgpu e
    /// quelle del codice di aimpro. Il pannello F3 si prepara dopo e non entra in questi conti.
    pub alloc_wgpu: u64,
    pub alloc_nostre: u64,
    pub hz_schermo: f64,
    pub banco: Option<Banco>,
    /// Il controllo degli aggiornamenti e il download, nel loro thread; il menu ne mostra lo stato.
    aggiornamento: Arc<Aggiornamento>,
}

/// Carica in scheda video le texture nuove o cambiate di egui.
fn carica_texture(g: &mut Grafica, delta: &mut egui::TexturesDelta) {
    for (id, delte) in &delta.set {
        for d in delte {
            g.egui.update_texture(&g.device, &g.queue, *id, d);
        }
    }
    delta.set.clear();
}

/// Libera le texture che egui non usa più (dopo il disegno).
fn libera_texture(g: &mut Grafica, delta: &mut egui::TexturesDelta) {
    for id in delta.free.drain() {
        g.egui.free_texture(&id);
    }
}

fn modo_migliore(monitor: &MonitorHandle) -> Option<VideoModeHandle> {
    let dimensioni = monitor.size();
    monitor
        .video_modes()
        .filter(|m| m.size() == dimensioni)
        .max_by_key(|m| (m.refresh_rate_millihertz(), m.bit_depth()))
}

impl App {
    pub fn new(banco: Option<Banco>, proxy: EventLoopProxy<()>) -> Self {
        let percorso_config = config::percorso_predefinito();
        let (config, avviso) = config::carica(&percorso_config);
        // Il banco non va in rete: niente che si mescoli alle sue misure. Il thread sveglia il menu con un evento.
        let aggiornamento = Aggiornamento::avvia(
            banco.is_none() && config.controlla_aggiornamenti,
            Box::new(move || {
                let _ = proxy.send_event(());
            }),
        );
        let egui_ctx = egui::Context::default();
        interfaccia::stile(&egui_ctx);
        // La sessione parte adesso, anche se i file dello storico finiscono di leggersi dopo.
        let avvio = storico::istante_ms();
        let mut app = App {
            config,
            percorso_config,
            avviso,
            finestra: None,
            grafica: None,
            egui_ctx,
            egui_stato: None,
            ingresso: Ingresso::avvia(),
            giro: Giro::new(OCCHIO, Caso::new(tempo::qpc() as u64)),
            registro: Registro::new(OCCHIO),
            storico: None,
            // Il banco non usa lo storico: niente lettura che si mescoli alle sue misure.
            storico_in_arrivo: banco
                .is_none()
                .then(|| std::thread::spawn(move || Storico::carica(storico::cartella_predefinita(), avvio))),
            avvio_ms: avvio,
            confronto: None,
            avviso_storico: None,
            esportato: None,
            latenza: Latenza::new(),
            stato: Stato::Menu,
            imbardata: 0.0,
            beccheggio: 0.0,
            applicati: (0, 0),
            rad_per_count: 0.0,
            fov_y: 1.0,
            origine: tempo::qpc(),
            clic: [Clic::default(); 256],
            pannello_visibile: false,
            pannello: None,
            in_primo_piano: true,
            ridotta: false,
            attesa: tempo::Attesa::new(),
            prossima_scadenza: 0,
            prossimo_ridisegno: None,
            da_salvare: false,
            testo_pannello_menu: None,
            ultimo_frame_menu: None,
            frame_disegnati: 0,
            alloc_wgpu: 0,
            alloc_nostre: 0,
            hz_schermo: 0.0,
            banco,
            aggiornamento,
        };
        app.applica_config();
        app
    }

    fn applica_config(&mut self) {
        self.rad_per_count = radianti_per_count(self.config.gioco, self.config.sensibilita);
        self.fov_y = fov_verticale(self.config.gioco, self.config.fov);
        if let Some(g) = self.grafica.as_mut() {
            g.imposta_vsync(self.config.vsync);
        }
        self.prossima_scadenza = 0;
    }

    fn salva_config(&mut self) {
        self.da_salvare = false;
        if self.banco.is_some() {
            return; // il banco cambia vsync e limite solo per sé
        }
        match config::salva(&self.percorso_config, &self.config) {
            Ok(()) => self.avviso = None,
            Err(e) => self.avviso = Some(format!("impostazioni non salvate: {e}")),
        }
    }

    fn salva_se_serve(&mut self) {
        if self.da_salvare {
            self.salva_config();
        }
    }

    /// I clic rimasti in coda da prima (fra una pausa e la ripresa) non contano.
    fn scarta_clic(&mut self) {
        let _ = self.ingresso.prendi_clic(&mut self.clic);
    }

    fn ms(&self, q: i64) -> f64 {
        tempo::ms(q - self.origine)
    }

    /// Chiede un frame del menu. Non disegna subito: lo fa `about_to_wait`, al massimo 60 volte al secondo, così un
    /// mouse a 1000 Hz sopra il menu non diventa 1000 frame al secondo.
    fn ridisegna(&mut self) {
        let ora = Instant::now();
        self.prossimo_ridisegno = Some(self.prossimo_ridisegno.map_or(ora, |t| t.min(ora)));
    }

    /// Quanti frame al secondo, al massimo, fuori dal giro: 60 (scelta di Carlo, 22/11), o la frequenza dello
    /// schermo se è più bassa. Il menu non è il giro: più frame consumano CPU senza servire a niente.
    pub fn fps_menu(&self) -> f64 {
        if self.hz_schermo > 0.0 { self.hz_schermo.min(FPS_MENU) } else { FPS_MENU }
    }

    /// Il tempo minimo fra due frame del menu.
    fn intervallo_menu(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.fps_menu())
    }

    /// Nel giro il cursore di Windows è nascosto e fermo al centro della finestra; fuori è libero.
    fn cursore_di_gioco(&self, gioco: bool) {
        // Entra ed esce dal giro sempre da qui: la rete dell'aggiornamento si ferma finché si gioca.
        aggiornamento::nel_giro(gioco);
        if let Some(f) = &self.finestra {
            f.set_cursor_visible(!gioco);
            let _ = f.set_cursor_grab(if gioco { CursorGrabMode::Confined } else { CursorGrabMode::None });
            if gioco {
                sistema::blocca_cursore(sistema::hwnd_di(f), 1);
            } else {
                sistema::libera_cursore();
            }
        }
    }

    fn rilegge_base(&mut self) {
        let l = self.ingresso.leggi();
        self.applicati = (l.dx, l.dy);
    }

    pub fn inizia_giro(&mut self) {
        self.salva_se_serve();
        self.ingresso.imposta_attivo(true);
        self.rilegge_base();
        self.scarta_clic();
        self.imbardata = 0.0;
        self.beccheggio = 0.0;
        self.giro.inizia(self.ms(tempo::qpc()));
        self.registro.inizia(&self.giro, camera::avanti(self.imbardata, self.beccheggio));
        self.latenza.azzera();
        self.prossima_scadenza = 0;
        self.stato = Stato::Giro;
        self.cursore_di_gioco(true);
    }

    pub fn pausa(&mut self) {
        if self.stato != Stato::Giro {
            return;
        }
        self.ingresso.imposta_attivo(false);
        self.giro.pausa(self.ms(tempo::qpc()));
        self.stato = Stato::Pausa;
        self.cursore_di_gioco(false);
        self.ridisegna();
    }

    pub fn riprendi(&mut self) {
        if self.stato != Stato::Pausa {
            return;
        }
        self.ingresso.imposta_attivo(true);
        self.rilegge_base();
        self.scarta_clic();
        self.latenza.riparti();
        self.giro.riprendi(self.ms(tempo::qpc()));
        self.prossima_scadenza = 0;
        self.stato = Stato::Giro;
        self.cursore_di_gioco(true);
    }

    pub fn torna_al_menu(&mut self) {
        self.ingresso.imposta_attivo(false);
        self.giro.annulla();
        self.stato = Stato::Menu;
        self.cursore_di_gioco(false);
        self.ridisegna();
    }

    /// Chiude il giro in corso e mostra il riepilogo (lo usa il banco).
    pub fn termina_giro(&mut self) {
        if matches!(self.stato, Stato::Giro | Stato::Pausa) {
            self.fine_giro();
        }
    }

    pub fn mostra_pannello(&mut self, visibile: bool) {
        if self.pannello_visibile != visibile {
            self.pannello_visibile = visibile;
            self.pannello = None;
            self.ridisegna();
        }
    }

    fn fine_giro(&mut self) {
        self.ingresso.imposta_attivo(false);
        let r = self.giro.riepilogo();
        self.salva_giro(&r);
        self.stato = Stato::Riepilogo(r);
        self.cursore_di_gioco(false);
        self.ridisegna();
    }

    /// Il giro finito va nello storico: dopo il present dell'ultimo frame, fuori dal tratto fra mouse e schermo.
    /// Il banco non salva: i suoi giri sono tagliati a metà e lo storico è di chi gioca.
    fn salva_giro(&mut self, r: &Riepilogo) {
        self.confronto = None;
        self.avviso_storico = None;
        if self.banco.is_some() {
            return;
        }
        let ora = Ora::adesso();
        let g = GiroSalvato::nuovo(
            storico::SCATTO,
            giro::DURATA_GIRO_MS / 1000.0,
            r,
            &self.registro,
            &self.config,
            ora,
            storico::istante_ms(),
        );
        let (confronto, scritto) = self.storico().aggiungi(g, ora);
        self.confronto = Some(confronto);
        if let Err(e) = scritto {
            self.avviso_storico = Some(format!("giro non salvato nello storico: {e}"));
        }
    }

    /// Lo storico letto dal thread partito all'avvio; se non ha ancora finito, lo aspetta.
    fn storico(&mut self) -> &mut Storico {
        let (in_arrivo, avvio) = (&mut self.storico_in_arrivo, self.avvio_ms);
        self.storico.get_or_insert_with(|| {
            in_arrivo
                .take()
                .and_then(|t| t.join().ok())
                .unwrap_or_else(|| Storico::carica(storico::cartella_predefinita(), avvio))
        })
    }

    fn esporta_csv(&mut self) {
        let cartella = esporta::documenti().join("aimpro");
        self.esportato = Some(match esporta::esporta(self.storico().giri(), &cartella, Ora::adesso()) {
            Ok(p) => format!("Esportato in {}", p.display()),
            Err(e) => format!("Esportazione non riuscita in {}: {e}", cartella.display()),
        });
    }

    /// Applica i count fino ai totali `(dx, dy)`.
    #[inline]
    fn applica(&mut self, dx: i64, dy: i64) {
        let (ddx, ddy) = (dx - self.applicati.0, dy - self.applicati.1);
        self.imbardata += ddx as f64 * self.rad_per_count;
        self.beccheggio = camera::limita_beccheggio(self.beccheggio - ddy as f64 * self.rad_per_count);
        self.applicati = (dx, dy);
    }

    fn scena(&self, tempo_ms: f64, nel_giro: bool) -> Scena {
        let proporzione = self.grafica.as_ref().map_or(16.0 / 9.0, Grafica::proporzione);
        let avanti = camera::avanti(self.imbardata, self.beccheggio);
        let destra = camera::destra(self.imbardata);
        let restante = self.giro.restante_ms(tempo_ms);
        Scena {
            vp: camera::vista_proiezione(OCCHIO, self.imbardata, self.beccheggio, self.fov_y, proporzione),
            occhio: OCCHIO,
            destra,
            su: destra.cross(avanti),
            sfera: nel_giro.then(|| (self.giro.bersaglio, self.giro.raggio())),
            tempo: nel_giro.then(|| {
                ((restante / 1000.0).ceil() as u32, (restante / crate::giro::DURATA_GIRO_MS) as f32)
            }),
        }
    }

    /// Un frame del giro. Il nostro codice qui non alloca (il pannello F3, se aperto, si prepara dopo il present).
    pub fn frame_giro(&mut self) {
        let mut inizio = tempo::qpc();
        if self.config.limite_fps > 0 && !self.config.vsync {
            let periodo = tempo::tick(1000.0 / f64::from(self.config.limite_fps));
            if self.prossima_scadenza == 0 || inizio - self.prossima_scadenza > periodo {
                self.prossima_scadenza = inizio;
            } else {
                self.attesa.fino_a(self.prossima_scadenza);
                inizio = tempo::qpc();
            }
            self.prossima_scadenza += periodo;
        }
        let a0 = crate::allocazioni::totale();
        self.latenza.inizio_frame(inizio);
        let Some(frame) = self.grafica.as_mut().and_then(Grafica::acquisisci) else {
            return;
        };
        let a1 = crate::allocazioni::totale();
        let acquisito = tempo::qpc();

        // Il mouse all'ultimo momento: prima i clic (con la visuale del loro istante), poi il movimento.
        let n = self.ingresso.prendi_clic(&mut self.clic);
        let lettura = self.ingresso.leggi();
        let letto = tempo::qpc();
        let mut primo_clic = 0;
        for i in 0..n {
            let c = self.clic[i];
            self.applica(c.dx, c.dy);
            let d = camera::avanti(self.imbardata, self.beccheggio);
            let esito = self.giro.clic(self.ms(c.qpc), d);
            self.registro.clic(&self.giro, esito, d);
            if primo_clic == 0 && esito != Esito::Ignorato {
                primo_clic = c.qpc;
            }
        }
        self.applica(lettura.dx, lettura.dy);
        self.registro.osserva(&self.giro, camera::avanti(self.imbardata, self.beccheggio));

        let adesso = self.ms(letto);
        let finito = self.giro.aggiorna(adesso);
        let scena = self.scena(adesso, true);
        let strato = self.pannello.as_ref().filter(|_| self.pannello_visibile).map(|p| StratoEgui {
            primitive: &p.primitive,
            schermo: &p.schermo,
            nuove: p.nuove,
        });
        if let Some(g) = self.grafica.as_mut() {
            let a3 = crate::allocazioni::totale();
            g.disegna(frame, Some(&scena), strato);
            let a4 = crate::allocazioni::totale();
            // Le allocazioni dentro wgpu (acquisizione, encoder, submit, present) e le nostre, contate a parte.
            self.alloc_wgpu += (a1 - a0) + (a4 - a3);
            self.alloc_nostre += a3 - a1;
        }
        if let Some(p) = self.pannello.as_mut() {
            p.nuove = false;
        }
        let presentato = tempo::qpc();
        self.frame_disegnati += 1;
        self.latenza.fine_frame(presentato, acquisito, letto, lettura.primo, lettura.ultimo, primo_clic);
        // Il pannello si prepara qui, dopo il present: si vedrà nel frame dopo, e il suo costo non sta fra la
        // lettura del mouse e lo schermo.
        self.prepara_pannello_giro(presentato);
        static PRIMO_GIRO: std::sync::Once = std::sync::Once::new();
        PRIMO_GIRO.call_once(|| sistema::segna_memoria("primo frame del giro"));
        if finito {
            self.fine_giro();
        }
    }

    fn prepara_pannello_giro(&mut self, ora: i64) {
        if !self.pannello_visibile {
            return;
        }
        if self.pannello.as_ref().is_some_and(|p| ora < p.prossimo) {
            return;
        }
        let presentazione =
            self.grafica.as_ref().map_or(String::new(), |g| format!("{:?}", g.presentazione()));
        let misure = self.latenza.misure_recenti(CAMPIONI_PANNELLO);
        let testo = testo_pannello(&misure, &presentazione, self.config.limite_fps, &self.riga_ingresso());
        let (Some(finestra), Some(stato)) = (self.finestra.clone(), self.egui_stato.as_mut()) else {
            return;
        };
        let input = stato.take_egui_input(&finestra);
        let mut uscita = self.egui_ctx.run_ui(input, |ui| interfaccia::pannello_latenza(ui, &testo));
        let Some(g) = self.grafica.as_mut() else {
            uscita.textures_delta.clear();
            return;
        };
        carica_texture(g, &mut uscita.textures_delta);
        libera_texture(g, &mut uscita.textures_delta);
        let primitive = self.egui_ctx.tessellate(uscita.shapes, uscita.pixels_per_point);
        let schermo = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [g.config.width, g.config.height],
            pixels_per_point: uscita.pixels_per_point,
        };
        self.pannello = Some(PannelloGiro {
            primitive,
            schermo,
            prossimo: ora + tempo::tick(AGGIORNA_PANNELLO_MS),
            nuove: true,
        });
    }

    fn riga_ingresso(&self) -> String {
        format!(
            "eventi mouse      {} (simulati {}, assoluti ignorati {})\n",
            self.ingresso.eventi.load(std::sync::atomic::Ordering::Relaxed),
            self.ingresso.simulati.load(std::sync::atomic::Ordering::Relaxed),
            self.ingresso.assoluti.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Un frame fuori dal giro: la stanza ferma e i pannelli di egui. Solo quando serve.
    fn frame_menu(&mut self) {
        self.ultimo_frame_menu = Some(Instant::now());
        let (Some(finestra), Some(stato)) = (self.finestra.clone(), self.egui_stato.as_mut()) else {
            return;
        };
        let input = stato.take_egui_input(&finestra);
        let mut cfg = self.config.clone();
        let stato_ora = self.stato;
        let restanti = (self.giro.restante_ms(self.ms(tempo::qpc())) / 1000.0).ceil() as u32;
        let (scheda, presentazione) = self.grafica.as_ref().map_or((String::new(), String::new()), |g| {
            (g.scheda.name.clone(), format!("{:?}", g.presentazione()))
        });
        let avviso = self.avviso.clone();
        let (confronto, avviso_storico, esportato) =
            (self.confronto, self.avviso_storico.clone(), self.esportato.clone());
        if stato_ora == Stato::Statistiche {
            self.storico();
        }
        let storico = self.storico.as_ref();
        // Fuori dal giro le misure non cambiano: il testo si ricalcola solo se ci sono frame nuovi.
        let pannello = if self.pannello_visibile {
            let n = self.latenza.versione();
            if self.testo_pannello_menu.as_ref().is_none_or(|(m, _)| *m != n) {
                let m = self.latenza.misure_recenti(CAMPIONI_PANNELLO);
                let t = testo_pannello(&m, &presentazione, self.config.limite_fps, "");
                self.testo_pannello_menu = Some((n, t));
            }
            self.testo_pannello_menu.as_ref().map(|(_, t)| t.clone())
        } else {
            None
        };
        let stato_aggiornamento = self.aggiornamento.stato();
        let mut azione = Azione::Nessuna;
        let mut uscita = self.egui_ctx.run_ui(input, |ui| {
            azione = match stato_ora {
                Stato::Menu => interfaccia::menu(
                    ui,
                    &mut cfg,
                    &InfoMenu {
                        scheda: &scheda,
                        presentazione: &presentazione,
                        avviso: avviso.as_deref(),
                        build: self.aggiornamento.attuale(),
                        aggiornamento: &stato_aggiornamento,
                    },
                ),
                Stato::Pausa => interfaccia::pausa(ui, restanti),
                Stato::Riepilogo(r) => {
                    interfaccia::riepilogo(ui, &r, confronto.as_ref(), avviso_storico.as_deref())
                }
                Stato::Statistiche => storico
                    .map_or(Azione::TornaAlMenu, |s| interfaccia::statistiche(ui, s, esportato.as_deref())),
                Stato::Giro => Azione::Nessuna,
            };
            if let Some(t) = &pannello {
                interfaccia::pannello_latenza(ui, t);
            }
        });
        if let Some(stato) = self.egui_stato.as_mut() {
            stato.handle_platform_output(&finestra, uscita.platform_output);
        }
        let ritardo = uscita.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay);
        self.prossimo_ridisegno = match ritardo {
            Some(d) if d.is_zero() => Some(Instant::now()),
            Some(d) if d < Duration::from_secs(3600) => Some(Instant::now() + d),
            _ => None,
        };

        // La stanza dietro i pannelli; in pausa anche la sfera e il tempo fermi.
        let scena = self.scena(self.ms(tempo::qpc()), matches!(stato_ora, Stato::Pausa));
        if let Some(g) = self.grafica.as_mut() {
            carica_texture(g, &mut uscita.textures_delta);
            let primitive = self.egui_ctx.tessellate(uscita.shapes, uscita.pixels_per_point);
            let schermo = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [g.config.width, g.config.height],
                pixels_per_point: uscita.pixels_per_point,
            };
            if let Some(frame) = g.acquisisci() {
                g.disegna(
                    frame,
                    Some(&scena),
                    Some(StratoEgui { primitive: &primitive, schermo: &schermo, nuove: true }),
                );
                self.frame_disegnati += 1;
                static PRIMO_MENU: std::sync::Once = std::sync::Once::new();
                PRIMO_MENU
                    .call_once(|| sistema::segna_memoria("primo frame del menu (caratteri e testo di egui)"));
            }
            libera_texture(g, &mut uscita.textures_delta);
        }
        uscita.textures_delta.clear();
        // Il pannello del giro va ricalcolato: le texture di egui possono essere cambiate.
        self.pannello = None;

        if cfg != self.config {
            self.config = cfg;
        }
        self.esegui(azione);
        // Si scrive il file solo a mouse rilasciato: trascinare un valore cambia le impostazioni a ogni frame.
        if self.da_salvare && !self.egui_ctx.input(|i| i.pointer.any_down()) {
            self.salva_config();
        }
    }

    pub fn esegui(&mut self, azione: Azione) {
        match azione {
            Azione::Nessuna => {}
            Azione::Impostazioni => {
                self.config = self.config.clone().normalizzata();
                self.applica_config();
                self.da_salvare = true;
                self.testo_pannello_menu = None;
                self.ridisegna();
            }
            Azione::Inizia => self.inizia_giro(),
            Azione::Riprendi => self.riprendi(),
            Azione::EsciDalGiro | Azione::TornaAlMenu => self.torna_al_menu(),
            Azione::Statistiche => {
                self.salva_se_serve();
                self.torna_al_menu();
                self.esportato = None;
                self.stato = Stato::Statistiche;
            }
            Azione::EsportaCsv => {
                self.esporta_csv();
                self.ridisegna();
            }
            Azione::Aggiorna => {
                self.salva_se_serve();
                self.aggiornamento.scarica_e_sostituisci();
                self.ridisegna();
            }
            Azione::Chiudi => {
                self.salva_se_serve();
                aggiornamento::aspetta_sostituzione();
                std::process::exit(0)
            }
        }
    }

    /// La presentazione in uso è col vsync (Fifo)? La legge dalla swapchain, non dalle impostazioni.
    pub fn vsync(&self) -> bool {
        self.grafica.as_ref().is_some_and(|g| g.presentazione() == wgpu::PresentMode::Fifo)
    }

    pub fn imposta_vsync_e_limite(&mut self, vsync: bool, limite: u32) {
        self.config.vsync = vsync;
        self.config.limite_fps = limite;
        self.applica_config();
    }

    fn deve_disegnare_il_giro(&self) -> bool {
        self.stato == Stato::Giro && self.in_primo_piano && !self.ridotta
    }
}

impl ApplicationHandler for App {
    /// Il thread dell'aggiornamento ha cambiato stato: si ridisegna il menu; a .exe nuovo pronto, si salva, lo si
    /// avvia e si esce.
    fn user_event(&mut self, _el: &ActiveEventLoop, _evento: ()) {
        if matches!(self.aggiornamento.stato(), aggiornamento::Stato::Pronto(_)) && self.stato != Stato::Giro
        {
            self.salva_se_serve();
            if self.aggiornamento.riavvia().is_ok() {
                std::process::exit(0);
            }
        }
        self.ridisegna();
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.finestra.is_some() {
            return;
        }
        let monitor = el.primary_monitor().or_else(|| el.available_monitors().next());
        self.hz_schermo =
            monitor.as_ref().and_then(|m| m.refresh_rate_millihertz()).map_or(0.0, |h| f64::from(h) / 1000.0);
        let schermo_intero = if std::env::args().any(|a| a == "--finestra") {
            None
        } else {
            match (&monitor, self.config.schermo_intero_esclusivo) {
                (Some(m), true) => modo_migliore(m)
                    .map(Fullscreen::Exclusive)
                    .or(Some(Fullscreen::Borderless(Some(m.clone())))),
                (m, _) => Some(Fullscreen::Borderless(m.clone())),
            }
        };
        if let Some(Fullscreen::Exclusive(m)) = &schermo_intero {
            self.hz_schermo = f64::from(m.refresh_rate_millihertz()) / 1000.0;
        }
        // La swapchain DXGI non si crea su una finestra già in modalità esclusiva (DXGI_ERROR_INVALID_CALL):
        // si parte senza bordi a tutto schermo e si passa all'esclusivo subito dopo.
        let esclusivo = match &schermo_intero {
            Some(Fullscreen::Exclusive(_)) => schermo_intero.clone(),
            _ => None,
        };
        let iniziale = match schermo_intero {
            Some(Fullscreen::Exclusive(_)) => Some(Fullscreen::Borderless(monitor.clone())),
            altro => altro,
        };
        let attributi = Window::default_attributes().with_title("aimpro").with_fullscreen(iniziale);
        let finestra = match el.create_window(attributi) {
            Ok(f) => Arc::new(f),
            Err(e) => {
                sistema::errore_fatale(&format!("la finestra non si crea: {e}"), self.banco.is_some());
                el.exit();
                return;
            }
        };
        match Grafica::new(finestra.clone(), self.config.vsync) {
            Ok(g) => self.grafica = Some(g),
            Err(e) => {
                sistema::errore_fatale(&format!("la grafica non parte: {e}"), self.banco.is_some());
                el.exit();
                return;
            }
        }
        let stato = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &finestra,
            Some(finestra.scale_factor() as f32),
            None,
            // egui fa l'atlante dei caratteri largo quanto il lato massimo delle texture: con quello della scheda
            // (16 384 pixel) sono 16 384 x 32 pixel già al primo frame, in RAM e da caricare in scheda video.
            // 2048 bastano per i pochi caratteri del menu.
            self.grafica
                .as_ref()
                .map(|g| (g.device.limits().max_texture_dimension_2d as usize).min(LATO_ATLANTE)),
        );
        self.egui_stato = Some(stato);
        if let Some(g) = self.grafica.as_mut() {
            let d = finestra.inner_size();
            g.ridimensiona(d.width, d.height, finestra.scale_factor() as f32);
        }
        if esclusivo.is_some() {
            finestra.set_fullscreen(esclusivo);
        }
        self.finestra = Some(finestra.clone());
        if let Some(b) = self.banco.as_mut() {
            b.avvia(crate::sistema::hwnd_di(&finestra));
        }
        finestra.request_redraw();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, evento: WindowEvent) {
        let Some(finestra) = self.finestra.clone() else {
            return;
        };
        let nel_giro = self.stato == Stato::Giro;
        // RedrawRequested non è un input: egui-winit risponde "ridisegna" anche a quello, e il menu si
        // richiamerebbe da solo all'infinito (misurato: 900 frame al secondo a menu fermo).
        let input = !nel_giro && !matches!(evento, WindowEvent::RedrawRequested);
        if input && let Some(stato) = self.egui_stato.as_mut() {
            let risposta = stato.on_window_event(&finestra, &evento);
            if risposta.repaint {
                self.ridisegna();
            }
        }
        match evento {
            WindowEvent::CloseRequested => {
                self.salva_se_serve();
                el.exit();
            }
            WindowEvent::Resized(d) => {
                if let Some(g) = self.grafica.as_mut() {
                    g.ridimensiona(d.width, d.height, finestra.scale_factor() as f32);
                }
                self.ridotta = d.width == 0 || d.height == 0;
                if self.ridotta {
                    self.pausa();
                }
                self.pannello = None;
                self.ridisegna();
            }
            WindowEvent::Focused(f) => {
                self.in_primo_piano = f;
                if !f {
                    self.pausa();
                }
            }
            WindowEvent::Occluded(o) => {
                self.ridotta = o;
                if o {
                    self.pausa();
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => match self.stato {
                        Stato::Giro => self.pausa(),
                        Stato::Pausa => self.riprendi(),
                        Stato::Statistiche => self.torna_al_menu(),
                        _ => {}
                    },
                    PhysicalKey::Code(KeyCode::F3) => {
                        self.pannello_visibile = !self.pannello_visibile;
                        self.pannello = None;
                        self.ridisegna();
                    }
                    _ => {}
                }
            }
            // Fuori dal giro al massimo 60 frame al secondo, anche quando Windows chiede di ridisegnare da sé
            // (succede a raffica mentre il mouse si muove sopra la finestra): il frame in più si rimanda.
            WindowEvent::RedrawRequested if !nel_giro && !self.ridotta => {
                if self.ultimo_frame_menu.is_some_and(|u| u.elapsed() < self.intervallo_menu()) {
                    self.ridisegna();
                } else {
                    self.frame_menu();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(mut b) = self.banco.take() {
            let continua = b.passo(self);
            self.banco = Some(b);
            if !continua {
                el.exit();
                return;
            }
        }
        if self.deve_disegnare_il_giro() {
            el.set_control_flow(ControlFlow::Poll);
            self.frame_giro();
            return;
        }
        match self.prossimo_ridisegno {
            Some(t) if !self.ridotta => {
                let dovuto = self.ultimo_frame_menu.map_or(t, |u| t.max(u + self.intervallo_menu()));
                if dovuto <= Instant::now() {
                    self.prossimo_ridisegno = None;
                    if let Some(f) = &self.finestra {
                        f.request_redraw();
                    }
                    el.set_control_flow(ControlFlow::Wait);
                } else {
                    el.set_control_flow(ControlFlow::WaitUntil(dovuto));
                }
            }
            _ => el.set_control_flow(if self.banco.is_some() {
                ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(50))
            } else {
                ControlFlow::Wait
            }),
        }
    }
}
