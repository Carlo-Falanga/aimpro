//! aimpro: aim trainer nativo per Windows, a bassa latenza.
//!
//! `aimpro`            il gioco
//! `aimpro --bench`    il banco di prova: misura da solo ed esce (`--secondi N` per fase di giro, `--uscita file`)
//! `--finestra`        in una finestra normale invece che a schermo intero (per le prove)

// In release niente finestra della console.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod allocazioni;
mod app;
mod banco;
mod camera;
mod config;
mod giro;
mod grafica;
mod ingresso;
mod interfaccia;
mod latenza;
mod misure;
mod sensibilita;
mod sistema;
mod tempo;

use std::path::PathBuf;

use winit::event_loop::{DeviceEvents, EventLoop};

#[global_allocator]
static ALLOCATORE: allocazioni::Contatore = allocazioni::Contatore;

/// Avvisi ed errori di wgpu e winit sulla console (in debug) e nel file degli errori.
struct Registro;

impl log::Log for Registro {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{}] {}: {}", r.level(), r.target(), r.args());
        }
    }

    fn flush(&self) {}
}

static REGISTRO: Registro = Registro;

fn argomento(nome: &str) -> Option<String> {
    let mut a = std::env::args().skip_while(|x| x != nome);
    a.next()?;
    a.next()
}

fn main() {
    let _ = log::set_logger(&REGISTRO).map(|()| log::set_max_level(log::LevelFilter::Warn));
    // Un errore fatale finisce in un file accanto alle impostazioni: senza console non lo vedrebbe nessuno.
    let cartella = config::percorso_predefinito().parent().map(PathBuf::from).unwrap_or_default();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::create_dir_all(&cartella);
        let testo = format!("{info}\n\n{}\n", std::backtrace::Backtrace::force_capture());
        let _ = std::fs::write(cartella.join("errore.txt"), &testo);
        eprintln!("{testo}");
    }));

    let banco = std::env::args().any(|a| a == "--bench").then(|| {
        // Al più 50 s per fase: il giro dura 60 s e finirebbe dentro la fase.
        let secondi = argomento("--secondi")
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| s.is_finite())
            .unwrap_or(15.0)
            .clamp(1.0, 50.0);
        let uscita = argomento("--uscita").map(PathBuf::from).unwrap_or_else(|| {
            let istante =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            PathBuf::from("misure").join(format!("bench-{istante}.md"))
        });
        banco::Banco::new(uscita, secondi)
    });

    let event_loop = EventLoop::new().expect("ciclo degli eventi");
    // Il mouse lo legge il thread del Raw Input: winit non deve registrarsi per gli stessi eventi (la
    // registrazione è una sola per processo).
    event_loop.listen_device_events(DeviceEvents::Never);
    let mut app = app::App::new(banco);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("aimpro: {e}");
    }
}
