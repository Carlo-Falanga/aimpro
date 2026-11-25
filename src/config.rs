//! Le impostazioni, in un file TOML nella cartella `aimpro` dentro `%APPDATA%`.
//!
//! Un file mancante o rovinato non blocca niente: si parte dai predefiniti e il file si riscrive al primo
//! salvataggio. Ogni valore fuori dai limiti torna al predefinito.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::scenari::Parametri;
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
    /// All'avvio chiede a GitHub se c'è una build più nuova (acceso di base; un file vecchio senza il campo lo
    /// prende acceso).
    pub controlla_aggiornamenti: bool,
    /// L'id dello scenario scelto nel menu (un file vecchio senza il campo prende lo scatto).
    pub scenario: String,
    /// Dimensione e velocità dei bersagli, 1 = come dice lo scenario (fra 0,5 e 2).
    pub dimensione: f64,
    pub velocita: f64,
    /// Gli id degli scenari preferiti, nell'ordine in cui sono stati aggiunti.
    pub preferiti: Vec<String>,
    /// Punteggio e precisione dal vivo nel giro, in alto a sinistra.
    pub punteggio_dal_vivo: bool,
    /// La pagina del primo avvio (gioco, sensibilità, DPI) è già stata vista. Un file di prima senza il campo l'ha
    /// già vista: chi aveva le impostazioni le ha già scelte.
    pub primo_avvio_fatto: bool,
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
            controlla_aggiornamenti: true,
            scenario: crate::storico::SCATTO.to_string(),
            dimensione: 1.0,
            velocita: 1.0,
            preferiti: Vec::new(),
            punteggio_dal_vivo: true,
            primo_avvio_fatto: false,
        }
    }

    pub fn preferito(&self, id: &str) -> bool {
        self.preferiti.iter().any(|p| p == id)
    }

    /// Aggiunge o toglie lo scenario dai preferiti.
    pub fn cambia_preferito(&mut self, id: &str) {
        if self.preferito(id) {
            self.preferiti.retain(|p| p != id);
        } else {
            self.preferiti.push(id.to_string());
        }
    }

    pub fn parametri(&self) -> Parametri {
        Parametri { dimensione: self.dimensione, velocita: self.velocita }
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
        let p = self.parametri().normalizzati();
        (self.dimensione, self.velocita) = (p.dimensione, p.velocita);
        if self.scenario.trim().is_empty() {
            self.scenario = crate::storico::SCATTO.to_string();
        }
        let mut visti = Vec::new();
        self.preferiti.retain(|p| {
            !p.is_empty() && !visti.contains(p) && {
                visti.push(p.clone());
                true
            }
        });
        self.preferiti.truncate(200);
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

/// I campi che il file non ha prendono i predefiniti di serde, cioè quelli di CS2: sensibilità e FOV vanno invece
/// presi dal gioco scritto nel file (un file con solo `gioco = "valorant"` non deve dare la sensibilità di CS2).
fn con_predefiniti_del_gioco(mut c: Config, testo: &str) -> Config {
    let Ok(tabella) = testo.parse::<toml::Table>() else {
        return c;
    };
    let p = Config::predefinita(c.gioco);
    if !tabella.contains_key("sensibilita") {
        c.sensibilita = p.sensibilita;
    }
    if !tabella.contains_key("fov") {
        c.fov = p.fov;
    }
    if !tabella.contains_key("primo_avvio_fatto") {
        c.primo_avvio_fatto = true;
    }
    c
}

/// Legge le impostazioni. Se il file non c'è o non si legge, i predefiniti e il motivo.
pub fn carica(percorso: &Path) -> (Config, Option<String>) {
    match std::fs::read_to_string(percorso) {
        Ok(testo) => match toml::from_str::<Config>(&testo) {
            Ok(c) => (con_predefiniti_del_gioco(c, &testo).normalizzata(), None),
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
            controlla_aggiornamenti: false,
            scenario: "terna".to_string(),
            dimensione: 1.25,
            velocita: 0.75,
            preferiti: vec!["filo".to_string(), "terna".to_string()],
            punteggio_dal_vivo: false,
            primo_avvio_fatto: true,
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
        assert!(!c.primo_avvio_fatto, "senza file è il primo avvio");
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
            controlla_aggiornamenti: true,
            scenario: String::new(),
            dimensione: 7.0,
            velocita: -1.0,
            preferiti: vec!["a".into(), "".into(), "a".into(), "b".into()],
            punteggio_dal_vivo: true,
            primo_avvio_fatto: false,
        }
        .normalizzata();
        assert_eq!((c.scenario.as_str(), c.dimensione, c.velocita), ("scatto", 2.0, 0.5));
        assert_eq!(c.preferiti, ["a", "b"], "niente doppi né vuoti");
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
    fn i_campi_mancanti_prendono_il_predefinito_del_gioco_del_file() {
        let d = cartella_di_prova("gioco-mancanti");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("config.toml");
        std::fs::write(&p, "gioco = \"valorant\"\n").unwrap();
        let (c, avviso) = carica(&p);
        assert_eq!(avviso, None);
        assert_eq!(c.gioco, Gioco::Valorant);
        assert_eq!(c.sensibilita, Gioco::Valorant.sensibilita_predefinita());
        assert_eq!(c.fov, Gioco::Valorant.fov_predefinito());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_file_di_prima_senza_il_campo_controlla_gli_aggiornamenti() {
        let d = cartella_di_prova("senza-aggiornamenti");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("config.toml");
        std::fs::write(
            &p,
            "gioco = \"cs2\"
sensibilita = 1.25
vsync = false
schermo_intero_esclusivo = true
",
        )
        .unwrap();
        let (c, avviso) = carica(&p);
        assert_eq!(avviso, None);
        assert!(c.controlla_aggiornamenti);
        assert!(Config::default().controlla_aggiornamenti, "acceso di base");
        assert_eq!((c.scenario.as_str(), c.dimensione, c.velocita), ("scatto", 1.0, 1.0), "scenari standard");
        assert!(c.primo_avvio_fatto, "chi aveva già le impostazioni non rivede il primo avvio");
        assert!(c.punteggio_dal_vivo && c.preferiti.is_empty());
        std::fs::write(
            &p,
            "controlla_aggiornamenti = false
",
        )
        .unwrap();
        assert!(!carica(&p).0.controlla_aggiornamenti);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn il_percorso_sta_nella_cartella_aimpro() {
        let p = percorso_predefinito();
        assert!(p.ends_with("aimpro/config.toml") || p.ends_with("aimpro\\config.toml"));
    }
}
