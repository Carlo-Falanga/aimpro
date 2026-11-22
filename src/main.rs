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
mod esporta;
mod giro;
mod grafica;
mod ingresso;
mod interfaccia;
mod latenza;
mod misure;
mod registro;
mod sensibilita;
mod sistema;
mod storico;
mod tempo;

use std::path::PathBuf;

use winit::event_loop::{DeviceEvents, EventLoop};

#[global_allocator]
static ALLOCATORE: allocazioni::Contatore = allocazioni::Contatore;

/// Avvisi ed errori di wgpu e winit sulla console (in debug). Gli errori che impediscono di partire vanno anche
/// in `errore.txt` e in una finestra di messaggio (`sistema::errore_fatale`).
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
    let col_banco = std::env::args().any(|a| a == "--bench");
    // Un panic finisce in errore.txt e in una finestra di messaggio: senza console non lo vedrebbe nessuno.
    std::panic::set_hook(Box::new(move |info| {
        let testo = format!("{info}\n\n{}", std::backtrace::Backtrace::force_capture());
        sistema::errore_fatale(&testo, col_banco);
    }));

    let banco = col_banco.then(|| {
        // Al più 25 s per fase: il giro dura 30 s e finirebbe dentro la fase.
        let secondi = argomento("--secondi")
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| s.is_finite())
            .unwrap_or(15.0)
            .clamp(1.0, 25.0);
        let uscita = argomento("--uscita").map(PathBuf::from).unwrap_or_else(|| {
            let istante =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            PathBuf::from("misure").join(format!("bench-{istante}.md"))
        });
        banco::Banco::new(uscita, secondi)
    });

    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            sistema::errore_fatale(&format!("il ciclo degli eventi non parte: {e}"), col_banco);
            std::process::exit(1);
        }
    };
    // Il mouse lo legge il thread del Raw Input: winit non deve registrarsi per gli stessi eventi (la
    // registrazione è una sola per processo).
    event_loop.listen_device_events(DeviceEvents::Never);
    let mut app = app::App::new(banco);
    if let Err(e) = event_loop.run_app(&mut app) {
        sistema::errore_fatale(&format!("il ciclo degli eventi si è fermato: {e}"), col_banco);
    }
    if sistema::ERRORE_FATALE.load(std::sync::atomic::Ordering::SeqCst) {
        std::process::exit(1);
    }
}
