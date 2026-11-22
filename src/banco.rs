//! Il banco di prova (`aimpro --bench`): gira da solo, misura ed esce.
//!
//! Fasi: menu fermo, menu col mouse che si muove, giro con vsync spento, giro con vsync acceso, giro con il limite
//! di fps alla frequenza dello schermo e al doppio, giro col pannello F3, riepilogo, pausa, finestra ridotta a
//! icona. Gira senza clic di chi guarda: chiede da sé il primo piano.
//!
//! Nelle fasi di giro un thread simula il mouse con `SendInput` (movimento ogni millisecondo, un clic ogni
//! 250 ms), **solo se la finestra di aimpro è in primo piano**: un clic simulato non deve mai finire in un altro
//! programma. L'input simulato passa dal Raw Input come quello vero; il cursore di Windows resta fermo, bloccato
//! al centro della finestra. Nel menu il thread manda invece messaggi di movimento alla finestra, senza clic e
//! senza muovere il cursore. Prima di cambiare fase il thread si ferma e lo conferma.
//!
//! A fine fase controlla di aver misurato quello che dice il nome (`verifica`): se no, la riga è NON VALIDA.
//!
//! Per ogni fase scrive: frame, fps, frame time e jitter, count→present, clic→present, CPU del processo,
//! allocazioni per frame, modalità di presentazione. Scrive anche gli istanti QPC di inizio e fine di ogni fase, per
//! tagliare il CSV di PresentMon (`--qpc_time`) sulle stesse fasi.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

use crate::allocazioni;
use crate::app::{App, Stato};
use crate::sistema;
use crate::tempo;

/// Il lato, in pixel, del quadrato al centro della finestra in cui si muove il cursore nella fase del menu col mouse.
const LATO_MENU: i32 = 200;

#[derive(Clone, Copy, Debug)]
enum Tipo {
    Menu,
    /// Il menu con il mouse che si muove sopra (senza clic): deve restare a 60 frame al secondo al massimo.
    MenuMouse,
    Giro {
        vsync: bool,
        limite_per_hz: f64,
        pannello: bool,
    },
    Riepilogo,
    Pausa,
    Icona,
}

#[derive(Clone, Copy, Debug)]
struct Fase {
    nome: &'static str,
    tipo: Tipo,
    secondi: f64,
}

struct Partenza {
    qpc: i64,
    cpu: sistema::Cpu,
    cpu_banco: sistema::Cpu,
    allocazioni: u64,
    alloc_wgpu: u64,
    alloc_nostre: u64,
    frame: u64,
    eventi: u64,
    simulati: u64,
    iniettati: u64,
}

pub struct Banco {
    fasi: Vec<Fase>,
    indice: usize,
    partenza: Option<Partenza>,
    uscita: PathBuf,
    testo: String,
    csv_fasi: String,
    inietta: Arc<AtomicBool>,
    /// Con `inietta`: anche i clic (solo nel giro, nel menu premerebbero i pulsanti).
    clic: Arc<AtomicBool>,
    /// Movimenti mandati dal thread del banco, con SendInput nel giro o come messaggi nel menu.
    iniettati: Arc<AtomicU64>,
    /// Il thread del mouse è fermo: l'ha visto `inietta` falso e non manderà niente finché non torna vero.
    fermo: Arc<AtomicBool>,
    /// Dov'era il cursore di Windows prima del banco: ci torna alla fine.
    cursore_prima: Option<(i32, i32)>,
    fine: Arc<AtomicBool>,
    hwnd: Arc<AtomicI64>,
    /// CPU usata dal thread che simula il mouse (bit di un f64), da togliere a quella del processo.
    cpu_banco: Arc<[AtomicU64; 2]>,
    avvio: i64,
    attesa_primo_piano: u32,
    /// Ripartenze della fase in corso.
    ripartenze: u32,
    /// La fase in corso ha perso il primo piano più di 3 volte: la sua riga non vale.
    non_valida: bool,
    note: String,
}

/// Libera il cursore di Windows e lo rimette dov'era prima del banco.
fn rimetti_cursore(dove: Option<(i32, i32)>) {
    sistema::libera_cursore();
    if let Some((x, y)) = dove {
        sistema::sposta_cursore(x, y);
    }
}

/// Comunque finisca il banco (fine normale o finestra chiusa), il cursore torna libero e al suo posto.
impl Drop for Banco {
    fn drop(&mut self) {
        self.ferma_mouse();
        self.fine.store(true, Ordering::Release);
        rimetti_cursore(self.cursore_prima);
    }
}

impl Banco {
    pub fn new(uscita: PathBuf, secondi_giro: f64) -> Self {
        sistema::cicli_per_ms(); // la calibrazione dei cicli prima delle fasi, non durante
        let g = secondi_giro;
        let fasi = vec![
            Fase {
                nome: "riscaldamento",
                tipo: Tipo::Giro { vsync: false, limite_per_hz: 0.0, pannello: false },
                secondi: 3.0,
            },
            Fase { nome: "menu fermo", tipo: Tipo::Menu, secondi: 8.0 },
            Fase { nome: "menu, mouse in movimento", tipo: Tipo::MenuMouse, secondi: 5.0 },
            Fase {
                nome: "giro, vsync spento",
                tipo: Tipo::Giro { vsync: false, limite_per_hz: 0.0, pannello: false },
                secondi: g,
            },
            Fase {
                nome: "giro, vsync acceso",
                tipo: Tipo::Giro { vsync: true, limite_per_hz: 0.0, pannello: false },
                secondi: g,
            },
            Fase {
                nome: "giro, vsync spento, limite = Hz schermo",
                tipo: Tipo::Giro { vsync: false, limite_per_hz: 1.0, pannello: false },
                secondi: g,
            },
            Fase {
                nome: "giro, vsync spento, limite = 2 x Hz schermo",
                tipo: Tipo::Giro { vsync: false, limite_per_hz: 2.0, pannello: false },
                secondi: g,
            },
            Fase {
                nome: "giro, vsync spento, pannello F3 aperto",
                tipo: Tipo::Giro { vsync: false, limite_per_hz: 0.0, pannello: true },
                secondi: g,
            },
            Fase { nome: "riepilogo fermo", tipo: Tipo::Riepilogo, secondi: 5.0 },
            Fase { nome: "pausa", tipo: Tipo::Pausa, secondi: 8.0 },
            Fase { nome: "ridotta a icona", tipo: Tipo::Icona, secondi: 8.0 },
        ];
        Banco {
            fasi,
            indice: 0,
            partenza: None,
            uscita,
            testo: String::new(),
            csv_fasi: "fase,qpc_inizio,qpc_fine,frequenza_qpc,valida\n".to_string(),
            inietta: Arc::new(AtomicBool::new(false)),
            clic: Arc::new(AtomicBool::new(false)),
            iniettati: Arc::new(AtomicU64::new(0)),
            fermo: Arc::new(AtomicBool::new(true)),
            cursore_prima: sistema::posizione_cursore(),
            fine: Arc::new(AtomicBool::new(false)),
            hwnd: Arc::new(AtomicI64::new(0)),
            cpu_banco: Arc::new([AtomicU64::new(0), AtomicU64::new(0)]),
            avvio: tempo::qpc(),
            attesa_primo_piano: 0,
            ripartenze: 0,
            non_valida: false,
            note: String::new(),
        }
    }

    /// Avvia il thread che simula il mouse.
    pub fn avvia(&mut self, hwnd: isize) {
        self.hwnd.store(hwnd as i64, Ordering::Release);
        // Sicurezza: se qualcosa si blocca, il banco esce comunque e lascia libero lo schermo.
        // Ogni fase può ripartire fino a 3 volte (con 5 s di attesa del primo piano): il limite ne tiene conto.
        let limite = self.fasi.iter().map(|f| 4.0 * (f.secondi + 5.0)).sum::<f64>() + 30.0;
        let cursore_prima = self.cursore_prima;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs_f64(limite));
            eprintln!("aimpro --bench: tempo massimo superato, esco");
            rimetti_cursore(cursore_prima);
            std::process::exit(3);
        });
        let (inietta, fine, h) = (self.inietta.clone(), self.fine.clone(), self.hwnd.clone());
        let clic = self.clic.clone();
        let iniettati = self.iniettati.clone();
        let fermo = self.fermo.clone();
        let cpu = self.cpu_banco.clone();
        std::thread::Builder::new()
            .name("aimpro-banco-mouse".into())
            .spawn(move || {
                let attesa = tempo::Attesa::new();
                let mut prossimo = tempo::qpc();
                let mut passo: i64 = 0;
                let mut centro_menu = None;
                while !fine.load(Ordering::Acquire) {
                    let c = sistema::cpu_thread();
                    cpu[0].store(c.tempo_ms.to_bits(), Ordering::Release);
                    cpu[1].store(c.cicli_ms.to_bits(), Ordering::Release);
                    if !inietta.load(Ordering::SeqCst) {
                        fermo.store(true, Ordering::SeqCst);
                        // Fuori dalle fasi col mouse il thread dorme: niente CPU sprecata.
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        prossimo = tempo::qpc();
                        continue;
                    }
                    fermo.store(false, Ordering::SeqCst);
                    prossimo += tempo::tick(1.0);
                    attesa.fino_a(prossimo);
                    // Durante l'attesa la fase può essere finita: si ricontrolla prima di mandare.
                    if !inietta.load(Ordering::SeqCst) {
                        continue;
                    }
                    let hwnd = h.load(Ordering::Acquire) as isize;
                    if centro_menu.is_none() {
                        centro_menu = sistema::centro_finestra(hwnd);
                    }
                    // Avanti e indietro: 150 ms a destra, 150 ms a sinistra; un po' su e giù.
                    let dx = if (passo / 150) % 2 == 0 { 4 } else { -4 };
                    let dy = if (passo / 400) % 2 == 0 { 1 } else { -1 };
                    let mandato = if clic.load(Ordering::Acquire) {
                        // Nel giro: SendInput, che passa dal Raw Input come un mouse vero. Il cursore di Windows
                        // resta fermo, perché nel giro è bloccato al centro della finestra.
                        sistema::simula_movimento(hwnd, dx, dy)
                    } else {
                        // Nel menu: un messaggio di movimento alla finestra, in un quadrato al centro. Il menu lo
                        // vede come un movimento vero, il cursore di Windows non si muove.
                        let (cx, cy) = centro_menu.unwrap_or((0, 0));
                        let x = cx + (passo % LATO_MENU as i64) as i32 - LATO_MENU / 2;
                        let y = cy + ((passo / 7) % LATO_MENU as i64) as i32 - LATO_MENU / 2;
                        sistema::manda_movimento_alla_finestra(hwnd, x, y)
                    };
                    if mandato {
                        iniettati.fetch_add(1, Ordering::Relaxed);
                    }
                    if passo % 250 == 125 && clic.load(Ordering::Acquire) {
                        sistema::simula_clic(hwnd);
                    }
                    passo += 1;
                }
            })
            .expect("thread del banco");
    }

    fn cpu_banco(&self) -> sistema::Cpu {
        sistema::Cpu {
            tempo_ms: f64::from_bits(self.cpu_banco[0].load(Ordering::Acquire)),
            cicli_ms: f64::from_bits(self.cpu_banco[1].load(Ordering::Acquire)),
        }
    }

    /// Ferma il thread del mouse e aspetta che lo confermi (al massimo 100 ms): dopo, non parte più niente, né un
    /// movimento col cursore libero né un clic sul menu.
    fn ferma_mouse(&self) {
        self.inietta.store(false, Ordering::SeqCst);
        self.clic.store(false, Ordering::SeqCst);
        let inizio = std::time::Instant::now();
        while !self.fermo.load(Ordering::SeqCst) && inizio.elapsed() < std::time::Duration::from_millis(100) {
            std::thread::yield_now();
        }
    }

    fn entra(&mut self, app: &mut App, f: Fase) {
        self.ferma_mouse();
        if !matches!(f.tipo, Tipo::Giro { .. }) {
            app.mostra_pannello(false);
        }
        match f.tipo {
            Tipo::Menu | Tipo::MenuMouse => {
                if let Some(w) = &app.finestra {
                    w.set_minimized(false);
                }
                app.torna_al_menu();
                if matches!(f.tipo, Tipo::MenuMouse) {
                    // Movimenti mandati alla finestra come messaggi (sono quelli a chiedere i ridisegni del
                    // menu), senza clic e senza muovere il cursore di Windows.
                    self.clic.store(false, Ordering::Release);
                    self.inietta.store(true, Ordering::Release);
                }
            }
            Tipo::Giro { vsync, limite_per_hz, pannello } => {
                app.mostra_pannello(pannello);
                let limite = (app.hz_schermo * limite_per_hz).round() as u32;
                app.imposta_vsync_e_limite(vsync, limite);
                app.inizia_giro();
                self.clic.store(true, Ordering::Release);
                self.inietta.store(true, Ordering::Release);
            }
            Tipo::Riepilogo => {
                // Il giro della fase prima finisce qui: si vede il riepilogo con i suoi numeri.
                app.mostra_pannello(false);
                app.termina_giro();
            }
            Tipo::Pausa => {
                app.imposta_vsync_e_limite(false, 0);
                app.inizia_giro();
                app.pausa();
            }
            Tipo::Icona => {
                app.inizia_giro();
                if let Some(w) = &app.finestra {
                    w.set_minimized(true);
                }
                app.pausa();
            }
        }
        self.partenza = Some(Partenza {
            qpc: tempo::qpc(),
            cpu: sistema::cpu_processo(),
            cpu_banco: self.cpu_banco(),
            allocazioni: allocazioni::totale(),
            alloc_wgpu: app.alloc_wgpu,
            alloc_nostre: app.alloc_nostre,
            frame: app.frame_disegnati,
            eventi: app.ingresso.eventi.load(Ordering::Relaxed),
            simulati: app.ingresso.simulati.load(Ordering::Relaxed),
            iniettati: self.iniettati.load(Ordering::Relaxed),
        });
    }

    fn chiudi_fase(&mut self, app: &mut App, f: Fase, p: &Partenza) {
        let ora = tempo::qpc();
        let durata_ms = tempo::ms(ora - p.qpc);
        let cpu = (sistema::cpu_processo() - p.cpu) - (self.cpu_banco() - p.cpu_banco);
        let frame = app.frame_disegnati - p.frame;
        let alloc = allocazioni::totale() - p.allocazioni;
        let eventi = app.ingresso.eventi.load(Ordering::Relaxed) - p.eventi;
        let m = app.latenza.misure();
        let presentazione =
            app.grafica.as_ref().map_or(String::new(), |g| format!("{:?}", g.presentazione()));
        let fps = frame as f64 / (durata_ms / 1000.0);
        let r = |x: &crate::misure::Riassunto| {
            if x.n == 0 { "-".to_string() } else { format!("{:.2} / {:.2}", x.mediana, x.p99) }
        };
        // Per frame time e count→present anche 99,9° percentile e massimo: a migliaia di fps i picchi rari
        // restano sotto il 99°.
        let r4 = |x: &crate::misure::Riassunto| {
            if x.n == 0 {
                "-".to_string()
            } else {
                format!("{:.2} / {:.2} / {:.2} / {:.2}", x.mediana, x.p99, x.p999, x.massimo)
            }
        };
        if let Some(motivo) = self.verifica(app, f, p, frame, m.count_present_vecchio.n, durata_ms) {
            self.non_valida = true;
            let _ = writeln!(self.note, "- {}: NON VALIDA, {}", f.nome, motivo);
        }
        let nome =
            if self.non_valida { format!("{} (NON VALIDA: vedi le note)", f.nome) } else { f.nome.into() };
        let nel_giro = matches!(f.tipo, Tipo::Giro { .. });
        let mem = sistema::memoria();
        let _ = writeln!(
            self.testo,
            "| {} | {} | {:.0} | {} | {} | {} | {} | {} | {:.1}% / {:.1}% | {:.1} / {:.1} | {} | {} |",
            nome,
            if nel_giro { presentazione.as_str() } else { "-" },
            fps,
            if nel_giro { r4(&m.frame) } else { "-".into() },
            if nel_giro && m.frame.n > 0 { format!("{:.3}", m.frame.deviazione) } else { "-".into() },
            if nel_giro { r4(&m.count_present_vecchio) } else { "-".into() },
            if nel_giro { r(&m.count_present_recente) } else { "-".into() },
            if nel_giro { r(&m.clic_present) } else { "-".into() },
            cpu.tempo_ms / durata_ms * 100.0,
            cpu.cicli_ms / durata_ms * 100.0,
            sistema::Memoria::mb(mem.privata),
            sistema::Memoria::mb(mem.impegnata),
            if frame == 0 {
                format!("{alloc} in tutto")
            } else if nel_giro {
                let per = |x: u64| x as f64 / frame as f64;
                format!(
                    "{:.1} / {:.1} / {:.2}",
                    per(alloc),
                    per(app.alloc_wgpu - p.alloc_wgpu),
                    per(app.alloc_nostre - p.alloc_nostre)
                )
            } else {
                format!("{:.1}", alloc as f64 / frame as f64)
            },
            eventi,
        );
        // Il nome fra virgolette: contiene virgole.
        let _ = writeln!(
            self.csv_fasi,
            "\"{}\",{},{},{},{}",
            f.nome,
            p.qpc,
            ora,
            tempo::tick(1000.0),
            if self.non_valida { "no" } else { "si" }
        );
    }

    /// Controlla, a fine fase, che la fase abbia misurato davvero quello che dice il suo nome. Restituisce il
    /// motivo se non è così.
    fn verifica(
        &self,
        app: &App,
        f: Fase,
        p: &Partenza,
        frame: u64,
        campioni: usize,
        durata_ms: f64,
    ) -> Option<String> {
        let hwnd = self.hwnd.load(Ordering::Acquire) as isize;
        let iniettati = self.iniettati.load(Ordering::Relaxed) - p.iniettati;
        let simulati = app.ingresso.simulati.load(Ordering::Relaxed) - p.simulati;
        let attesi = (durata_ms * 0.9) as u64; // un movimento al millisecondo, con margine
        let mouse = || {
            if iniettati < attesi {
                Some(format!("mandati solo {iniettati} movimenti del mouse su {attesi} attesi"))
            } else if simulati * 10 < iniettati * 9 {
                Some(format!("il Raw Input ha visto {simulati} movimenti su {iniettati} mandati"))
            } else {
                None
            }
        };
        match f.tipo {
            Tipo::Giro { vsync, limite_per_hz, pannello } => {
                if app.stato != Stato::Giro {
                    Some(format!("a fine fase lo stato era {:?}, non il giro", app.stato))
                } else if !sistema::in_primo_piano(hwnd) {
                    Some("a fine fase la finestra non era in primo piano".into())
                } else if !sistema::cursore_bloccato(hwnd, 1) {
                    Some("il cursore di Windows non era bloccato al centro".into())
                } else if app.vsync() != vsync {
                    Some("la presentazione non era quella della fase".into())
                } else if app.config.limite_fps != (app.hz_schermo * limite_per_hz).round() as u32 {
                    Some("il limite di fps non era quello della fase".into())
                } else if app.pannello_visibile != pannello {
                    Some("il pannello F3 non era come la fase lo chiede".into())
                } else if frame == 0 || campioni == 0 {
                    Some("nessun frame del giro misurato".into())
                } else {
                    mouse()
                }
            }
            Tipo::Menu | Tipo::MenuMouse if app.stato != Stato::Menu => {
                Some(format!("a fine fase lo stato era {:?}, non il menu", app.stato))
            }
            Tipo::MenuMouse if !sistema::in_primo_piano(hwnd) => Some("finestra non in primo piano".into()),
            Tipo::MenuMouse if iniettati < attesi => {
                Some(format!("mandati solo {iniettati} movimenti alla finestra su {attesi} attesi"))
            }
            Tipo::MenuMouse if frame == 0 => Some("il menu non ha ridisegnato".into()),
            Tipo::MenuMouse if frame as f64 / (durata_ms / 1000.0) > app.fps_menu() * 1.05 => {
                Some(format!("il menu ha ridisegnato più di {:.0} volte al secondo", app.fps_menu()))
            }
            Tipo::MenuMouse => None,
            Tipo::Menu => None,
            Tipo::Riepilogo if !matches!(app.stato, Stato::Riepilogo(_)) => {
                Some(format!("a fine fase lo stato era {:?}, non il riepilogo", app.stato))
            }
            Tipo::Pausa if app.stato != Stato::Pausa => {
                Some(format!("a fine fase lo stato era {:?}, non la pausa", app.stato))
            }
            Tipo::Icona if !app.ridotta => Some("la finestra non era ridotta a icona".into()),
            Tipo::Riepilogo | Tipo::Pausa | Tipo::Icona => None,
        }
    }

    /// Un passo del banco: dice `false` quando ha finito.
    pub fn passo(&mut self, app: &mut App) -> bool {
        if self.indice >= self.fasi.len() {
            return false; // finito: winit può chiamare ancora una volta prima di uscire
        }
        if app.grafica.is_none() {
            return true;
        }
        // Le fasi di giro ripartono se la finestra perde il primo piano (il giro va in pausa da solo).
        if self.partenza.is_none() {
            let hwnd = self.hwnd.load(Ordering::Acquire) as isize;
            // Il primo piano deve averlo visto anche winit (Focused), che decide se il giro si disegna.
            if !(sistema::in_primo_piano(hwnd) && app.in_primo_piano) {
                if self.attesa_primo_piano < 100 {
                    // Chiede il primo piano da sé (niente clic di chi guarda) e aspetta fino a 5 s.
                    if sistema::porta_in_primo_piano(hwnd) {
                        let nome = self.fasi[self.indice].nome;
                        let _ = writeln!(
                            self.note,
                            "- {nome}: la finestra non era in primo piano, l'ha preso il banco"
                        );
                        let _ = writeln!(self.note, "  (dopo {} ms)", self.attesa_primo_piano * 50);
                    }
                    self.attesa_primo_piano += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    return true;
                }
                // 5 s senza primo piano: la fase parte lo stesso, ma se è di giro la sua riga non vale.
                self.non_valida = matches!(self.fasi[self.indice].tipo, Tipo::Giro { .. });
            }
            let f = self.fasi[self.indice];
            self.entra(app, f);
            return true;
        }
        let f = self.fasi[self.indice];
        let p = self.partenza.take().expect("partenza");
        if tempo::ms(tempo::qpc() - p.qpc) < f.secondi * 1000.0 {
            // In una fase di giro, se il giro è andato in pausa (primo piano perso) le misure sono falsate: la fase
            // riparte da capo, al massimo 3 volte; dopo, la riga si segna come non valida.
            if matches!(f.tipo, Tipo::Giro { .. }) && (app.stato == Stato::Pausa || !app.in_primo_piano) {
                if self.ripartenze < 3 {
                    self.ripartenze += 1;
                    let _ = writeln!(self.note, "- {}: primo piano perso, fase ripartita da capo", f.nome);
                    self.partenza = None;
                    self.attesa_primo_piano = 0;
                    self.non_valida = false;
                    return true;
                }
                self.non_valida = true;
            }
            self.partenza = Some(p);
            return true;
        }
        self.chiudi_fase(app, f, &p);
        self.indice += 1;
        self.ripartenze = 0;
        self.non_valida = false;
        if self.indice < self.fasi.len() {
            let f = self.fasi[self.indice];
            self.entra(app, f);
            return true;
        }
        self.ferma_mouse();
        self.fine.store(true, Ordering::Release);
        app.torna_al_menu();
        self.scrivi(app);
        false
    }

    fn scrivi(&self, app: &App) {
        let scheda = app.grafica.as_ref().map_or(String::new(), |g| {
            let s = &g.scheda;
            let driver = [s.driver.as_str(), s.driver_info.as_str()].join(" ");
            let driver = driver.trim();
            if driver.is_empty() {
                format!("{} ({:?})", s.name, s.backend)
            } else {
                format!("{} ({:?}, driver {driver})", s.name, s.backend)
            }
        });
        let dimensioni = app.grafica.as_ref().map_or((0, 0), |g| (g.config.width, g.config.height));
        let mut out = String::new();
        let _ = writeln!(out, "# aimpro --bench\n");
        let _ = writeln!(out, "- scheda video: {scheda}");
        let _ = writeln!(
            out,
            "- schermo: {}x{} a {:.2} Hz, schermo intero {}",
            dimensioni.0,
            dimensioni.1,
            app.hz_schermo,
            if app.config.schermo_intero_esclusivo { "esclusivo" } else { "senza bordi" }
        );
        let _ =
            writeln!(out, "- core logici: {}", std::thread::available_parallelism().map_or(0, |n| n.get()));
        let _ = writeln!(
            out,
            "- frequenza dei cicli CPU (calibrata): {:.0} MHz",
            sistema::cicli_per_ms() / 1000.0
        );
        let _ = writeln!(
            out,
            "- eventi simulati ricevuti dal Raw Input: {} (assoluti ignorati: {})",
            app.ingresso.simulati.load(Ordering::Relaxed),
            app.ingresso.assoluti.load(Ordering::Relaxed)
        );
        let _ = writeln!(out, "- durata totale: {:.1} s\n", tempo::ms(tempo::qpc() - self.avvio) / 1000.0);
        let _ = writeln!(
            out,
            "Tempi in ms, mediana / 99° percentile (frame time e count→present vecchio anche 99,9° e massimo). \
             CPU del processo sul tempo della fase (100% = un core intero), senza il thread del banco che simula il \
             mouse, in due modi: a tempo (GetProcessTimes, a scatti di 15,6 ms) / a cicli (convertiti con la \
             frequenza calibrata, più bassa del vero se il core rallenta). Allocazioni per frame: di tutto il \
             processo, poi quelle dentro wgpu e quelle del codice di aimpro fra get_current_texture e present. \
             Memoria a fine fase, in MB: set di lavoro privato (la colonna \"Memoria\" di Gestione attività) / \
             memoria impegnata privata.\n"
        );
        let _ = writeln!(
            out,
            "| fase | presentazione | fps | frame time | jitter | count→present (vecchio) | count→present \
             (recente) | clic→present | CPU (tempo / cicli) | memoria MB (privata / impegnata) | allocazioni \
             per frame (processo / wgpu / aimpro) | eventi mouse |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|---|---|");
        out.push_str(&self.testo);
        let _ = writeln!(out, "\nMemoria alle tappe dell'avvio, in MB (privata / impegnata):\n");
        for (tappa, m) in sistema::tappe_memoria() {
            let _ = writeln!(
                out,
                "- {tappa}: {:.1} / {:.1}",
                sistema::Memoria::mb(m.privata),
                sistema::Memoria::mb(m.impegnata)
            );
        }
        if !self.note.is_empty() {
            let _ = writeln!(out, "\nNote:\n{}", self.note);
        }
        if let Some(cartella) = self.uscita.parent() {
            let _ = std::fs::create_dir_all(cartella);
        }
        let _ = std::fs::write(&self.uscita, &out);
        let _ = std::fs::write(self.uscita.with_extension("fasi.csv"), &self.csv_fasi);
        println!("{out}");
        println!("Scritto in {}", self.uscita.display());
    }
}
