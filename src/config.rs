//! Le impostazioni, in un file TOML nella cartella `aimpro` dentro `%APPDATA%`.
//!
//! Un file mancante o rovinato non blocca niente: si parte dai predefiniti e il file si riscrive al primo
//! salvataggio. Ogni valore fuori dai limiti torna al predefinito.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sensibilita::Gioco;

pub const LIMITI_SENSIBILITA: (f64, f64) = (0.001, 100.0);
pub const LIMITI_FOV: (f64, f64) = (50.0, 140.0);
pub const LIMITI_DPI: (f64, f64) = (100.0, 64000.0);
pub const LIMITI_FPS: (u32, u32) = (30, 2000);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub gioco: Gioco,
    pub sensibilita: f64,
    /// FOV orizzontale come nel gioco (CS2 su 4:3, Valorant su 16:9).
    pub fov: f64,
    /// Solo per mostrare i centimetri per giro: non cambia la rotazione.
    pub dpi: f64,
    /// Sincronizzazione verticale. Spenta di base: meno latenza, possibile tearing.
    pub vsync: bool,
    /// Limite di fotogrammi al secondo con vsync spento; 0 = nessun limite.
    pub limite_fps: u32,
    /// Schermo intero esclusivo (cambia modalità video); altrimenti finestra senza bordi a tutto schermo.
    pub schermo_intero_esclusivo: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config::predefinita(Gioco::Cs2)
    }
}

fn dentro(v: f64, (min, max): (f64, f64)) -> bool {
    v.is_finite() && v >= min && v <= max
}

impl Config {
    pub fn predefinita(gioco: Gioco) -> Self {
        Config {
            gioco,
            sensibilita: gioco.sensibilita_predefinita(),
            fov: gioco.fov_predefinito(),
            dpi: 800.0,
            vsync: false,
            limite_fps: 0,
            schermo_intero_esclusivo: true,
        }
    }

    /// Riporta ogni valore fuori dai limiti al predefinito del gioco.
    pub fn normalizzata(mut self) -> Self {
        let p = Config::predefinita(self.gioco);
        if !dentro(self.sensibilita, LIMITI_SENSIBILITA) {
            self.sensibilita = p.sensibilita;
        }
        if !dentro(self.fov, LIMITI_FOV) {
            self.fov = p.fov;
        }
        if !dentro(self.dpi, LIMITI_DPI) {
            self.dpi = p.dpi;
        }
        if self.limite_fps != 0 && !(LIMITI_FPS.0..=LIMITI_FPS.1).contains(&self.limite_fps) {
            self.limite_fps = 0;
        }
        self
    }
}

/// `%APPDATA%\aimpro\config.toml`; se `APPDATA` manca, accanto al programma.
pub fn percorso_predefinito() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("aimpro").join("config.toml")
}

/// Legge le impostazioni. Se il file non c'è o non si legge, i predefiniti e il motivo.
pub fn carica(percorso: &Path) -> (Config, Option<String>) {
    match std::fs::read_to_string(percorso) {
        Ok(testo) => match toml::from_str::<Config>(&testo) {
            Ok(c) => (c.normalizzata(), None),
            Err(e) => (Config::default(), Some(format!("impostazioni illeggibili, uso i predefiniti: {e}"))),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
        Err(e) => (Config::default(), Some(format!("impostazioni non lette: {e}"))),
    }
}

/// Scrive le impostazioni: prima in un file accanto, poi lo rinomina, così un'interruzione non lascia il file
/// a metà.
pub fn salva(percorso: &Path, c: &Config) -> std::io::Result<()> {
    if let Some(cartella) = percorso.parent() {
        std::fs::create_dir_all(cartella)?;
    }
    let testo = toml::to_string_pretty(&c.clone().normalizzata()).map_err(std::io::Error::other)?;
    let provvisorio = percorso.with_extension("toml.nuovo");
    std::fs::write(&provvisorio, testo)?;
    std::fs::rename(&provvisorio, percorso)
}

#[cfg(test)]
mod test {
    use super::*;

    fn cartella_di_prova(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aimpro-test-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn salva_e_rilegge_le_stesse_impostazioni() {
        let p = cartella_di_prova("salva").join("aimpro").join("config.toml");
        let c = Config {
            gioco: Gioco::Valorant,
            sensibilita: 0.32,
            fov: 103.0,
            dpi: 1600.0,
            vsync: true,
            limite_fps: 240,
            schermo_intero_esclusivo: false,
        };
        salva(&p, &c).unwrap();
        assert_eq!(carica(&p), (c, None));
    }

    #[test]
    fn senza_file_i_predefiniti_di_cs2() {
        let p = cartella_di_prova("manca").join("config.toml");
        let (c, errore) = carica(&p);
        assert_eq!(c, Config::predefinita(Gioco::Cs2));
        assert!(errore.is_none());
        assert_eq!(c.fov, 90.0);
        assert!(!c.vsync, "vsync spento di base");
    }

    #[test]
    fn file_rovinato_da_i_predefiniti_e_il_motivo() {
        let d = cartella_di_prova("rovinato");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("config.toml");
        std::fs::write(&p, "gioco = [non toml").unwrap();
        let (c, errore) = carica(&p);
        assert_eq!(c, Config::default());
        assert!(errore.is_some());
    }

    #[test]
    fn valori_assurdi_tornano_ai_predefiniti_del_gioco() {
        let c = Config {
            gioco: Gioco::Valorant,
            sensibilita: -3.0,
            fov: 500.0,
            dpi: 0.0,
            vsync: false,
            limite_fps: 5,
            schermo_intero_esclusivo: true,
        }
        .normalizzata();
        assert_eq!(c.sensibilita, 0.4);
        assert_eq!(c.fov, 103.0);
        assert_eq!(c.dpi, 800.0);
        assert_eq!(c.limite_fps, 0);
    }

    #[test]
    fn campi_mancanti_prendono_il_predefinito() {
        let c: Config = toml::from_str("gioco = \"valorant\"\nsensibilita = 0.5\n").unwrap();
        assert_eq!(c.gioco, Gioco::Valorant);
        assert_eq!(c.sensibilita, 0.5);
        assert_eq!(c.dpi, 800.0);
    }

    #[test]
    fn il_percorso_sta_nella_cartella_aimpro() {
        let p = percorso_predefinito();
        assert!(p.ends_with("aimpro/config.toml") || p.ends_with("aimpro\\config.toml"));
    }
}
