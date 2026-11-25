//! I conti delle statistiche: andamento nel tempo, giorni, settimana contro settimana, punti deboli e dettaglio di un
//! giro. Tutto si calcola fuori dal giro, nella schermata Statistiche, da una sintesi per giro preparata quando lo
//! storico si legge (o quando un giro finisce): in memoria restano pochi numeri per giro, non i bersagli.
//!
//! Il punteggio di un giro passa da `punteggio`, l'unico punto che sa come si conta in ogni scenario.

use std::path::{Path, PathBuf};

pub use crate::storico::leggi as leggi_giro;

use crate::registro::Bersaglio;
use crate::storico::{GiroSalvato, SCATTO};

const GIORNO_MS: i64 = 86_400_000;
/// Giri della media mobile.
pub const FINESTRA_MEDIA: usize = 5;
/// Colpi minimi perché una direzione o una distanza conti fra i punti deboli.
pub const COLPI_MINIMI: u32 = 10;

/// Il punteggio di un giro, quello di record e grafici. Nello scatto sono i colpi.
pub fn punteggio(g: &GiroSalvato) -> f64 {
    f64::from(g.colpi)
}

/// Come si scrive il punteggio di uno scenario.
pub fn testo_punteggio(_scenario: &str, v: f64) -> String {
    format!("{v:.0}")
}

/// Il nome di uno scenario da mostrare.
pub fn nome_scenario(id: &str) -> String {
    let mut c = id.chars();
    c.next().map_or(String::new(), |p| p.to_uppercase().chain(c).collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direzione {
    Sinistra,
    Destra,
    Alto,
    Basso,
}

impl Direzione {
    pub const TUTTE: [Direzione; 4] =
        [Direzione::Sinistra, Direzione::Destra, Direzione::Alto, Direzione::Basso];

    pub fn nome(self) -> &'static str {
        match self {
            Direzione::Sinistra => "verso sinistra",
            Direzione::Destra => "verso destra",
            Direzione::Alto => "verso l'alto",
            Direzione::Basso => "verso il basso",
        }
    }

    fn indice(self) -> usize {
        self as usize
    }
}

/// La direzione del bersaglio rispetto al precedente: quella dello spostamento più lungo fra orizzontale e
/// verticale. `None` nei giri salvati prima che lo spostamento si registrasse.
pub fn direzione(b: &Bersaglio) -> Option<Direzione> {
    let (o, v) = (b.orizzontale_gradi?, b.verticale_gradi?);
    Some(if o.abs() >= v.abs() {
        if o >= 0.0 { Direzione::Destra } else { Direzione::Sinistra }
    } else if v >= 0.0 {
        Direzione::Alto
    } else {
        Direzione::Basso
    })
}

/// Le fasce di distanza dal bersaglio precedente: limite superiore in gradi e nome.
pub const FASCE: [(f64, &str); 4] =
    [(20.0, "fino a 20°"), (35.0, "20-35°"), (50.0, "35-50°"), (f64::INFINITY, "oltre 50°")];

pub fn fascia(distanza_gradi: f64) -> usize {
    FASCE.iter().position(|(limite, _)| distanza_gradi < *limite).unwrap_or(FASCE.len() - 1)
}

/// Colpi, tempi e overshoot di un gruppo di bersagli. Piccolo: ogni giro ne tiene 20.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gruppo {
    pub colpi: u32,
    pub somma_ms: f32,
    /// A 32 bit anche se un giro ne fa poche centinaia: lo stesso tipo fa i totali di migliaia di giri.
    pub overshoot: u32,
    /// Bersagli con almeno un overshoot.
    pub con_overshoot: u32,
}

impl Gruppo {
    fn aggiungi(&mut self, b: &Bersaglio) {
        self.colpi += 1;
        self.somma_ms += b.tempo_ms as f32;
        self.overshoot += b.overshoot;
        self.con_overshoot += u32::from(b.overshoot > 0);
    }

    fn unisci(&mut self, o: &Gruppo) {
        self.colpi += o.colpi;
        self.somma_ms += o.somma_ms;
        self.overshoot += o.overshoot;
        self.con_overshoot += o.con_overshoot;
    }

    pub fn tempo_medio(&self) -> Option<f64> {
        (self.colpi > 0).then(|| f64::from(self.somma_ms) / f64::from(self.colpi))
    }

    pub fn overshoot_per_bersaglio(&self) -> Option<f64> {
        (self.colpi > 0).then(|| f64::from(self.overshoot) / f64::from(self.colpi))
    }

    /// Quota dei bersagli con almeno un overshoot, fra 0 e 1.
    pub fn quota_con_overshoot(&self) -> Option<f64> {
        (self.colpi > 0).then(|| f64::from(self.con_overshoot) / f64::from(self.colpi))
    }
}

/// I bersagli di un giro (o di più giri) per direzione e fascia di distanza insieme: così una direzione si confronta
/// con le altre a parità di distanza. La riga 4 sono i bersagli senza direzione (giri salvati prima).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gruppi {
    incrocio: [[Gruppo; 4]; 5],
}

impl Gruppi {
    pub fn da_bersagli(bersagli: &[Bersaglio]) -> Self {
        let mut g = Gruppi::default();
        for b in bersagli {
            let riga = direzione(b).map_or(4, Direzione::indice);
            g.incrocio[riga][fascia(b.distanza_gradi)].aggiungi(b);
        }
        g
    }

    fn unisci(&mut self, o: &Gruppi) {
        for (a, b) in self.incrocio.iter_mut().flatten().zip(o.incrocio.iter().flatten()) {
            a.unisci(b);
        }
    }

    fn somma<'a>(gruppi: impl Iterator<Item = &'a Gruppo>) -> Gruppo {
        let mut t = Gruppo::default();
        for g in gruppi {
            t.unisci(g);
        }
        t
    }

    pub fn direzione(&self, d: Direzione) -> Gruppo {
        Self::somma(self.incrocio[d.indice()].iter())
    }

    pub fn distanza(&self, fascia: usize) -> Gruppo {
        Self::somma(self.incrocio.iter().map(|r| &r[fascia]))
    }

    pub fn tutti(&self) -> Gruppo {
        Self::somma(self.incrocio.iter().flatten())
    }

    /// Ci sono bersagli con la direzione registrata?
    pub fn ha_direzioni(&self) -> bool {
        self.incrocio[..4].iter().flatten().any(|g| g.colpi > 0)
    }

    /// Quanto la direzione è più lenta della media a parità di distanza: per ogni fascia, il suo tempo medio meno
    /// quello di tutti i bersagli della fascia, pesato con i suoi colpi. `None` sotto `COLPI_MINIMI` colpi.
    pub fn ritardo_direzione(&self, d: Direzione) -> Option<f64> {
        let riga = &self.incrocio[d.indice()];
        let colpi: u32 = riga.iter().map(|g| g.colpi).sum();
        if colpi < COLPI_MINIMI {
            return None;
        }
        let mut somma = 0.0;
        for (f, g) in riga.iter().enumerate() {
            if let (Some(t), Some(tf)) = (g.tempo_medio(), self.distanza(f).tempo_medio()) {
                somma += (t - tf) * f64::from(g.colpi);
            }
        }
        Some(somma / f64::from(colpi))
    }
}

/// Quello che le statistiche tengono di un giro.
#[derive(Clone, Debug, PartialEq)]
pub struct Sintesi {
    pub scenario: String,
    /// Ora locale di fine giro, `AAAA-MM-GG hh:mm:ss`.
    pub data: String,
    pub istante_ms: i64,
    pub punteggio: f64,
    pub colpi: u32,
    pub errori: u32,
    pub precisione: Option<f64>,
    pub tempo_medio_ms: Option<f64>,
    pub overshoot: u32,
    /// "CS2 1.25": gioco e sensibilità del giro.
    pub sensibilita: String,
    pub gruppi: Gruppi,
    /// Il file del giro, per riaprirlo nel dettaglio; `None` se non è stato scritto.
    pub percorso: Option<PathBuf>,
}

impl Sintesi {
    pub fn da_giro(g: &GiroSalvato, percorso: Option<&Path>) -> Self {
        Sintesi {
            scenario: g.scenario.clone(),
            data: g.data.clone(),
            istante_ms: g.istante_ms,
            punteggio: punteggio(g),
            colpi: g.colpi,
            errori: g.errori,
            precisione: g.precisione,
            tempo_medio_ms: g.tempo_medio_ms,
            overshoot: g.overshoot,
            sensibilita: format!("{} {}", g.impostazioni.gioco.nome(), g.impostazioni.sensibilita),
            gruppi: Gruppi::da_bersagli(&g.bersagli),
            percorso: percorso.map(Path::to_path_buf),
        }
    }

    /// Il giorno, `AAAA-MM-GG`.
    pub fn giorno(&self) -> &str {
        self.data.get(..10).unwrap_or(&self.data)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Periodo {
    Settimana,
    Mese,
    Tutto,
}

impl Periodo {
    pub const TUTTI: [Periodo; 3] = [Periodo::Settimana, Periodo::Mese, Periodo::Tutto];

    pub fn nome(self) -> &'static str {
        match self {
            Periodo::Settimana => "7 giorni",
            Periodo::Mese => "30 giorni",
            Periodo::Tutto => "Tutto",
        }
    }

    /// Il primo istante dentro il periodo.
    pub fn da(self, adesso_ms: i64) -> i64 {
        match self {
            Periodo::Settimana => adesso_ms - 7 * GIORNO_MS,
            Periodo::Mese => adesso_ms - 30 * GIORNO_MS,
            Periodo::Tutto => i64::MIN,
        }
    }
}

/// Tutte le sintesi, dalla più vecchia alla più recente.
#[derive(Debug, Default)]
pub struct Statistiche {
    giri: Vec<Sintesi>,
    /// Cresce a ogni giro aggiunto: chi tiene conti fatti sa quando rifarli.
    pub versione: u64,
}

impl Statistiche {
    /// Dalle sintesi lette dallo storico, in qualunque ordine.
    pub fn da_sintesi(mut giri: Vec<Sintesi>) -> Self {
        giri.sort_by_key(|g| g.istante_ms);
        Statistiche { giri, versione: 0 }
    }

    pub fn aggiungi(&mut self, s: Sintesi) {
        // Quasi sempre in coda: un giro nuovo è il più recente.
        let i = self.giri.partition_point(|g| g.istante_ms <= s.istante_ms);
        self.giri.insert(i, s);
        self.versione += 1;
    }

    pub fn giro(&self, i: usize) -> &Sintesi {
        &self.giri[i]
    }

    /// Gli scenari con almeno un giro: scatto per primo, poi gli altri in ordine di nome.
    pub fn scenari(&self) -> Vec<String> {
        let mut v: Vec<&str> = self.giri.iter().map(|g| g.scenario.as_str()).collect();
        v.sort_unstable();
        v.dedup();
        if let Some(i) = v.iter().position(|s| *s == SCATTO) {
            let s = v.remove(i);
            v.insert(0, s);
        }
        v.into_iter().map(str::to_string).collect()
    }

    /// Gli indici dei giri di uno scenario dentro il periodo, dal più vecchio.
    pub fn indici(&self, scenario: &str, periodo: Periodo, adesso_ms: i64) -> Vec<usize> {
        let da = periodo.da(adesso_ms);
        (0..self.giri.len())
            .filter(|&i| self.giri[i].scenario == scenario && self.giri[i].istante_ms >= da)
            .collect()
    }

    /// I giri di uno scenario dentro il periodo, dal più vecchio.
    pub fn giri(&self, scenario: &str, periodo: Periodo, adesso_ms: i64) -> Vec<&Sintesi> {
        self.indici(scenario, periodo, adesso_ms).into_iter().map(|i| &self.giri[i]).collect()
    }
}

/// I giri da quando è partito aimpro.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sessione {
    pub giri: usize,
    pub media: Option<f64>,
    pub migliore: Option<f64>,
}

pub fn sessione(giri: &[&Sintesi], avvio_ms: i64) -> Sessione {
    let v: Vec<f64> = giri.iter().filter(|g| g.istante_ms >= avvio_ms).map(|g| g.punteggio).collect();
    Sessione {
        giri: v.len(),
        media: (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64),
        migliore: v.iter().copied().max_by(f64::total_cmp),
    }
}

/// Media mobile sugli ultimi `finestra` valori presenti, fino a ciascun giro compreso.
pub fn media_mobile(valori: &[Option<f64>], finestra: usize) -> Vec<Option<f64>> {
    let mut ultimi: std::collections::VecDeque<f64> = std::collections::VecDeque::with_capacity(finestra);
    valori
        .iter()
        .map(|v| {
            if let Some(v) = v {
                if ultimi.len() == finestra.max(1) {
                    ultimi.pop_front();
                }
                ultimi.push_back(*v);
            }
            (!ultimi.is_empty()).then(|| ultimi.iter().sum::<f64>() / ultimi.len() as f64)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Giorno {
    pub giorno: String,
    pub giri: usize,
    pub media: f64,
    pub migliore: f64,
}

/// Un riassunto per giorno, il più recente in alto.
pub fn per_giorno(giri: &[&Sintesi]) -> Vec<Giorno> {
    let mut out: Vec<Giorno> = Vec::new();
    for g in giri.iter().rev() {
        match out.last_mut() {
            Some(d) if d.giorno == g.giorno() => {
                d.media = (d.media * d.giri as f64 + g.punteggio) / (d.giri + 1) as f64;
                d.giri += 1;
                d.migliore = d.migliore.max(g.punteggio);
            }
            _ => out.push(Giorno {
                giorno: g.giorno().to_string(),
                giri: 1,
                media: g.punteggio,
                migliore: g.punteggio,
            }),
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settimane {
    pub giri_questa: usize,
    pub media_questa: f64,
    pub giri_prima: usize,
    pub media_prima: f64,
}

impl Settimane {
    /// Miglioramento della media in percentuale (positivo = meglio).
    pub fn miglioramento(&self) -> Option<f64> {
        (self.media_prima > 0.0).then(|| (self.media_questa / self.media_prima - 1.0) * 100.0)
    }
}

/// Gli ultimi 7 giorni contro i 7 di prima; `None` se una delle due settimane non ha giri.
pub fn settimane(giri: &[&Sintesi], adesso_ms: i64) -> Option<Settimane> {
    let media = |da: i64, a: i64| {
        let v: Vec<f64> =
            giri.iter().filter(|g| g.istante_ms >= da && g.istante_ms < a).map(|g| g.punteggio).collect();
        (!v.is_empty()).then(|| (v.len(), v.iter().sum::<f64>() / v.len() as f64))
    };
    let (giri_questa, media_questa) = media(adesso_ms - 7 * GIORNO_MS, i64::MAX)?;
    let (giri_prima, media_prima) = media(adesso_ms - 14 * GIORNO_MS, adesso_ms - 7 * GIORNO_MS)?;
    Some(Settimane { giri_questa, media_questa, giri_prima, media_prima })
}

/// La media di un gruppo di giri.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Media {
    pub giri: usize,
    pub punteggio: f64,
    pub precisione: Option<f64>,
    pub tempo_medio_ms: Option<f64>,
    pub overshoot_per_bersaglio: Option<f64>,
    pub gruppi: Gruppi,
}

fn media_di(giri: &[&Sintesi]) -> Media {
    let n = giri.len().max(1) as f64;
    let precisioni: Vec<f64> = giri.iter().filter_map(|g| g.precisione).collect();
    let colpi: u32 = giri.iter().filter(|g| g.tempo_medio_ms.is_some()).map(|g| g.colpi).sum();
    let somma_tempi: f64 = giri.iter().filter_map(|g| g.tempo_medio_ms.map(|t| t * f64::from(g.colpi))).sum();
    let tutti_colpi: u32 = giri.iter().map(|g| g.colpi).sum();
    let mut gruppi = Gruppi::default();
    for g in giri {
        gruppi.unisci(&g.gruppi);
    }
    Media {
        giri: giri.len(),
        punteggio: giri.iter().map(|g| g.punteggio).sum::<f64>() / n,
        precisione: (!precisioni.is_empty())
            .then(|| precisioni.iter().sum::<f64>() / precisioni.len() as f64),
        tempo_medio_ms: (colpi > 0).then(|| somma_tempi / f64::from(colpi)),
        overshoot_per_bersaglio: (tutti_colpi > 0)
            .then(|| f64::from(giri.iter().map(|g| g.overshoot).sum::<u32>()) / f64::from(tutti_colpi)),
        gruppi,
    }
}

/// La fascia di distanza dove si perde di più rispetto ai giri migliori: il suo tempo medio negli altri giri e nei
/// migliori.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FasciaPeggiore {
    pub fascia: usize,
    pub altri_ms: f64,
    pub migliori_ms: f64,
}

impl FasciaPeggiore {
    /// Quanto più lenti, in percentuale, gli altri giri rispetto ai migliori.
    pub fn scarto(&self) -> f64 {
        (self.altri_ms / self.migliori_ms - 1.0) * 100.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PuntiDeboli {
    pub gruppi: Gruppi,
    /// La direzione più lenta a parità di distanza (con almeno `COLPI_MINIMI` colpi), col suo ritardo in ms, se ce ne
    /// sono almeno due da confrontare.
    pub direzione_peggiore: Option<(Direzione, f64)>,
    pub fascia_peggiore: Option<FasciaPeggiore>,
    /// Il quinto dei giri col punteggio più alto (almeno uno) contro gli altri; `None` con meno di 2 giri.
    pub migliori: Option<(Media, Media)>,
}

fn direzione_peggiore(g: &Gruppi) -> Option<(Direzione, f64)> {
    let validi: Vec<(Direzione, f64)> =
        Direzione::TUTTE.iter().filter_map(|&d| g.ritardo_direzione(d).map(|r| (d, r))).collect();
    if validi.len() < 2 {
        return None;
    }
    validi.into_iter().max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Fra le fasce con abbastanza colpi sia nei giri migliori sia negli altri, quella dove gli altri giri sono più
/// lenti dei migliori, in proporzione.
fn fascia_peggiore(migliori: &Gruppi, altri: &Gruppi) -> Option<FasciaPeggiore> {
    (0..FASCE.len())
        .filter_map(|f| {
            let (m, a) = (migliori.distanza(f), altri.distanza(f));
            if m.colpi < COLPI_MINIMI || a.colpi < COLPI_MINIMI {
                return None;
            }
            Some(FasciaPeggiore { fascia: f, altri_ms: a.tempo_medio()?, migliori_ms: m.tempo_medio()? })
        })
        // Solo dove gli altri giri sono davvero più lenti dei migliori.
        .filter(|f| f.scarto() > 0.0)
        .max_by(|x, y| x.scarto().total_cmp(&y.scarto()))
}

pub fn punti_deboli(giri: &[&Sintesi]) -> PuntiDeboli {
    let mut gruppi = Gruppi::default();
    for g in giri {
        gruppi.unisci(&g.gruppi);
    }
    let migliori = (giri.len() >= 2).then(|| {
        let mut ordinati: Vec<&Sintesi> = giri.to_vec();
        let n = (ordinati.len() / 5).max(1);
        ordinati.select_nth_unstable_by(n - 1, |a, b| b.punteggio.total_cmp(&a.punteggio));
        (media_di(&ordinati[..n]), media_di(&ordinati[n..]))
    });
    PuntiDeboli {
        direzione_peggiore: direzione_peggiore(&gruppi),
        fascia_peggiore: migliori.as_ref().and_then(|(m, a)| fascia_peggiore(&m.gruppi, &a.gruppi)),
        gruppi,
        migliori,
    }
}

/// Il record dello scenario fra i giri dati.
pub fn record(giri: &[&Sintesi]) -> Option<f64> {
    giri.iter().map(|g| g.punteggio).max_by(f64::total_cmp)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::storico::test::giro_di_prova;

    fn b(tempo_ms: f64, distanza: f64, o: f64, v: f64, overshoot: u32) -> Bersaglio {
        Bersaglio {
            tempo_ms,
            distanza_gradi: distanza,
            overshoot,
            orizzontale_gradi: Some(o),
            verticale_gradi: Some(v),
        }
    }

    fn sintesi(punteggio: u32, istante_ms: i64, data: &str) -> Sintesi {
        let mut g = giro_di_prova(punteggio, istante_ms);
        g.data = data.to_string();
        Sintesi::da_giro(&g, None)
    }

    #[test]
    fn la_direzione_e_quella_dello_spostamento_piu_lungo() {
        assert_eq!(direzione(&b(1.0, 30.0, 30.0, 5.0, 0)), Some(Direzione::Destra));
        assert_eq!(direzione(&b(1.0, 30.0, -30.0, 5.0, 0)), Some(Direzione::Sinistra));
        assert_eq!(direzione(&b(1.0, 30.0, 3.0, 20.0, 0)), Some(Direzione::Alto));
        assert_eq!(direzione(&b(1.0, 30.0, -3.0, -20.0, 0)), Some(Direzione::Basso));
        let vecchio = Bersaglio {
            tempo_ms: 1.0,
            distanza_gradi: 30.0,
            overshoot: 0,
            orizzontale_gradi: None,
            verticale_gradi: None,
        };
        assert_eq!(direzione(&vecchio), None);
    }

    #[test]
    fn un_bersaglio_del_formato_vecchio_si_legge_senza_direzione() {
        let vecchio: Bersaglio =
            toml::from_str("tempo_ms = 500.0\ndistanza_gradi = 20.0\novershoot = 1\n").unwrap();
        assert_eq!((vecchio.orizzontale_gradi, vecchio.verticale_gradi), (None, None));
        let g = Gruppi::da_bersagli(&[vecchio]);
        assert_eq!(g.tutti().colpi, 1);
        assert!(!g.ha_direzioni());
        assert_eq!(g.distanza(fascia(20.0)).colpi, 1);
    }

    #[test]
    fn le_fasce_di_distanza() {
        assert_eq!(fascia(10.0), 0);
        assert_eq!(fascia(19.99), 0);
        assert_eq!(fascia(20.0), 1);
        assert_eq!(fascia(49.0), 2);
        assert_eq!(fascia(85.0), 3);
    }

    #[test]
    fn i_gruppi_contano_colpi_tempi_e_overshoot() {
        let g = Gruppi::da_bersagli(&[
            b(400.0, 15.0, 15.0, 0.0, 0),
            b(600.0, 40.0, 40.0, 2.0, 2),
            b(900.0, 25.0, -25.0, 1.0, 1),
        ]);
        let destra = g.direzione(Direzione::Destra);
        assert_eq!((destra.colpi, destra.overshoot, destra.con_overshoot), (2, 2, 1));
        assert_eq!(destra.tempo_medio(), Some(500.0));
        assert_eq!(g.direzione(Direzione::Sinistra).tempo_medio(), Some(900.0));
        assert_eq!(g.direzione(Direzione::Alto).tempo_medio(), None);
        assert_eq!([0, 1, 2, 3].map(|f| g.distanza(f).colpi), [1, 1, 1, 0]);
        assert_eq!(g.tutti().overshoot_per_bersaglio(), Some(1.0));
        assert!((g.tutti().quota_con_overshoot().unwrap() - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn la_sintesi_prende_punteggio_e_gruppi_dal_giro() {
        let g = giro_di_prova(42, 1000);
        let s = Sintesi::da_giro(&g, Some(Path::new("x.toml")));
        assert_eq!(s.punteggio, 42.0);
        assert_eq!(s.gruppi.tutti().colpi, 2);
        assert_eq!(s.percorso.as_deref(), Some(Path::new("x.toml")));
        assert_eq!(s.giorno(), "2026-11-22");
    }

    #[test]
    fn la_media_mobile_salta_i_valori_mancanti() {
        let v = [Some(10.0), None, Some(20.0), Some(30.0), Some(40.0)];
        assert_eq!(media_mobile(&v, 3), [Some(10.0), Some(10.0), Some(15.0), Some(20.0), Some(30.0)]);
        assert_eq!(media_mobile(&[None, Some(4.0)], 3), [None, Some(4.0)]);
        assert!(media_mobile(&[], 5).is_empty());
    }

    #[test]
    fn il_periodo_filtra_per_istante_e_lo_scenario_per_id() {
        let adesso = 100 * GIORNO_MS;
        let mut s = Statistiche::default();
        s.aggiungi(sintesi(10, adesso - 40 * GIORNO_MS, "a"));
        s.aggiungi(sintesi(20, adesso - 10 * GIORNO_MS, "b"));
        s.aggiungi(sintesi(30, adesso - GIORNO_MS, "c"));
        let mut altro = sintesi(99, adesso - GIORNO_MS, "d");
        altro.scenario = "tracking".to_string();
        s.aggiungi(altro);
        let p = |per| s.giri(SCATTO, per, adesso).iter().map(|g| g.punteggio).collect::<Vec<_>>();
        assert_eq!(p(Periodo::Settimana), [30.0]);
        assert_eq!(p(Periodo::Mese), [20.0, 30.0]);
        assert_eq!(p(Periodo::Tutto), [10.0, 20.0, 30.0]);
        assert_eq!(s.scenari(), ["scatto", "tracking"]);
    }

    #[test]
    fn aggiungere_tiene_l_ordine_per_istante() {
        let mut s = Statistiche::da_sintesi(vec![sintesi(4, 400, "x"), sintesi(2, 200, "x")]);
        assert_eq!(s.versione, 0);
        s.aggiungi(sintesi(3, 300, "x"));
        s.aggiungi(sintesi(1, 100, "x"));
        assert_eq!(s.versione, 2);
        let v: Vec<f64> = s.giri(SCATTO, Periodo::Tutto, 0).iter().map(|g| g.punteggio).collect();
        assert_eq!(v, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(s.giro(s.indici(SCATTO, Periodo::Tutto, 0)[2]).punteggio, 3.0);
    }

    #[test]
    fn la_sessione_conta_i_giri_da_quando_e_partito_aimpro() {
        let g = [sintesi(50, 100, "x"), sintesi(20, 2000, "x"), sintesi(31, 3000, "x")];
        let r: Vec<&Sintesi> = g.iter().collect();
        assert_eq!(sessione(&r, 1000), Sessione { giri: 2, media: Some(25.5), migliore: Some(31.0) });
        assert_eq!(sessione(&r, 5000), Sessione { giri: 0, media: None, migliore: None });
    }

    #[test]
    fn il_riassunto_per_giorno() {
        let g = [
            sintesi(10, 1, "2026-11-20 10:00:00"),
            sintesi(20, 2, "2026-11-21 09:00:00"),
            sintesi(30, 3, "2026-11-21 18:00:00"),
            sintesi(25, 4, "2026-11-21 19:00:00"),
        ];
        let r: Vec<&Sintesi> = g.iter().collect();
        assert_eq!(
            per_giorno(&r),
            [
                Giorno { giorno: "2026-11-21".to_string(), giri: 3, media: 25.0, migliore: 30.0 },
                Giorno { giorno: "2026-11-20".to_string(), giri: 1, media: 10.0, migliore: 10.0 },
            ]
        );
    }

    #[test]
    fn la_settimana_contro_quella_prima() {
        let adesso = 100 * GIORNO_MS;
        let g = [
            sintesi(10, adesso - 20 * GIORNO_MS, "x"),
            sintesi(20, adesso - 10 * GIORNO_MS, "x"),
            sintesi(30, adesso - 8 * GIORNO_MS, "x"),
            sintesi(27, adesso - 2 * GIORNO_MS, "x"),
            sintesi(28, adesso - GIORNO_MS, "x"),
        ];
        let r: Vec<&Sintesi> = g.iter().collect();
        let s = settimane(&r, adesso).unwrap();
        assert_eq!((s.giri_questa, s.media_questa, s.giri_prima, s.media_prima), (2, 27.5, 2, 25.0));
        assert!((s.miglioramento().unwrap() - 10.0).abs() < 1e-9);
        assert_eq!(settimane(&r[..3], adesso), None, "niente giri questa settimana");
        assert_eq!(settimane(&r[3..], adesso), None, "niente giri la settimana prima");
    }

    #[test]
    fn il_punto_debole_e_la_direzione_piu_lenta_a_parita_di_distanza() {
        let mut bersagli = Vec::new();
        for _ in 0..12 {
            // A destra lontani e lenti, ma solo perché lontani: alla stessa distanza sono veloci quanto gli altri.
            bersagli.push(b(800.0, 60.0, 60.0, 0.0, 0));
            bersagli.push(b(500.0, 15.0, 15.0, 0.0, 0));
            // Verso l'alto vicini, e più lenti degli altri bersagli vicini.
            bersagli.push(b(650.0, 15.0, 0.0, 15.0, 1));
            bersagli.push(b(800.0, 60.0, -60.0, 0.0, 0));
        }
        // Pochi bersagli in basso e lentissimi: non bastano per contare.
        bersagli.push(b(3000.0, 40.0, 0.0, -40.0, 0));
        let mut g = giro_di_prova(30, 1);
        g.bersagli = bersagli;
        let s = Sintesi::da_giro(&g, None);
        let p = punti_deboli(&[&s]);
        let (d, ritardo) = p.direzione_peggiore.unwrap();
        assert_eq!(d, Direzione::Alto, "senza tenere conto della distanza sarebbe destra o sinistra");
        // Vicini: media di 12 a 500 e 12 a 650 = 575, l'alto ci mette 75 ms in più.
        assert!((ritardo - 75.0).abs() < 1e-3, "{ritardo}");
        assert_eq!(p.fascia_peggiore, None, "un giro solo non ha giri migliori");
        assert_eq!(p.migliori, None, "un giro solo non ha confronto");
        assert_eq!(p.gruppi.direzione(Direzione::Alto).overshoot, 12);
    }

    #[test]
    fn la_fascia_peggiore_e_dove_si_perde_di_piu_rispetto_ai_giri_migliori() {
        let giro = |punteggio: u32, vicini: f64, lontani: f64| {
            let mut g = giro_di_prova(punteggio, i64::from(punteggio));
            g.bersagli = (0..12)
                .flat_map(|_| [b(vicini, 15.0, 15.0, 0.0, 0), b(lontani, 60.0, -60.0, 0.0, 0)])
                .collect();
            Sintesi::da_giro(&g, None)
        };
        // Il giro migliore è più veloce soprattutto sui vicini (400 contro 500, +25%); sui lontani 800 contro 880 (+10%).
        let g = [giro(50, 400.0, 800.0), giro(30, 500.0, 880.0), giro(31, 500.0, 880.0)];
        let r: Vec<&Sintesi> = g.iter().collect();
        let f = punti_deboli(&r).fascia_peggiore.unwrap();
        assert_eq!(f.fascia, 0);
        assert!((f.scarto() - 25.0).abs() < 1e-3, "{}", f.scarto());
        // Se gli altri giri sono più veloci dei migliori in ogni fascia, non c'è una fascia in cui si perde.
        let g = [giro(50, 500.0, 900.0), giro(30, 400.0, 800.0), giro(31, 400.0, 800.0)];
        let r: Vec<&Sintesi> = g.iter().collect();
        assert_eq!(punti_deboli(&r).fascia_peggiore, None);
    }

    #[test]
    fn i_totali_di_tanti_giri_non_si_fermano() {
        // 1000 giri da 80 bersagli, tutti con un overshoot: 80 000 overshoot, oltre i 16 bit.
        let mut g = giro_di_prova(80, 1);
        g.bersagli = (0..80).map(|_| b(500.0, 15.0, 15.0, 0.0, 1)).collect();
        let s = Sintesi::da_giro(&g, None);
        let tanti: Vec<&Sintesi> = (0..1000).map(|_| &s).collect();
        let t = punti_deboli(&tanti).gruppi.tutti();
        assert_eq!((t.colpi, t.overshoot, t.con_overshoot), (80_000, 80_000, 80_000));
        assert_eq!(t.overshoot_per_bersaglio(), Some(1.0));
    }

    #[test]
    fn con_una_direzione_sola_non_c_e_un_peggiore() {
        let mut g = giro_di_prova(30, 1);
        g.bersagli = (0..20).map(|_| b(500.0, 15.0, 15.0, 0.0, 0)).collect();
        let s = Sintesi::da_giro(&g, None);
        assert_eq!(punti_deboli(&[&s]).direzione_peggiore, None);
    }

    #[test]
    fn i_giri_migliori_sono_il_quinto_col_punteggio_piu_alto() {
        let g: Vec<Sintesi> = (1..=10).map(|i| sintesi(i * 10, i64::from(i), "x")).collect();
        let r: Vec<&Sintesi> = g.iter().collect();
        let (migliori, altri) = punti_deboli(&r).migliori.unwrap();
        assert_eq!((migliori.giri, altri.giri), (2, 8));
        assert_eq!(migliori.punteggio, 95.0);
        assert_eq!(altri.punteggio, 45.0);
        assert!(migliori.precisione.unwrap() > altri.precisione.unwrap());
        assert_eq!(record(&r), Some(100.0));
    }

    #[test]
    fn un_giro_si_rilegge_dal_suo_file_per_il_dettaglio() {
        let d = crate::storico::test::cartella_di_prova("dettaglio");
        let mut s = crate::storico::Storico::carica(d.clone());
        let g = giro_di_prova(7, 1);
        let p = s.aggiungi(g.clone(), crate::storico::test::ora(1)).1.unwrap();
        assert_eq!(leggi_giro(&p), Some(g));
        assert_eq!(leggi_giro(&d.join("manca.toml")), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
