//! Lo storico dei giri: un file TOML per giro in `%APPDATA%\aimpro\storico\`, scritto a giro finito (fuori dal
//! frame), e il record per scenario. Le altre statistiche stanno in `statistiche.rs`.
//!
//! Il formato (versione `FORMATO`) è descritto nel `CLAUDE.md` del repo: scenari e playlist ci si appoggiano.
//! Un file che non si legge, o di un formato più nuovo, si salta e si conta: l'avvio non si ferma.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::giro::Riepilogo;
use crate::registro::{Bersaglio, Registro};
use crate::sensibilita::{Gioco, cm_per_360};

/// Versione del formato dei file dello storico.
pub const FORMATO: u32 = 1;
/// L'id dello scenario scatto, come sta nei file.
pub const SCATTO: &str = "scatto";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Impostazioni {
    pub gioco: Gioco,
    pub sensibilita: f64,
    pub cm_360: f64,
    pub fov: f64,
    pub dpi: f64,
    pub vsync: bool,
    /// 0 = nessun limite.
    pub limite_fps: u32,
    /// Versione di aimpro che ha giocato il giro.
    pub versione: String,
}

impl Impostazioni {
    pub fn da_config(c: &Config) -> Self {
        Impostazioni {
            gioco: c.gioco,
            sensibilita: c.sensibilita,
            cm_360: cm_per_360(c.gioco, c.sensibilita, c.dpi),
            fov: c.fov,
            dpi: c.dpi,
            vsync: c.vsync,
            limite_fps: c.limite_fps,
            versione: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GiroSalvato {
    pub formato: u32,
    pub scenario: String,
    /// Ora locale di fine giro, `AAAA-MM-GG hh:mm:ss`.
    pub data: String,
    /// Millisecondi dal 1970 (UTC): l'ordine dei giri.
    pub istante_ms: i64,
    pub durata_s: f64,
    pub colpi: u32,
    pub errori: u32,
    /// Colpi su clic, fra 0 e 1; manca senza clic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precisione: Option<f64>,
    /// Media comparsa→colpo; manca senza colpi.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo_medio_ms: Option<f64>,
    /// Tutti gli overshoot del giro, anche sul bersaglio rimasto in campo.
    pub overshoot: u32,
    pub impostazioni: Impostazioni,
    /// I bersagli colpiti, in ordine. In memoria lo storico non li tiene (servono solo nel file).
    #[serde(default)]
    pub bersagli: Vec<Bersaglio>,
}

/// L'ora locale, da Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ora {
    pub anno: u16,
    pub mese: u16,
    pub giorno: u16,
    pub ore: u16,
    pub minuti: u16,
    pub secondi: u16,
    pub millesimi: u16,
}

impl Ora {
    pub fn adesso() -> Self {
        // SAFETY: GetLocalTime scrive solo la struttura che restituisce.
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        Ora {
            anno: t.wYear,
            mese: t.wMonth,
            giorno: t.wDay,
            ore: t.wHour,
            minuti: t.wMinute,
            secondi: t.wSecond,
            millesimi: t.wMilliseconds,
        }
    }

    pub fn testo(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.anno, self.mese, self.giorno, self.ore, self.minuti, self.secondi
        )
    }

    /// Per i nomi dei file: `AAAAMMGG-hhmmss`.
    pub fn compatta(&self) -> String {
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            self.anno, self.mese, self.giorno, self.ore, self.minuti, self.secondi
        )
    }
}

pub fn istante_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl GiroSalvato {
    pub fn nuovo(
        scenario: &str,
        durata_s: f64,
        r: &Riepilogo,
        reg: &Registro,
        c: &Config,
        ora: Ora,
        istante_ms: i64,
    ) -> Self {
        GiroSalvato {
            formato: FORMATO,
            scenario: scenario.to_string(),
            data: ora.testo(),
            istante_ms,
            durata_s,
            colpi: r.colpi,
            errori: r.errori,
            precisione: r.precisione,
            tempo_medio_ms: r.tempo_medio_ms,
            overshoot: reg.overshoot_totale(),
            impostazioni: Impostazioni::da_config(c),
            bersagli: reg.bersagli().to_vec(),
        }
    }
}

/// Come è andato il giro rispetto al record dello scenario di prima.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Confronto {
    pub colpi: u32,
    /// Il record prima di questo giro; `None` se è il primo giro dello scenario.
    pub record_prima: Option<u32>,
}

impl Confronto {
    /// Più colpi del record di prima; il primo giro di uno scenario è record se ha almeno un colpo.
    pub fn nuovo_record(&self) -> bool {
        self.record_prima.map_or(self.colpi > 0, |r| self.colpi > r)
    }

    pub fn differenza(&self) -> Option<i64> {
        self.record_prima.map(|r| i64::from(self.colpi) - i64::from(r))
    }
}

pub struct Storico {
    cartella: PathBuf,
    /// Dal più vecchio al più recente, senza i bersagli.
    giri: Vec<GiroSalvato>,
    /// File trovati e non letti (rovinati, o di un formato più nuovo).
    pub saltati: u32,
}

/// `%APPDATA%\aimpro\storico`, accanto alle impostazioni.
pub fn cartella_predefinita() -> PathBuf {
    let config = crate::config::percorso_predefinito();
    config.parent().map_or_else(|| PathBuf::from("storico"), |p| p.join("storico"))
}

/// Un giro intero dal suo file; `None` se non si legge o è di un formato più nuovo.
pub fn leggi(percorso: &Path) -> Option<GiroSalvato> {
    let testo = std::fs::read_to_string(percorso).ok()?;
    let g: GiroSalvato = toml::from_str(&testo).ok()?;
    (g.formato <= FORMATO && !g.scenario.is_empty()).then_some(g)
}

impl Storico {
    /// Legge tutti i giri della cartella. Una cartella che non c'è è uno storico vuoto. L'app usa `carica_con`.
    #[cfg(test)]
    pub fn carica(cartella: PathBuf) -> Self {
        Self::carica_con(cartella, |_, _| {})
    }

    /// Come `carica`, e passa a `ogni_giro` ogni giro letto, con i bersagli e il suo file, prima che i bersagli si
    /// tolgano dalla memoria (le statistiche ne tengono la sintesi).
    pub fn carica_con(cartella: PathBuf, mut ogni_giro: impl FnMut(&GiroSalvato, &Path)) -> Self {
        let mut giri = Vec::new();
        let mut saltati = 0;
        if let Ok(voci) = std::fs::read_dir(&cartella) {
            for voce in voci.flatten() {
                let p = voce.path();
                if p.extension().is_none_or(|e| e != "toml") {
                    continue;
                }
                match leggi(&p) {
                    Some(mut g) => {
                        ogni_giro(&g, &p);
                        g.bersagli = Vec::new();
                        giri.push(g);
                    }
                    None => saltati += 1,
                }
            }
        }
        giri.sort_by_key(|g| g.istante_ms);
        Storico { cartella, giri, saltati }
    }

    #[cfg(test)]
    pub fn giri(&self) -> &[GiroSalvato] {
        &self.giri
    }

    pub fn giri_di<'a>(&'a self, scenario: &'a str) -> impl DoubleEndedIterator<Item = &'a GiroSalvato> + 'a {
        self.giri.iter().filter(move |g| g.scenario == scenario)
    }

    /// Il record dello scenario: i colpi del giro migliore.
    pub fn record(&self, scenario: &str) -> Option<u32> {
        self.giri_di(scenario).map(|g| g.colpi).max()
    }

    /// Scrive il giro nel suo file e lo aggiunge allo storico; dice il confronto con il record di prima.
    /// Anche se la scrittura non riesce, il giro resta in memoria fino alla chiusura.
    pub fn aggiungi(&mut self, mut g: GiroSalvato, ora: Ora) -> (Confronto, std::io::Result<PathBuf>) {
        let confronto = Confronto { colpi: g.colpi, record_prima: self.record(&g.scenario) };
        let scritto = self.scrivi(&g, ora);
        g.bersagli = Vec::new();
        self.giri.push(g);
        (confronto, scritto)
    }

    fn scrivi(&self, g: &GiroSalvato, ora: Ora) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.cartella)?;
        let testo = toml::to_string(g).map_err(std::io::Error::other)?;
        // Nel nome del file solo lettere, cifre, `-` e `_` dell'id (nel file resta l'id intero).
        let id: String =
            g.scenario.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
        let base = format!("{}-{:03}-{id}", ora.compatta(), ora.millesimi);
        let mut percorso = self.cartella.join(format!("{base}.toml"));
        let mut n = 2;
        while percorso.exists() {
            percorso = self.cartella.join(format!("{base}-{n}.toml"));
            n += 1;
        }
        // Prima un file accanto, poi il nome vero: un'interruzione non lascia nello storico un file a metà.
        let provvisorio = percorso.with_extension("toml.nuovo");
        std::fs::write(&provvisorio, testo)?;
        std::fs::rename(&provvisorio, &percorso)?;
        Ok(percorso)
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::config::Config;

    pub fn cartella_di_prova(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aimpro-test-storico-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    pub fn ora(secondi: u16) -> Ora {
        Ora { anno: 2026, mese: 11, giorno: 22, ore: 14, minuti: 3, secondi, millesimi: 7 }
    }

    pub fn giro_di_prova(colpi: u32, istante_ms: i64) -> GiroSalvato {
        GiroSalvato {
            formato: FORMATO,
            scenario: SCATTO.to_string(),
            data: ora(5).testo(),
            istante_ms,
            durata_s: 30.0,
            colpi,
            errori: 3,
            precisione: Some(f64::from(colpi) / f64::from(colpi + 3)),
            tempo_medio_ms: Some(702.25),
            overshoot: 4,
            impostazioni: Impostazioni::da_config(&Config::default()),
            bersagli: vec![
                Bersaglio {
                    tempo_ms: 640.5,
                    distanza_gradi: 23.25,
                    overshoot: 1,
                    orizzontale_gradi: Some(-20.5),
                    verticale_gradi: Some(11.0),
                },
                Bersaglio {
                    tempo_ms: 764.0,
                    distanza_gradi: 31.0,
                    overshoot: 0,
                    orizzontale_gradi: Some(30.0),
                    verticale_gradi: Some(-7.75),
                },
            ],
        }
    }

    #[test]
    fn un_giro_scritto_si_rilegge_uguale() {
        let d = cartella_di_prova("rilegge");
        let mut s = Storico::carica(d.clone());
        let g = giro_di_prova(42, 1000);
        let (_, scritto) = s.aggiungi(g.clone(), ora(5));
        let p = scritto.unwrap();
        assert!(p.starts_with(&d));
        assert_eq!(p.file_name().unwrap(), "20261122-140305-007-scatto.toml");
        let testo = std::fs::read_to_string(&p).unwrap();
        let letto: GiroSalvato = toml::from_str(&testo).unwrap();
        assert_eq!(letto, g, "nel file anche i bersagli");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn il_file_ha_tutti_i_campi() {
        let testo = toml::to_string(&giro_di_prova(42, 1000)).unwrap();
        for campo in [
            "formato = 1",
            "scenario = \"scatto\"",
            "data = \"2026-11-22 14:03:05\"",
            "istante_ms = 1000",
            "durata_s = 30.0",
            "colpi = 42",
            "errori = 3",
            "precisione = ",
            "tempo_medio_ms = 702.25",
            "overshoot = 4",
            "[impostazioni]",
            "gioco = \"cs2\"",
            "sensibilita = ",
            "cm_360 = ",
            "fov = 90.0",
            "dpi = 800.0",
            "vsync = false",
            "limite_fps = 0",
            "versione = ",
            "[[bersagli]]",
            "tempo_ms = 640.5",
            "distanza_gradi = 23.25",
            "orizzontale_gradi = -20.5",
            "verticale_gradi = 11.0",
        ] {
            assert!(testo.contains(campo), "manca {campo:?} in:\n{testo}");
        }
    }

    #[test]
    fn senza_clic_precisione_e_tempo_mancano_e_si_rileggono() {
        let mut g = giro_di_prova(0, 1);
        g.errori = 0;
        g.precisione = None;
        g.tempo_medio_ms = None;
        g.bersagli.clear();
        let testo = toml::to_string(&g).unwrap();
        assert!(!testo.contains("precisione"));
        assert_eq!(toml::from_str::<GiroSalvato>(&testo).unwrap(), g);
    }

    #[test]
    fn un_file_rovinato_si_salta_e_gli_altri_si_leggono() {
        let d = cartella_di_prova("rovinato");
        let mut s = Storico::carica(d.clone());
        s.aggiungi(giro_di_prova(10, 1000), ora(1)).1.unwrap();
        s.aggiungi(giro_di_prova(20, 2000), ora(2)).1.unwrap();
        std::fs::write(d.join("rotto.toml"), "colpi = [non toml").unwrap();
        std::fs::write(d.join("mezzo.toml"), "formato = 1\ncolpi = 5\n").unwrap();
        let mut futuro = toml::to_string(&giro_di_prova(99, 3000)).unwrap();
        futuro = futuro.replace("formato = 1", "formato = 9");
        std::fs::write(d.join("futuro.toml"), futuro).unwrap();
        std::fs::write(d.join("nota.txt"), "non è dello storico").unwrap();
        std::fs::write(d.join("x.toml.nuovo"), "a metà").unwrap();
        let s = Storico::carica(d.clone());
        assert_eq!(s.giri().iter().map(|g| g.colpi).collect::<Vec<_>>(), [10, 20]);
        assert_eq!(s.saltati, 3);
        assert!(s.giri().iter().all(|g| g.bersagli.is_empty()), "in memoria senza bersagli");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn senza_cartella_lo_storico_e_vuoto() {
        let s = Storico::carica(cartella_di_prova("manca"));
        assert!(s.giri().is_empty());
        assert_eq!((s.saltati, s.record(SCATTO)), (0, None));
    }

    #[test]
    fn il_record_e_il_giro_con_piu_colpi_e_resta_dopo_un_riavvio() {
        let d = cartella_di_prova("record");
        let mut s = Storico::carica(d.clone());
        let (c, _) = s.aggiungi(giro_di_prova(30, 1), ora(1));
        assert!(c.nuovo_record() && c.record_prima.is_none());
        let (c, _) = s.aggiungi(giro_di_prova(25, 2), ora(2));
        assert!(!c.nuovo_record());
        assert_eq!(c.differenza(), Some(-5));
        let (c, _) = s.aggiungi(giro_di_prova(30, 3), ora(3));
        assert!(!c.nuovo_record(), "pareggiare non è un record nuovo");
        assert_eq!(c.differenza(), Some(0));
        let (c, _) = s.aggiungi(giro_di_prova(34, 4), ora(4));
        assert!(c.nuovo_record());
        assert_eq!(c.differenza(), Some(4));
        // Riavvio: si rilegge dai file.
        let s = Storico::carica(d.clone());
        assert_eq!(s.record(SCATTO), Some(34));
        assert_eq!(s.record("altro"), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_id_strano_non_rompe_il_nome_del_file() {
        let d = cartella_di_prova("id");
        let mut s = Storico::carica(d.clone());
        let mut g = giro_di_prova(5, 1);
        g.scenario = "tracking 1/2.v3".to_string();
        let p = s.aggiungi(g, ora(1)).1.unwrap();
        assert_eq!(p.file_name().unwrap(), "20261122-140301-007-tracking_1_2_v3.toml");
        assert_eq!(Storico::carica(d.clone()).record("tracking 1/2.v3"), Some(5));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_primo_giro_senza_colpi_non_e_un_record_nuovo() {
        assert!(!Confronto { colpi: 0, record_prima: None }.nuovo_record());
        assert!(Confronto { colpi: 1, record_prima: None }.nuovo_record());
    }

    #[test]
    fn due_giri_nello_stesso_millesimo_non_si_sovrascrivono() {
        let d = cartella_di_prova("stesso");
        let mut s = Storico::carica(d.clone());
        let a = s.aggiungi(giro_di_prova(1, 1), ora(1)).1.unwrap();
        let b = s.aggiungi(giro_di_prova(2, 2), ora(1)).1.unwrap();
        assert_ne!(a, b);
        assert_eq!(Storico::carica(d.clone()).giri().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn i_giri_si_ordinano_per_istante() {
        let d = cartella_di_prova("ordine");
        let mut s = Storico::carica(d.clone());
        s.aggiungi(giro_di_prova(3, 3000), ora(1)).1.unwrap();
        s.aggiungi(giro_di_prova(1, 1000), ora(2)).1.unwrap();
        s.aggiungi(giro_di_prova(2, 2000), ora(3)).1.unwrap();
        let s = Storico::carica(d.clone());
        assert_eq!(s.giri().iter().map(|g| g.colpi).collect::<Vec<_>>(), [1, 2, 3]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
