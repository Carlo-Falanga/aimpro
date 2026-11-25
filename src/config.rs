//! Le impostazioni, in un file TOML nella cartella `aimpro` dentro `%APPDATA%`.
//!
//! Un file mancante o rovinato non blocca niente: si parte dai predefiniti e il file si riscrive al primo
//! salvataggio. Ogni valore fuori dai limiti torna al predefinito.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::aspetto::{ColoriBersaglio, Tema};
use crate::mirino::Mirino;
use crate::sensibilita::{Gioco, Variazione};

pub const LIMITI_FOV: (f64, f64) = (50.0, 140.0);
pub const LIMITI_DPI: (f64, f64) = (100.0, 64000.0);
pub const LIMITI_FPS: (u32, u32) = (30, 2000);
pub const LIMITI_MOLTIPLICATORE_Y: (f64, f64) = (0.1, 10.0);
pub const LIMITI_VOLUME: (f64, f64) = (0.0, 100.0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Un gioco che non c'è più (Fortnite, tolto il 25/11/2026) o sconosciuto diventa CS2.
    #[serde(deserialize_with = "gioco_o_predefinito")]
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
    /// La sensibilità verticale rispetto a quella orizzontale (1 = uguale).
    pub moltiplicatore_y: f64,
    /// La sensibilità a caso, entro ±N%.
    pub variazione: Variazione,
    pub mirino: Mirino,
    pub bersagli: ColoriBersaglio,
    pub tema: Tema,
    /// I suoni di colpo, errore e comparsa.
    pub suoni: bool,
    /// Da 0 a 100.
    pub volume: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config::predefinita(Gioco::Cs2)
    }
}

/// Il gioco scritto nel file, se aimpro lo conosce.
fn gioco_noto(nome: &str) -> Option<Gioco> {
    use serde::de::IntoDeserializer;
    Gioco::deserialize(IntoDeserializer::<serde::de::value::Error>::into_deserializer(nome)).ok()
}

fn gioco_o_predefinito<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Gioco, D::Error> {
    let nome = String::deserialize(d)?;
    Ok(gioco_noto(&nome).unwrap_or(Gioco::Cs2))
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
            moltiplicatore_y: 1.0,
            variazione: Variazione::default(),
            mirino: Mirino::default(),
            bersagli: ColoriBersaglio::default(),
            tema: Tema::Grafite,
            suoni: true,
            volume: 50.0,
        }
    }

    /// Riporta ogni valore fuori dai limiti al predefinito del gioco.
    pub fn normalizzata(mut self) -> Self {
        let p = Config::predefinita(self.gioco);
        if !dentro(self.moltiplicatore_y, LIMITI_MOLTIPLICATORE_Y) {
            self.moltiplicatore_y = p.moltiplicatore_y;
        }
        if !dentro(self.volume, LIMITI_VOLUME) {
            self.volume = p.volume;
        }
        self.variazione = self.variazione.normalizzata();
        self.mirino = self.mirino.normalizzato();
        if !dentro(self.sensibilita, self.gioco.limiti_sensibilita()) {
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

/// I campi che il file non ha prendono i predefiniti di serde, cioè quelli di CS2: sensibilità e FOV vanno invece
/// presi dal gioco scritto nel file (un file con solo `gioco = "valorant"` non deve dare la sensibilità di CS2).
fn con_predefiniti_del_gioco(mut c: Config, testo: &str) -> Config {
    let Ok(tabella) = testo.parse::<toml::Table>() else {
        return c;
    };
    let p = Config::predefinita(c.gioco);
    // Con un gioco sconosciuto, sensibilità e FOV del file sono nelle sue unità: non valgono per CS2.
    let sconosciuto = tabella.get("gioco").and_then(|g| g.as_str()).is_some_and(|g| gioco_noto(g).is_none());
    if !tabella.contains_key("sensibilita") || sconosciuto {
        c.sensibilita = p.sensibilita;
    }
    if !tabella.contains_key("fov") || sconosciuto {
        c.fov = p.fov;
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
            moltiplicatore_y: 0.8,
            variazione: Variazione {
                attiva: true,
                percento: 12.5,
                ogni: crate::sensibilita::Ogni::Secondi,
                secondi: 7.0,
            },
            mirino: Mirino {
                forma: crate::mirino::Forma::Cerchio,
                lunghezza: 9.0,
                spessore: 3.0,
                spazio: 2.0,
                bordo: 0.0,
                colore: [10, 200, 30],
            },
            bersagli: ColoriBersaglio { normale: [1, 2, 3], guardato: [4, 5, 6], colpito: [7, 8, 9] },
            tema: Tema::Sabbia,
            suoni: false,
            volume: 35.0,
        };
        salva(&p, &c).unwrap();
        assert_eq!(carica(&p), (c, None));
    }

    /// Fortnite c'era in una build di prova ed è stato tolto: un file con quel gioco (o con un gioco che non
    /// esiste) torna a CS2 con sensibilità e FOV di CS2, senza avvisi, e tiene il resto.
    #[test]
    fn un_gioco_tolto_torna_al_predefinito() {
        let d = cartella_di_prova("gioco-tolto");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("config.toml");
        std::fs::write(
            &p,
            "gioco = \"fortnite\"\nsensibilita = 6.5\nfov = 80.0\ndpi = 1600.0\ntema = \"notte\"\n",
        )
        .unwrap();
        let (c, avviso) = carica(&p);
        assert_eq!(avviso, None);
        assert_eq!(c.gioco, Gioco::Cs2);
        assert_eq!((c.sensibilita, c.fov), (1.25, 90.0));
        assert_eq!(c.dpi, 1600.0);
        assert_eq!(c.tema, Tema::Notte);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Il config.toml come lo scriveva la build di prima (main del 25/11): si legge senza avvisi, tiene i suoi
    /// valori e prende i predefiniti per le cose nuove.
    #[test]
    fn un_config_di_prima_si_legge_ancora() {
        let d = cartella_di_prova("di-prima");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("config.toml");
        std::fs::write(
            &p,
            "gioco = \"valorant\"
sensibilita = 0.32
fov = 103.0
dpi = 1600.0
vsync = false
limite_fps = 240
schermo_intero_esclusivo = true
controlla_aggiornamenti = false
",
        )
        .unwrap();
        let (c, avviso) = carica(&p);
        assert_eq!(avviso, None);
        assert_eq!((c.gioco, c.sensibilita, c.dpi, c.limite_fps), (Gioco::Valorant, 0.32, 1600.0, 240));
        assert!(!c.controlla_aggiornamenti);
        let base = Config::default();
        assert_eq!(c.moltiplicatore_y, 1.0);
        assert_eq!(c.variazione, base.variazione);
        assert!(!c.variazione.attiva, "la sensibilità a caso parte spenta");
        assert_eq!(c.mirino, Mirino::default());
        assert_eq!(c.bersagli, ColoriBersaglio::default());
        assert_eq!(c.tema, Tema::Grafite);
        assert!(c.suoni);
        assert_eq!(c.volume, 50.0);
        // E una tabella del mirino scritta a metà prende il resto dai predefiniti.
        std::fs::write(&p, "[mirino]\nforma = \"punto\"\n").unwrap();
        let (c, avviso) = carica(&p);
        assert_eq!(avviso, None);
        assert_eq!(c.mirino, Mirino { forma: crate::mirino::Forma::Punto, ..Mirino::default() });
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn i_cm360_hanno_i_loro_limiti() {
        let mut c = Config::predefinita(Gioco::Cm360);
        assert_eq!(c.sensibilita, 40.0);
        c.sensibilita = 250.0; // oltre il 100 degli altri giochi, ma dentro per cm/360
        assert_eq!(c.clone().normalizzata().sensibilita, 250.0);
        c.sensibilita = 0.5;
        assert_eq!(c.normalizzata().sensibilita, 40.0);
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
            moltiplicatore_y: 50.0,
            volume: -4.0,
            ..Config::default()
        }
        .normalizzata();
        assert_eq!(c.moltiplicatore_y, 1.0);
        assert_eq!(c.volume, 50.0);
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
