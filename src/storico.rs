//! Lo storico dei giri: un file TOML per giro in `%APPDATA%\aimpro\storico\`, scritto a giro finito (fuori dal
//! frame), e i conti che ne vengono: record per scenario, sessione, ultimi giri.
//!
//! Il formato (versione `FORMATO`) è descritto nel `CLAUDE.md` del repo: scenari e playlist ci si appoggiano.
//! Un file che non si legge, o di un formato più nuovo, si salta e si conta: l'avvio non si ferma.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::giro::Riepilogo;
use crate::registro::{Bersaglio, Registro};
use crate::scenari::Parametri;
use crate::sensibilita::{Gioco, cm_per_360};

/// Versione del formato dei file dello storico. Il 2 aggiunge punteggio, tracking e parametri; i file del formato 1
/// si leggono ancora (punteggio = colpi, parametri standard).
pub const FORMATO: u32 = 2;
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
    /// Il punteggio: i colpi, o la percentuale del tracking (0-100). Manca nei file del formato 1: sono i colpi.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub punteggio: Option<f64>,
    /// Negli scenari di tracking, la frazione del giro con il mirino sulla sfera e il tasto premuto (0-1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking: Option<f64>,
    pub impostazioni: Impostazioni,
    /// Dimensione e velocità dei bersagli scelte nel menu; mancano nei file del formato 1 (standard).
    #[serde(default)]
    pub parametri: Parametri,
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
    #[allow(clippy::too_many_arguments)]
    pub fn nuovo(
        scenario: &str,
        durata_s: f64,
        r: &Riepilogo,
        reg: &Registro,
        c: &Config,
        parametri: Parametri,
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
            punteggio: Some(r.punteggio()),
            tracking: r.tracking,
            impostazioni: Impostazioni::da_config(c),
            parametri,
            bersagli: reg.bersagli().to_vec(),
        }
    }

    /// I colpi, o la percentuale del tracking.
    pub fn punteggio(&self) -> f64 {
        self.punteggio.unwrap_or(f64::from(self.colpi))
    }

    /// Giocato coi parametri dello scenario: solo questi fanno record e grafico.
    pub fn standard(&self) -> bool {
        self.parametri.standard()
    }
}

/// Come è andato il giro rispetto al record dello scenario di prima.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Confronto {
    pub punteggio: f64,
    /// Il record prima di questo giro; `None` se è il primo giro (con parametri standard) dello scenario.
    pub record_prima: Option<f64>,
    /// Il giro è coi parametri standard: solo allora può essere un record.
    pub standard: bool,
}

impl Confronto {
    /// Più del record di prima; il primo giro di uno scenario è record se ha un punteggio. Con i parametri cambiati
    /// mai.
    pub fn nuovo_record(&self) -> bool {
        self.standard && self.record_prima.map_or(self.punteggio > 0.0, |r| self.punteggio > r + 1e-9)
    }

    pub fn differenza(&self) -> Option<f64> {
        self.record_prima.map(|r| self.punteggio - r)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sessione {
    /// I giri coi parametri standard da quando è partito aimpro.
    pub giri: usize,
    pub media: Option<f64>,
    pub migliore: Option<f64>,
    /// I giri coi parametri cambiati da quando è partito aimpro (non entrano in media e migliore).
    pub cambiati: usize,
}

fn massimo(v: impl Iterator<Item = f64>) -> Option<f64> {
    v.fold(None, |m, x| Some(m.map_or(x, |m: f64| m.max(x))))
}

pub struct Storico {
    cartella: PathBuf,
    /// Dal più vecchio al più recente, senza i bersagli.
    giri: Vec<GiroSalvato>,
    /// File trovati e non letti (rovinati, o di un formato più nuovo).
    pub saltati: u32,
    /// Quando è partito aimpro: i giri da qui in poi sono la sessione.
    avvio_ms: i64,
}

/// `%APPDATA%\aimpro\storico`, accanto alle impostazioni.
pub fn cartella_predefinita() -> PathBuf {
    let config = crate::config::percorso_predefinito();
    config.parent().map_or_else(|| PathBuf::from("storico"), |p| p.join("storico"))
}

fn leggi(percorso: &Path) -> Option<GiroSalvato> {
    let testo = std::fs::read_to_string(percorso).ok()?;
    let g: GiroSalvato = toml::from_str(&testo).ok()?;
    (g.formato >= 1 && g.formato <= FORMATO && !g.scenario.is_empty()).then_some(g)
}

impl Storico {
    /// Legge tutti i giri della cartella. Una cartella che non c'è è uno storico vuoto.
    pub fn carica(cartella: PathBuf, avvio_ms: i64) -> Self {
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
                        g.bersagli = Vec::new();
                        giri.push(g);
                    }
                    None => saltati += 1,
                }
            }
        }
        giri.sort_by_key(|g| g.istante_ms);
        Storico { cartella, giri, saltati, avvio_ms }
    }

    pub fn giri(&self) -> &[GiroSalvato] {
        &self.giri
    }

    pub fn giri_di<'a>(&'a self, scenario: &'a str) -> impl DoubleEndedIterator<Item = &'a GiroSalvato> + 'a {
        self.giri.iter().filter(move |g| g.scenario == scenario)
    }

    /// Il record dello scenario: il punteggio del giro migliore coi parametri standard.
    pub fn record(&self, scenario: &str) -> Option<f64> {
        massimo(self.giri_di(scenario).filter(|g| g.standard()).map(GiroSalvato::punteggio))
    }

    /// I giri dello scenario da quando è partito aimpro.
    pub fn sessione(&self, scenario: &str) -> Sessione {
        let di_sessione = || self.giri_di(scenario).filter(|g| g.istante_ms >= self.avvio_ms);
        let punti: Vec<f64> = di_sessione().filter(|g| g.standard()).map(GiroSalvato::punteggio).collect();
        Sessione {
            giri: punti.len(),
            media: (!punti.is_empty()).then(|| punti.iter().sum::<f64>() / punti.len() as f64),
            migliore: massimo(punti.iter().copied()),
            cambiati: di_sessione().filter(|g| !g.standard()).count(),
        }
    }

    /// Gli id degli ultimi scenari giocati, dal più recente, senza ripetizioni (per la home).
    pub fn ultimi_giocati(&self, n: usize) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for g in self.giri.iter().rev() {
            if v.len() >= n {
                break;
            }
            if !v.contains(&g.scenario.as_str()) {
                v.push(&g.scenario);
            }
        }
        v
    }

    /// I giri che hanno fatto un record nuovo (coi parametri standard), dal più recente: al più `n`.
    pub fn record_recenti(&self, n: usize) -> Vec<&GiroSalvato> {
        let mut migliori: Vec<(&str, f64)> = Vec::new();
        let mut record = Vec::new();
        for g in self.giri.iter().filter(|g| g.standard()) {
            let p = g.punteggio();
            match migliori.iter_mut().find(|(id, _)| *id == g.scenario) {
                Some((_, m)) if p > *m + 1e-9 => {
                    *m = p;
                    record.push(g);
                }
                Some(_) => {}
                None if p > 0.0 => {
                    migliori.push((&g.scenario, p));
                    record.push(g);
                }
                None => {}
            }
        }
        record.into_iter().rev().take(n).collect()
    }

    /// Scrive il giro nel suo file e lo aggiunge allo storico; dice il confronto con il record di prima.
    /// Anche se la scrittura non riesce, il giro resta in memoria fino alla chiusura.
    pub fn aggiungi(&mut self, mut g: GiroSalvato, ora: Ora) -> (Confronto, std::io::Result<PathBuf>) {
        let confronto = Confronto {
            punteggio: g.punteggio(),
            record_prima: self.record(&g.scenario),
            standard: g.standard(),
        };
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
            punteggio: Some(f64::from(colpi)),
            tracking: None,
            impostazioni: Impostazioni::da_config(&Config::default()),
            parametri: Parametri::STANDARD,
            bersagli: vec![
                Bersaglio { tempo_ms: 640.5, distanza_gradi: 23.25, overshoot: 1 },
                Bersaglio { tempo_ms: 764.0, distanza_gradi: 31.0, overshoot: 0 },
            ],
        }
    }

    #[test]
    fn un_giro_scritto_si_rilegge_uguale() {
        let d = cartella_di_prova("rilegge");
        let mut s = Storico::carica(d.clone(), 0);
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
            "formato = 2",
            "scenario = \"scatto\"",
            "data = \"2026-11-22 14:03:05\"",
            "istante_ms = 1000",
            "durata_s = 30.0",
            "colpi = 42",
            "errori = 3",
            "precisione = ",
            "tempo_medio_ms = 702.25",
            "overshoot = 4",
            "punteggio = 42.0",
            "[parametri]",
            "dimensione = 1.0",
            "velocita = 1.0",
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
        let mut s = Storico::carica(d.clone(), 0);
        s.aggiungi(giro_di_prova(10, 1000), ora(1)).1.unwrap();
        s.aggiungi(giro_di_prova(20, 2000), ora(2)).1.unwrap();
        std::fs::write(d.join("rotto.toml"), "colpi = [non toml").unwrap();
        std::fs::write(d.join("mezzo.toml"), "formato = 1\ncolpi = 5\n").unwrap();
        let mut futuro = toml::to_string(&giro_di_prova(99, 3000)).unwrap();
        futuro = futuro.replace("formato = 2", "formato = 9");
        std::fs::write(d.join("futuro.toml"), futuro).unwrap();
        std::fs::write(d.join("nota.txt"), "non è dello storico").unwrap();
        std::fs::write(d.join("x.toml.nuovo"), "a metà").unwrap();
        let s = Storico::carica(d.clone(), 0);
        assert_eq!(s.giri().iter().map(|g| g.colpi).collect::<Vec<_>>(), [10, 20]);
        assert_eq!(s.saltati, 3);
        assert!(s.giri().iter().all(|g| g.bersagli.is_empty()), "in memoria senza bersagli");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn senza_cartella_lo_storico_e_vuoto() {
        let s = Storico::carica(cartella_di_prova("manca"), 0);
        assert!(s.giri().is_empty());
        assert_eq!((s.saltati, s.record(SCATTO)), (0, None));
    }

    #[test]
    fn il_record_e_il_giro_con_piu_colpi_e_resta_dopo_un_riavvio() {
        let d = cartella_di_prova("record");
        let mut s = Storico::carica(d.clone(), 0);
        let (c, _) = s.aggiungi(giro_di_prova(30, 1), ora(1));
        assert!(c.nuovo_record() && c.record_prima.is_none());
        let (c, _) = s.aggiungi(giro_di_prova(25, 2), ora(2));
        assert!(!c.nuovo_record());
        assert_eq!(c.differenza(), Some(-5.0));
        let (c, _) = s.aggiungi(giro_di_prova(30, 3), ora(3));
        assert!(!c.nuovo_record(), "pareggiare non è un record nuovo");
        assert_eq!(c.differenza(), Some(0.0));
        let (c, _) = s.aggiungi(giro_di_prova(34, 4), ora(4));
        assert!(c.nuovo_record());
        assert_eq!(c.differenza(), Some(4.0));
        // Riavvio: si rilegge dai file.
        let s = Storico::carica(d.clone(), 0);
        assert_eq!(s.record(SCATTO), Some(34.0));
        assert_eq!(s.record("altro"), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn la_sessione_conta_solo_i_giri_da_quando_e_partito() {
        let d = cartella_di_prova("sessione");
        let mut s = Storico::carica(d.clone(), 0);
        s.aggiungi(giro_di_prova(50, 100), ora(1)).1.unwrap();
        let mut s = Storico::carica(d.clone(), 1000);
        assert_eq!(s.sessione(SCATTO), Sessione { giri: 0, media: None, migliore: None, cambiati: 0 });
        s.aggiungi(giro_di_prova(20, 2000), ora(2)).1.unwrap();
        s.aggiungi(giro_di_prova(31, 3000), ora(3)).1.unwrap();
        assert_eq!(
            s.sessione(SCATTO),
            Sessione { giri: 2, media: Some(25.5), migliore: Some(31.0), cambiati: 0 }
        );
        assert_eq!(s.record(SCATTO), Some(50.0), "il record guarda tutto lo storico");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_id_strano_non_rompe_il_nome_del_file() {
        let d = cartella_di_prova("id");
        let mut s = Storico::carica(d.clone(), 0);
        let mut g = giro_di_prova(5, 1);
        g.scenario = "tracking 1/2.v3".to_string();
        let p = s.aggiungi(g, ora(1)).1.unwrap();
        assert_eq!(p.file_name().unwrap(), "20261122-140301-007-tracking_1_2_v3.toml");
        assert_eq!(Storico::carica(d.clone(), 0).record("tracking 1/2.v3"), Some(5.0));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_primo_giro_senza_colpi_non_e_un_record_nuovo() {
        assert!(!Confronto { punteggio: 0.0, record_prima: None, standard: true }.nuovo_record());
        assert!(Confronto { punteggio: 1.0, record_prima: None, standard: true }.nuovo_record());
    }

    #[test]
    fn due_giri_nello_stesso_millesimo_non_si_sovrascrivono() {
        let d = cartella_di_prova("stesso");
        let mut s = Storico::carica(d.clone(), 0);
        let a = s.aggiungi(giro_di_prova(1, 1), ora(1)).1.unwrap();
        let b = s.aggiungi(giro_di_prova(2, 2), ora(1)).1.unwrap();
        assert_ne!(a, b);
        assert_eq!(Storico::carica(d.clone(), 0).giri().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn i_giri_si_ordinano_per_istante() {
        let d = cartella_di_prova("ordine");
        let mut s = Storico::carica(d.clone(), 0);
        s.aggiungi(giro_di_prova(3, 3000), ora(1)).1.unwrap();
        s.aggiungi(giro_di_prova(1, 1000), ora(2)).1.unwrap();
        s.aggiungi(giro_di_prova(2, 2000), ora(3)).1.unwrap();
        let s = Storico::carica(d.clone(), 0);
        assert_eq!(s.giri().iter().map(|g| g.colpi).collect::<Vec<_>>(), [1, 2, 3]);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Un file del formato 1, come lo scriveva aimpro prima degli scenari.
    const FORMATO_1: &str = r#"formato = 1
scenario = "scatto"
data = "2026-11-22 14:00:34"
istante_ms = 1795352434530
durata_s = 30.0
colpi = 88
errori = 0
precisione = 1.0
tempo_medio_ms = 338.28
overshoot = 89

[impostazioni]
gioco = "cs2"
sensibilita = 1.25
cm_360 = 41.56
fov = 90.0
dpi = 800.0
vsync = false
limite_fps = 0
versione = "0.1.0"

[[bersagli]]
tempo_ms = 297.44
distanza_gradi = 24.78
overshoot = 1
"#;

    #[test]
    fn ultimi_giocati_e_record_recenti_restano_dopo_un_riavvio() {
        let d = cartella_di_prova("home");
        let mut s = Storico::carica(d.clone(), 0);
        let giro = |id: &str, colpi: u32, t: i64| {
            let mut g = giro_di_prova(colpi, t);
            g.scenario = id.into();
            g
        };
        s.aggiungi(giro("scatto", 30, 1), ora(1)).1.unwrap();
        s.aggiungi(giro("terna", 20, 2), ora(2)).1.unwrap();
        s.aggiungi(giro("scatto", 25, 3), ora(3)).1.unwrap();
        s.aggiungi(giro("deriva", 0, 4), ora(4)).1.unwrap();
        s.aggiungi(giro("terna", 22, 5), ora(5)).1.unwrap();
        let mut cambiato = giro("scatto", 99, 6);
        cambiato.parametri.dimensione = 2.0;
        s.aggiungi(cambiato, ora(6)).1.unwrap();
        // Riavvio: tutto dai file.
        let s = Storico::carica(d.clone(), 0);
        assert_eq!(s.ultimi_giocati(10), ["scatto", "terna", "deriva"]);
        assert_eq!(s.ultimi_giocati(2), ["scatto", "terna"]);
        let r: Vec<(&str, f64)> =
            s.record_recenti(10).iter().map(|g| (g.scenario.as_str(), g.punteggio())).collect();
        assert_eq!(
            r,
            [("terna", 22.0), ("terna", 20.0), ("scatto", 30.0)],
            "niente zero né parametri cambiati"
        );
        assert_eq!(s.record_recenti(1).len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn i_giri_del_formato_1_si_leggono_ancora() {
        let d = cartella_di_prova("formato1");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("20261122-140034-530-scatto.toml"), FORMATO_1).unwrap();
        let s = Storico::carica(d.clone(), 0);
        assert_eq!(s.saltati, 0);
        let g = &s.giri()[0];
        assert_eq!((g.formato, g.colpi, g.punteggio(), g.tracking), (1, 88, 88.0, None));
        assert!(g.standard(), "senza parametri: quelli standard");
        assert_eq!(s.record(SCATTO), Some(88.0));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn il_formato_2_salva_il_punteggio_del_tracking() {
        let d = cartella_di_prova("tracking");
        let mut s = Storico::carica(d.clone(), 0);
        let mut g = giro_di_prova(0, 1);
        g.scenario = "filo".into();
        (g.errori, g.precisione, g.tempo_medio_ms, g.overshoot) = (0, None, None, 0);
        g.bersagli.clear();
        g.tracking = Some(0.734);
        g.punteggio = Some(73.4);
        let p = s.aggiungi(g.clone(), ora(1)).1.unwrap();
        let testo = std::fs::read_to_string(p).unwrap();
        assert!(testo.contains("formato = 2") && testo.contains("tracking = 0.734"), "{testo}");
        assert!(testo.contains("punteggio = 73.4"), "{testo}");
        let s = Storico::carica(d.clone(), 0);
        assert_eq!(s.giri()[0].tracking, Some(0.734));
        assert_eq!(s.record("filo"), Some(73.4));
        assert_eq!(s.record(SCATTO), None, "record per scenario");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn coi_parametri_cambiati_il_giro_si_salva_ma_non_fa_record() {
        let d = cartella_di_prova("parametri");
        let mut s = Storico::carica(d.clone(), 0);
        s.aggiungi(giro_di_prova(30, 1), ora(1)).1.unwrap();
        let mut g = giro_di_prova(60, 2);
        g.parametri = Parametri { dimensione: 1.5, velocita: 1.0 };
        let (c, scritto) = s.aggiungi(g, ora(2));
        assert!(scritto.is_ok(), "salvato");
        assert!(!c.standard && !c.nuovo_record(), "non è un record");
        assert_eq!(c.record_prima, Some(30.0));
        let s = Storico::carica(d.clone(), 0);
        assert_eq!(s.giri().len(), 2);
        assert_eq!(s.giri()[1].parametri.dimensione, 1.5);
        assert_eq!(s.record(SCATTO), Some(30.0), "il record resta quello coi parametri standard");
        let ses = s.sessione(SCATTO);
        assert_eq!((ses.giri, ses.cambiati, ses.migliore), (2 - 1, 1, Some(30.0)));
        let _ = std::fs::remove_dir_all(&d);
    }
}
