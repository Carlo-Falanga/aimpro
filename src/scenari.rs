//! Gli scenari: file TOML con le regole di un giro (quante sfere, quanto grandi, dove, come si muovono, come si fa il
//! punteggio). Quelli di aimpro stanno nella cartella `scenari/` del repo e finiscono dentro l'.exe
//! (`include_str!`); quelli di chi gioca si mettono in `%APPDATA%\aimpro\scenari\` e compaiono nel menu all'avvio.
//!
//! Il formato (versione `FORMATO`) è descritto nel `CLAUDE.md` del repo: playlist e aspetto ci si appoggiano.
//! Un file che non si legge, o con un valore fuori dai limiti, o con l'id di uno scenario già caricato, si salta con un
//! avviso nel menu: gli altri scenari restano.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Versione del formato dei file degli scenari.
pub const FORMATO: u32 = 1;
/// Sfere in campo insieme, al massimo (il buffer della scheda video è grande così).
pub const MAX_SFERE: usize = 8;
/// I limiti dei parametri regolabili nel menu (dimensione e velocità dei bersagli, 1 = come dice lo scenario).
pub const LIMITI_PARAMETRO: (f64, f64) = (0.5, 2.0);

/// Gli scenari di aimpro, nell'ordine del menu.
const DI_AIMPRO: [(&str, &str); 6] = [
    ("scatto.toml", include_str!("../scenari/scatto.toml")),
    ("deriva.toml", include_str!("../scenari/deriva.toml")),
    ("filo.toml", include_str!("../scenari/filo.toml")),
    ("guizzo.toml", include_str!("../scenari/guizzo.toml")),
    ("terna.toml", include_str!("../scenari/terna.toml")),
    ("sciame.toml", include_str!("../scenari/sciame.toml")),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Punteggio {
    /// I colpi: ogni sfera colpita conta uno, ogni clic a vuoto è un errore.
    Colpi,
    /// La percentuale del tempo del giro con il mirino sulla sfera e il tasto sinistro premuto.
    Tracking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TipoMovimento {
    Fermo,
    /// In linea retta, con rimbalzo ai bordi della zona.
    Lineare,
    /// Come lineare, ma cambia direzione all'improvviso a intervalli a caso.
    Casuale,
}

/// Le regole di uno scenario, già convertite in radianti e millisecondi: si copiano nel giro senza allocare.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Regole {
    pub durata_ms: f64,
    pub punteggio: Punteggio,
    pub sfere: usize,
    pub raggio: f64,
    pub distanza: f64,
    pub distanza_minima: f64,
    pub imbardata_max: f64,
    pub beccheggio_min: f64,
    pub beccheggio_max: f64,
    pub movimento: TipoMovimento,
    /// Radianti al secondo.
    pub velocita: f64,
    pub cambio_min_ms: f64,
    pub cambio_max_ms: f64,
}

impl Regole {
    pub fn tracking(&self) -> bool {
        self.punteggio == Punteggio::Tracking
    }

    /// Le regole con i parametri del menu: raggio e velocità moltiplicati.
    pub fn con(mut self, p: Parametri) -> Self {
        self.raggio *= p.dimensione;
        self.velocita *= p.velocita;
        self
    }
}

/// I parametri regolabili nel menu, uguali per tutti gli scenari: 1 = come dice lo scenario.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Parametri {
    pub dimensione: f64,
    pub velocita: f64,
}

impl Parametri {
    pub const STANDARD: Parametri = Parametri { dimensione: 1.0, velocita: 1.0 };

    /// Con parametri diversi dallo standard il giro si salva ma non fa record.
    pub fn standard(&self) -> bool {
        (self.dimensione - 1.0).abs() < 1e-9 && (self.velocita - 1.0).abs() < 1e-9
    }

    /// Dentro i limiti e arrotondati al centesimo (i valori del menu vanno a passi di 5%).
    pub fn normalizzati(self) -> Self {
        let n = |v: f64| {
            if v.is_finite() {
                ((v * 100.0).round() / 100.0).clamp(LIMITI_PARAMETRO.0, LIMITI_PARAMETRO.1)
            } else {
                1.0
            }
        };
        Parametri { dimensione: n(self.dimensione), velocita: n(self.velocita) }
    }
}

impl Default for Parametri {
    fn default() -> Self {
        Parametri::STANDARD
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
    pub id: String,
    pub nome: String,
    pub descrizione: String,
    pub regole: Regole,
    /// Il file da cui viene (`None` per quelli di aimpro).
    pub file: Option<PathBuf>,
}

// ------------------------------------------------------------------ il file

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    formato: u32,
    id: String,
    nome: String,
    #[serde(default)]
    descrizione: String,
    #[serde(default = "trenta")]
    durata_s: f64,
    punteggio: Punteggio,
    bersagli: FileBersagli,
    zona: FileZona,
    #[serde(default)]
    movimento: FileMovimento,
}

fn trenta() -> f64 {
    30.0
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileBersagli {
    #[serde(default = "uno")]
    numero: u32,
    raggio: f64,
    #[serde(default = "dodici")]
    distanza: f64,
    #[serde(default)]
    distanza_minima_gradi: f64,
}

fn uno() -> u32 {
    1
}

fn dodici() -> f64 {
    12.0
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileZona {
    imbardata_gradi: f64,
    beccheggio_min_gradi: f64,
    beccheggio_max_gradi: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileMovimento {
    tipo: TipoMovimento,
    #[serde(default)]
    velocita_gradi_s: f64,
    #[serde(default)]
    cambio_min_ms: f64,
    #[serde(default)]
    cambio_max_ms: f64,
}

impl Default for FileMovimento {
    fn default() -> Self {
        FileMovimento {
            tipo: TipoMovimento::Fermo,
            velocita_gradi_s: 0.0,
            cambio_min_ms: 0.0,
            cambio_max_ms: 0.0,
        }
    }
}

/// Controlla che `v` sia un numero fra `min` e `max`.
fn fra(nome: &str, v: f64, min: f64, max: f64) -> Result<(), String> {
    if v.is_finite() && (min..=max).contains(&v) {
        Ok(())
    } else {
        Err(format!("{nome} = {v} fuori dai limiti ({min} - {max})"))
    }
}

/// Legge un file di scenario. L'errore dice cosa non va, in una riga.
pub fn leggi(testo: &str) -> Result<Scenario, String> {
    let f: File = toml::from_str(testo).map_err(|e| {
        let m = e.message().trim().to_string();
        match e.span() {
            Some(s) => format!("riga {}: {m}", testo[..s.start.min(testo.len())].lines().count().max(1)),
            None => m,
        }
    })?;
    if f.formato == 0 || f.formato > FORMATO {
        return Err(format!("formato {} (questo aimpro legge fino al {FORMATO})", f.formato));
    }
    let id = f.id.trim();
    if id.is_empty()
        || id.len() > 40
        || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("id vuoto o con caratteri diversi da lettere, cifre, - e _ (al più 40)".into());
    }
    let nome = f.nome.trim();
    if nome.is_empty() || nome.chars().count() > 40 {
        return Err("nome vuoto o più lungo di 40 caratteri".into());
    }
    if f.descrizione.chars().count() > 200 {
        return Err("descrizione più lunga di 200 caratteri".into());
    }
    fra("durata_s", f.durata_s, 5.0, 300.0)?;
    if !(1..=MAX_SFERE as u32).contains(&f.bersagli.numero) {
        return Err(format!("bersagli.numero = {} fuori dai limiti (1 - {MAX_SFERE})", f.bersagli.numero));
    }
    fra("bersagli.raggio", f.bersagli.raggio, 0.05, 2.0)?;
    fra("bersagli.distanza", f.bersagli.distanza, 3.0, 16.0)?;
    fra("bersagli.distanza_minima_gradi", f.bersagli.distanza_minima_gradi, 0.0, 40.0)?;
    fra("zona.imbardata_gradi", f.zona.imbardata_gradi, 0.0, 80.0)?;
    fra("zona.beccheggio_min_gradi", f.zona.beccheggio_min_gradi, -45.0, 45.0)?;
    fra("zona.beccheggio_max_gradi", f.zona.beccheggio_max_gradi, f.zona.beccheggio_min_gradi, 45.0)?;
    let m = &f.movimento;
    match m.tipo {
        TipoMovimento::Fermo => {}
        TipoMovimento::Lineare => fra("movimento.velocita_gradi_s", m.velocita_gradi_s, 0.0, 180.0)?,
        TipoMovimento::Casuale => {
            fra("movimento.velocita_gradi_s", m.velocita_gradi_s, 0.0, 180.0)?;
            fra("movimento.cambio_min_ms", m.cambio_min_ms, 50.0, 10_000.0)?;
            fra("movimento.cambio_max_ms", m.cambio_max_ms, m.cambio_min_ms, 10_000.0)?;
        }
    }
    let r = f64::to_radians;
    Ok(Scenario {
        id: id.to_string(),
        nome: nome.to_string(),
        descrizione: f.descrizione.trim().to_string(),
        regole: Regole {
            durata_ms: f.durata_s * 1000.0,
            punteggio: f.punteggio,
            sfere: f.bersagli.numero as usize,
            raggio: f.bersagli.raggio,
            distanza: f.bersagli.distanza,
            distanza_minima: r(f.bersagli.distanza_minima_gradi),
            imbardata_max: r(f.zona.imbardata_gradi),
            beccheggio_min: r(f.zona.beccheggio_min_gradi),
            beccheggio_max: r(f.zona.beccheggio_max_gradi),
            movimento: m.tipo,
            velocita: if m.tipo == TipoMovimento::Fermo { 0.0 } else { r(m.velocita_gradi_s) },
            cambio_min_ms: m.cambio_min_ms,
            cambio_max_ms: m.cambio_max_ms,
        },
        file: None,
    })
}

/// Gli scenari di aimpro, dentro l'.exe.
pub fn di_aimpro() -> Vec<Scenario> {
    DI_AIMPRO
        .iter()
        .map(|(nome, testo)| leggi(testo).unwrap_or_else(|e| panic!("scenario di aimpro {nome}: {e}")))
        .collect()
}

/// `%APPDATA%\aimpro\scenari`, accanto alle impostazioni.
pub fn cartella_predefinita() -> PathBuf {
    let config = crate::config::percorso_predefinito();
    config.parent().map_or_else(|| PathBuf::from("scenari"), |p| p.join("scenari"))
}

/// Gli scenari di aimpro e poi quelli della cartella (in ordine di nome del file), con un avviso per ogni file
/// saltato. Una cartella che non c'è non è un errore.
pub fn carica(cartella: &Path) -> (Vec<Scenario>, Vec<String>) {
    let mut scenari = di_aimpro();
    let mut avvisi = Vec::new();
    let Ok(voci) = std::fs::read_dir(cartella) else {
        return (scenari, avvisi);
    };
    let mut file: Vec<PathBuf> = voci
        .flatten()
        .map(|v| v.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml")))
        .collect();
    file.sort();
    for p in file.into_iter().take(200) {
        let nome = p.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let letto = std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| leggi(&t));
        match letto {
            Ok(s) if scenari.iter().any(|x| x.id == s.id) => {
                avvisi.push(format!("scenario {nome} saltato: l'id \"{}\" c'è già", s.id));
            }
            Ok(mut s) => {
                s.file = Some(p);
                scenari.push(s);
            }
            Err(e) => avvisi.push(format!("scenario {nome} saltato: {e}")),
        }
    }
    (scenari, avvisi)
}

#[cfg(test)]
mod test {
    use super::*;

    fn cartella_di_prova(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aimpro-test-scenari-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const MIO: &str = r#"
formato = 1
id = "mio"
nome = "Il mio"
punteggio = "colpi"
[bersagli]
numero = 2
raggio = 0.5
[zona]
imbardata_gradi = 20
beccheggio_min_gradi = 0
beccheggio_max_gradi = 10
"#;

    #[test]
    fn gli_scenari_di_aimpro_sono_sei_e_durano_30_secondi() {
        let s = di_aimpro();
        let id: Vec<&str> = s.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(id, ["scatto", "deriva", "filo", "guizzo", "terna", "sciame"]);
        assert!(s.iter().all(|s| s.regole.durata_ms == 30_000.0));
        assert!(s.iter().all(|s| !s.descrizione.is_empty() && s.file.is_none()));
    }

    #[test]
    fn i_cinque_nuovi_hanno_le_regole_chieste() {
        let s = di_aimpro();
        let r = |id: &str| s.iter().find(|s| s.id == id).unwrap().regole;
        let (deriva, filo, guizzo, terna, sciame) =
            (r("deriva"), r("filo"), r("guizzo"), r("terna"), r("sciame"));
        // Flick con sfere in movimento lento.
        assert_eq!(
            (deriva.punteggio, deriva.sfere, deriva.movimento),
            (Punteggio::Colpi, 1, TipoMovimento::Lineare)
        );
        assert!(deriva.velocita > 0.0 && deriva.velocita < filo.velocita);
        // Tracking preciso: sfera piccola, moto regolare.
        assert_eq!((filo.punteggio, filo.movimento), (Punteggio::Tracking, TipoMovimento::Lineare));
        assert!(filo.raggio < r("scatto").raggio);
        // Tracking reattivo: cambi di direzione improvvisi.
        assert_eq!((guizzo.punteggio, guizzo.movimento), (Punteggio::Tracking, TipoMovimento::Casuale));
        // Cambio di bersaglio con 3 sfere ferme e con 3 in movimento.
        assert_eq!(
            (terna.punteggio, terna.sfere, terna.movimento),
            (Punteggio::Colpi, 3, TipoMovimento::Fermo)
        );
        assert_eq!(sciame.sfere, 3);
        assert_ne!(sciame.movimento, TipoMovimento::Fermo);
        assert!(sciame.velocita > 0.0);
    }

    #[test]
    fn scatto_resta_quello_di_prima() {
        let s = di_aimpro().remove(0);
        let r = s.regole;
        let g = f64::to_radians;
        assert_eq!(s.id, "scatto");
        assert_eq!((r.sfere, r.raggio, r.distanza, r.movimento), (1, 0.4, 12.0, TipoMovimento::Fermo));
        assert_eq!((r.imbardata_max, r.beccheggio_min, r.beccheggio_max), (g(40.0), g(-10.0), g(20.0)));
        assert_eq!(r.distanza_minima, g(10.0));
        assert_eq!(r.punteggio, Punteggio::Colpi);
    }

    #[test]
    fn un_file_con_i_soli_campi_obbligatori_si_legge() {
        let s = leggi(MIO).unwrap();
        assert_eq!((s.id.as_str(), s.nome.as_str(), s.regole.sfere), ("mio", "Il mio", 2));
        assert_eq!(s.regole.durata_ms, 30_000.0, "30 secondi di standard");
        assert_eq!(s.regole.distanza, 12.0);
        assert_eq!(s.regole.movimento, TipoMovimento::Fermo);
    }

    #[test]
    fn un_file_messo_nella_cartella_compare_dopo_quelli_di_aimpro() {
        let d = cartella_di_prova("aggiunto");
        std::fs::write(d.join("mio.toml"), MIO).unwrap();
        std::fs::write(d.join("nota.txt"), "non è uno scenario").unwrap();
        let (s, avvisi) = carica(&d);
        assert!(avvisi.is_empty(), "{avvisi:?}");
        assert_eq!(s.len(), 7);
        assert_eq!(s[6].id, "mio");
        assert_eq!(s[6].file.as_deref(), Some(d.join("mio.toml").as_path()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_file_rovinato_da_un_avviso_e_gli_altri_restano() {
        let d = cartella_di_prova("rovinati");
        std::fs::write(d.join("a-rotto.toml"), "id = [non toml").unwrap();
        std::fs::write(d.join("b-fuori.toml"), MIO.replace("raggio = 0.5", "raggio = 50.0")).unwrap();
        std::fs::write(d.join("c-doppio.toml"), MIO.replace("\"mio\"", "\"scatto\"")).unwrap();
        std::fs::write(d.join("d-futuro.toml"), MIO.replace("formato = 1", "formato = 7")).unwrap();
        std::fs::write(d.join("e-sconosciuto.toml"), format!("colore = \"rosso\"\n{MIO}")).unwrap();
        std::fs::write(d.join("f-buono.toml"), MIO).unwrap();
        let (s, avvisi) = carica(&d);
        assert_eq!(s.len(), 7, "i sei di aimpro e quello buono");
        assert_eq!(s[6].id, "mio");
        assert_eq!(avvisi.len(), 5, "{avvisi:#?}");
        assert!(avvisi[0].starts_with("scenario a-rotto.toml saltato: riga 1"), "{}", avvisi[0]);
        assert!(avvisi[1].contains("bersagli.raggio = 50"), "{}", avvisi[1]);
        assert!(avvisi[2].contains("c'è già"), "{}", avvisi[2]);
        assert!(avvisi[3].contains("formato 7"), "{}", avvisi[3]);
        assert!(avvisi[4].contains("colore"), "{}", avvisi[4]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn senza_cartella_solo_quelli_di_aimpro() {
        let (s, avvisi) = carica(&std::env::temp_dir().join("aimpro-test-scenari-non-esiste"));
        assert_eq!((s.len(), avvisi.len()), (6, 0));
    }

    #[test]
    fn un_movimento_casuale_vuole_gli_intervalli() {
        let t = MIO.to_string() + "[movimento]\ntipo = \"casuale\"\nvelocita_gradi_s = 20\n";
        assert!(leggi(&t).unwrap_err().contains("cambio_min_ms"));
        let t = t + "cambio_min_ms = 300\ncambio_max_ms = 200\n";
        assert!(leggi(&t).unwrap_err().contains("cambio_max_ms"));
    }

    #[test]
    fn i_parametri_standard_e_i_limiti() {
        assert!(Parametri::STANDARD.standard());
        assert!(Parametri::default().standard());
        assert!(!Parametri { dimensione: 1.2, velocita: 1.0 }.standard());
        let p = Parametri { dimensione: 9.0, velocita: f64::NAN }.normalizzati();
        assert_eq!((p.dimensione, p.velocita), (2.0, 1.0));
        assert_eq!(Parametri { dimensione: 0.849_999, velocita: 0.1 }.normalizzati().dimensione, 0.85);
        let r = di_aimpro()[1].regole;
        let c = r.con(Parametri { dimensione: 1.5, velocita: 2.0 });
        assert!((c.raggio - r.raggio * 1.5).abs() < 1e-12 && (c.velocita - r.velocita * 2.0).abs() < 1e-12);
    }
}
