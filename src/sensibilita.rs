//! Sensibilità e FOV come nei giochi: stessi gradi per count del mouse, stessa visuale.
//!
//! Yaw (gradi di rotazione per ogni count del sensore, a sensibilità 1). Nessuno di questi giochi lo pubblica in un
//! documento ufficiale: i valori sono quelli misurati e usati dai convertitori di sensibilità (mouse-sensitivity.com
//! e gli altri che lo citano), letti il 25/11/2026.
//! - CS2: `m_yaw` 0.022, valore predefinito del Source engine;
//! - Valorant: 0.07 (CS2 / 3.1818 = Valorant);
//! - Apex Legends: 0.022, lo stesso `m_yaw` del Source engine da cui viene il suo motore;
//! - Overwatch 2: 0.0066;
//! - Destiny 2: 0.0066;
//! - Marathon (Bungie, 2026): 0.0033 per "Look Sensitivity" (1-100, 24 di base), la metà di Destiny 2. Due
//!   convertitori d'accordo, letti il 25/11/2026: mouse-sensitivity.com (Overwatch 15 a 800 DPI = Marathon 30,
//!   11.5455 cm per giro) e aiming.pro (Marathon 1 = Overwatch 2 0.5).
//!
//! Tutti usano lo stesso valore in orizzontale e in verticale; il moltiplicatore verticale di aimpro è a parte.
//! "cm/360" non è un gioco: la sensibilità è scritta direttamente in centimetri di tappetino per un giro completo,
//! quindi i gradi per count dipendono dai DPI.
//!
//! FOV, come lo esprime il gioco:
//! - CS2: orizzontale su un'immagine 4:3 (90 di base), Hor+ sugli schermi più larghi;
//! - Valorant: orizzontale su 16:9 (103, fisso nel gioco);
//! - Apex Legends: il valore del menu (70-110, 90 di base) dà `cl_fovScale` = 1 + (valore - 70) * 0.55 / 40, e il FOV
//!   orizzontale vero su 4:3 è 70 * `cl_fovScale` (110 nel menu = 108,5 gradi su 4:3). Fonte: forum di
//!   mouse-sensitivity.com, discussione 7334 su `cl_fovScale` (misura della comunità, non di Respawn);
//! - Overwatch 2: orizzontale su 16:9 (80-103, 103 di base); sugli schermi non 16:9 il gioco taglia invece di
//!   allargare, qui la proiezione resta Hor+;
//! - Destiny 2: orizzontale su 16:9 (55-105, 85 di base): esportstales.com, con la tabella orizzontale/verticale;
//! - Marathon: "Field of View" 55-105, 90 di base, è un moltiplicatore: i gradi orizzontali su 16:9 sono il valore
//!   per 0.96 (90 = 86.4 gradi, 55.688 in verticale; 105 = 100.8). Fonte: mouse-sensitivity.com (scheda del gioco,
//!   tipo di FOV "moltiplicatore", e il calcolo del FOV effettivo; il 100.8 al massimo lo conferma l'autore nel
//!   forum, discussione 10714, 27/02/2026). Gli altri convertitori non danno il FOV;
//! - cm/360: orizzontale su 16:9, 103 di base.
//!
//! Il FOV del gioco si porta in verticale; poi la proiezione usa le proporzioni vere dello schermo (Hor+).
//! I gradi per count non dipendono dal FOV, come nei giochi: il FOV cambia quanto si vede, non quanto si gira.

use serde::{Deserialize, Serialize};

use crate::giro::Caso;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gioco {
    Cs2,
    Valorant,
    Apex,
    Overwatch2,
    Destiny2,
    Marathon,
    /// I centimetri per giro scritti direttamente.
    Cm360,
}

/// Su cosa il gioco esprime il FOV.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BaseFov {
    /// Orizzontale su questa proporzione.
    Orizzontale(f64),
    /// Il valore del menu di Apex Legends (vedi in cima).
    Apex,
    /// Il valore per questo fattore dà i gradi orizzontali su 16:9 (Marathon).
    Moltiplicatore(f64),
}

impl Gioco {
    pub const TUTTI: [Gioco; 7] = [
        Gioco::Cs2,
        Gioco::Valorant,
        Gioco::Apex,
        Gioco::Overwatch2,
        Gioco::Destiny2,
        Gioco::Marathon,
        Gioco::Cm360,
    ];

    pub fn nome(self) -> &'static str {
        match self {
            Gioco::Cs2 => "CS2",
            Gioco::Valorant => "Valorant",
            Gioco::Apex => "Apex Legends",
            Gioco::Overwatch2 => "Overwatch 2",
            Gioco::Destiny2 => "Destiny 2",
            Gioco::Marathon => "Marathon",
            Gioco::Cm360 => "cm/360",
        }
    }

    /// Gradi per count a sensibilità 1. Per cm/360 non c'è: dipende dai DPI.
    pub fn yaw(self) -> f64 {
        match self {
            Gioco::Cs2 | Gioco::Apex => 0.022,
            Gioco::Valorant => 0.07,
            Gioco::Overwatch2 | Gioco::Destiny2 => 0.0066,
            Gioco::Marathon => 0.0033,
            Gioco::Cm360 => f64::NAN,
        }
    }

    pub fn base_fov(self) -> BaseFov {
        match self {
            Gioco::Cs2 => BaseFov::Orizzontale(4.0 / 3.0),
            Gioco::Apex => BaseFov::Apex,
            Gioco::Marathon => BaseFov::Moltiplicatore(0.96),
            _ => BaseFov::Orizzontale(16.0 / 9.0),
        }
    }

    pub fn sensibilita_predefinita(self) -> f64 {
        match self {
            Gioco::Cs2 | Gioco::Apex => 1.25,
            Gioco::Valorant => 0.4,
            Gioco::Overwatch2 | Gioco::Destiny2 => 4.0,
            Gioco::Marathon => 8.0, // come gli altri, circa 40 cm per giro (nel gioco il predefinito è 24)
            Gioco::Cm360 => 40.0,
        }
    }

    pub fn fov_predefinito(self) -> f64 {
        match self {
            Gioco::Cs2 | Gioco::Apex => 90.0,
            Gioco::Valorant | Gioco::Overwatch2 | Gioco::Cm360 => 103.0,
            Gioco::Destiny2 => 85.0,
            Gioco::Marathon => 90.0,
        }
    }

    /// I limiti del valore di sensibilità: per cm/360 sono centimetri.
    pub fn limiti_sensibilita(self) -> (f64, f64) {
        match self {
            Gioco::Cm360 => (1.0, 500.0),
            _ => (0.001, 100.0),
        }
    }

    /// La riga del FOV nel menu.
    pub fn descrizione_fov(self) -> String {
        let base = format!("{:.0}", self.fov_predefinito());
        match self.base_fov() {
            BaseFov::Apex | BaseFov::Moltiplicatore(_) => format!("FOV (valore del menu, {base} di base)"),
            BaseFov::Orizzontale(p) if (p - 4.0 / 3.0).abs() < 1e-9 => {
                format!("FOV (su 4:3, {base} di base)")
            }
            BaseFov::Orizzontale(_) => format!("FOV (su 16:9, {base} di base)"),
        }
    }

    /// Il nome del valore di sensibilità nel menu.
    pub fn nome_sensibilita(self) -> &'static str {
        match self {
            Gioco::Cm360 => "cm per 360°",
            _ => "Sensibilità",
        }
    }
}

/// Gradi di rotazione per un count del mouse. I DPI servono solo a cm/360.
pub fn gradi_per_count(gioco: Gioco, sensibilita: f64, dpi: f64) -> f64 {
    match gioco {
        Gioco::Cm360 => 360.0 / (sensibilita / 2.54 * dpi),
        _ => gioco.yaw() * sensibilita,
    }
}

/// Radianti per count: quello che usa il ciclo dei frame.
pub fn radianti_per_count(gioco: Gioco, sensibilita: f64, dpi: f64) -> f64 {
    gradi_per_count(gioco, sensibilita, dpi).to_radians()
}

/// Centimetri di tappetino per un giro completo, a un dato DPI.
pub fn cm_per_360(gioco: Gioco, sensibilita: f64, dpi: f64) -> f64 {
    if gioco == Gioco::Cm360 {
        return sensibilita;
    }
    360.0 / gradi_per_count(gioco, sensibilita, dpi) / dpi * 2.54
}

/// Il FOV orizzontale vero di Apex Legends su 4:3, in gradi, dal valore del menu.
pub fn fov_apex_4_3(valore: f64) -> f64 {
    70.0 * (1.0 + (valore - 70.0) * 0.55 / 40.0)
}

/// FOV verticale in radianti a partire dal FOV del gioco (sulla sua base).
pub fn fov_verticale(gioco: Gioco, fov_gioco: f64) -> f64 {
    let (orizzontale, proporzione) = match gioco.base_fov() {
        BaseFov::Orizzontale(p) => (fov_gioco, p),
        BaseFov::Apex => (fov_apex_4_3(fov_gioco), 4.0 / 3.0),
        BaseFov::Moltiplicatore(f) => (fov_gioco * f, 16.0 / 9.0),
    };
    2.0 * ((orizzontale.to_radians() / 2.0).tan() / proporzione).atan()
}

/// La sensibilità nel gioco `a` che dà gli stessi gradi per count della sensibilità `s` nel gioco `da`.
pub fn converti(da: Gioco, s: f64, a: Gioco, dpi: f64) -> f64 {
    if da == a {
        return s;
    }
    if a == Gioco::Cm360 {
        return cm_per_360(da, s, dpi);
    }
    gradi_per_count(da, s, dpi) / a.yaw()
}

// ---------------------------------------------------------------- sensibilità a caso

/// Quando cambia la sensibilità a caso.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ogni {
    /// Un valore per tutto il giro.
    Giro,
    /// Un valore nuovo ogni `secondi` di gioco (la pausa non conta).
    Secondi,
}

pub const LIMITI_VARIAZIONE: (f64, f64) = (1.0, 50.0);
pub const LIMITI_SECONDI_VARIAZIONE: (f64, f64) = (1.0, 60.0);

/// La sensibilità a caso: entro ±`percento` della sensibilità scelta, a ogni giro o ogni N secondi.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Variazione {
    pub attiva: bool,
    pub percento: f64,
    pub ogni: Ogni,
    pub secondi: f64,
}

impl Default for Variazione {
    fn default() -> Self {
        Variazione { attiva: false, percento: 10.0, ogni: Ogni::Giro, secondi: 10.0 }
    }
}

impl Variazione {
    pub fn normalizzata(mut self) -> Self {
        let p = Variazione::default();
        let dentro = |v: f64, (a, b): (f64, f64)| v.is_finite() && v >= a && v <= b;
        if !dentro(self.percento, LIMITI_VARIAZIONE) {
            self.percento = p.percento;
        }
        if !dentro(self.secondi, LIMITI_SECONDI_VARIAZIONE) {
            self.secondi = p.secondi;
        }
        self
    }
}

/// I valori presi in un giro: il fattore in uso e il più basso e il più alto, per il riepilogo. Niente allocazioni:
/// si aggiorna nel frame del giro.
#[derive(Clone, Debug)]
pub struct Casuale {
    caso: Caso,
    pub fattore: f64,
    pub minimo: f64,
    pub massimo: f64,
    /// Quanti valori diversi nel giro.
    pub valori: u32,
    /// Secondi di gioco a cui cambia il prossimo (solo con `Ogni::Secondi`).
    prossimo_s: f64,
    attiva: bool,
}

impl Casuale {
    pub fn new(seme: u64) -> Self {
        Casuale {
            caso: Caso::new(seme),
            fattore: 1.0,
            minimo: 1.0,
            massimo: 1.0,
            valori: 0,
            prossimo_s: f64::INFINITY,
            attiva: false,
        }
    }

    fn estrai(&mut self, v: &Variazione) -> f64 {
        let p = v.percento.clamp(LIMITI_VARIAZIONE.0, LIMITI_VARIAZIONE.1) / 100.0;
        let f = 1.0 + (self.caso.prossimo() * 2.0 - 1.0) * p;
        self.fattore = f;
        self.minimo = self.minimo.min(f);
        self.massimo = self.massimo.max(f);
        self.valori += 1;
        f
    }

    /// A inizio giro: il fattore del giro (1 se la variazione è spenta).
    pub fn inizia(&mut self, v: &Variazione) -> f64 {
        self.attiva = v.attiva;
        self.minimo = f64::INFINITY;
        self.massimo = f64::NEG_INFINITY;
        self.valori = 0;
        if !v.attiva {
            self.fattore = 1.0;
            self.minimo = 1.0;
            self.massimo = 1.0;
            self.prossimo_s = f64::INFINITY;
            return 1.0;
        }
        self.prossimo_s = match v.ogni {
            Ogni::Giro => f64::INFINITY,
            Ogni::Secondi => v.secondi.clamp(LIMITI_SECONDI_VARIAZIONE.0, LIMITI_SECONDI_VARIAZIONE.1),
        };
        self.estrai(v)
    }

    /// Nel giro, con i secondi di gioco passati: il fattore nuovo se è ora di cambiarlo.
    #[inline]
    pub fn aggiorna(&mut self, v: &Variazione, secondi_di_gioco: f64, durata_s: f64) -> Option<f64> {
        // Alla fine del giro (o dopo) non si cambia più: un valore usato per zero secondi non va nel riepilogo.
        if !self.attiva || secondi_di_gioco < self.prossimo_s || secondi_di_gioco >= durata_s {
            return None;
        }
        let passo = v.secondi.clamp(LIMITI_SECONDI_VARIAZIONE.0, LIMITI_SECONDI_VARIAZIONE.1);
        while self.prossimo_s <= secondi_di_gioco {
            self.prossimo_s += passo;
        }
        Some(self.estrai(v))
    }

    pub fn attiva(&self) -> bool {
        self.attiva
    }
}

/// La riga del riepilogo con la sensibilità usata; `None` se la variazione era spenta.
pub fn testo_variazione(c: &Casuale, gioco: Gioco, base: f64) -> Option<String> {
    if !c.attiva() || c.valori == 0 {
        return None;
    }
    let cifre = |f: f64| {
        let v = base * f;
        if gioco == Gioco::Cm360 {
            // Più sensibilità vuol dire meno centimetri per giro.
            format!("{:.1} cm", base / f)
        } else if v >= 10.0 {
            format!("{v:.2}")
        } else {
            format!("{v:.3}")
        }
    };
    let pct = |f: f64| format!("{:+.1}%", (f - 1.0) * 100.0);
    Some(if c.valori == 1 {
        format!("{} ({} sulla tua)", cifre(c.fattore), pct(c.fattore))
    } else {
        // Dal valore più piccolo al più grande: per i cm/360 il fattore più alto dà meno centimetri.
        let (a, b) = if gioco == Gioco::Cm360 { (c.massimo, c.minimo) } else { (c.minimo, c.massimo) };
        format!("da {} a {} ({} valori, da {} a {})", cifre(a), cifre(b), c.valori, pct(a), pct(b))
    })
}

#[cfg(test)]
mod test {
    use super::*;

    fn vicino(a: f64, b: f64, tolleranza: f64) {
        assert!((a - b).abs() <= tolleranza, "{a} non è {b} (±{tolleranza})");
    }

    #[test]
    fn cs2_yaw_0_022_per_sensibilita() {
        assert_eq!(Gioco::Cs2.yaw(), 0.022);
        vicino(gradi_per_count(Gioco::Cs2, 1.0, 800.0), 0.022, 1e-12);
        vicino(gradi_per_count(Gioco::Cs2, 2.5, 800.0), 0.055, 1e-12);
    }

    #[test]
    fn valorant_yaw_0_07() {
        assert_eq!(Gioco::Valorant.yaw(), 0.07);
        vicino(gradi_per_count(Gioco::Valorant, 0.4, 800.0), 0.028, 1e-12);
    }

    #[test]
    fn radianti_coerenti_con_i_gradi() {
        vicino(radianti_per_count(Gioco::Cs2, 1.0, 800.0), 0.022f64.to_radians(), 1e-15);
    }

    #[test]
    fn raddoppiare_la_sensibilita_raddoppia_i_gradi() {
        let a = gradi_per_count(Gioco::Cs2, 1.0, 800.0);
        let b = gradi_per_count(Gioco::Cs2, 2.0, 800.0);
        vicino(b / a, 2.0, 1e-12);
    }

    #[test]
    fn conversione_cs2_valorant_tiene_i_gradi() {
        let v = converti(Gioco::Cs2, 1.5, Gioco::Valorant, 800.0);
        vicino(v, 1.5 / (0.07 / 0.022), 1e-12);
        vicino(gradi_per_count(Gioco::Valorant, v, 800.0), gradi_per_count(Gioco::Cs2, 1.5, 800.0), 1e-12);
        vicino(converti(Gioco::Valorant, v, Gioco::Cs2, 800.0), 1.5, 1e-12);
        vicino(converti(Gioco::Cs2, 2.0, Gioco::Cs2, 800.0), 2.0, 0.0);
    }

    #[test]
    fn cm_per_360_cs2_sensibilita_1_a_800_dpi() {
        // 360 / 0.022 = 16363.6 count; / 800 = 20.45 pollici; * 2.54 = 51.95 cm
        vicino(cm_per_360(Gioco::Cs2, 1.0, 800.0), 51.95, 0.005);
    }

    #[test]
    fn cm_per_360_valorant() {
        vicino(cm_per_360(Gioco::Valorant, 0.35, 800.0), 360.0 / (0.35 * 0.07) / 800.0 * 2.54, 1e-9);
    }

    #[test]
    fn fov_cs2_90_su_4_3_e_73_74_in_verticale() {
        vicino(fov_verticale(Gioco::Cs2, 90.0).to_degrees(), 73.7398, 1e-3);
    }

    #[test]
    fn fov_valorant_103_su_16_9_e_70_53_in_verticale() {
        vicino(fov_verticale(Gioco::Valorant, 103.0).to_degrees(), 70.5328, 1e-3);
    }

    #[test]
    fn cs2_90_su_4_3_e_106_26_su_16_9() {
        let v = fov_verticale(Gioco::Cs2, 90.0);
        let orizzontale = (2.0 * ((v / 2.0).tan() * 16.0 / 9.0).atan()).to_degrees();
        vicino(orizzontale, 106.26, 0.01);
    }

    #[test]
    fn fov_piu_largo_da_verticale_piu_largo() {
        assert!(fov_verticale(Gioco::Cs2, 100.0) > fov_verticale(Gioco::Cs2, 90.0));
    }

    #[test]
    fn predefiniti() {
        assert_eq!(Gioco::Cs2.fov_predefinito(), 90.0);
        assert_eq!(Gioco::Valorant.fov_predefinito(), 103.0);
        assert_eq!(Gioco::Cs2.nome(), "CS2");
        assert_eq!(Gioco::Valorant.nome(), "Valorant");
    }

    #[test]
    fn giochi_nuovi_yaw_dalle_fonti() {
        assert_eq!(Gioco::Apex.yaw(), 0.022);
        assert_eq!(Gioco::Overwatch2.yaw(), 0.0066);
        assert_eq!(Gioco::Destiny2.yaw(), 0.0066);
        assert_eq!(Gioco::Marathon.yaw(), 0.0033);
    }

    #[test]
    fn cm360_scritti_a_mano() {
        vicino(cm_per_360(Gioco::Cm360, 40.0, 800.0), 40.0, 1e-9);
        vicino(gradi_per_count(Gioco::Cm360, 40.0, 800.0), 360.0 / (40.0 / 2.54 * 800.0), 1e-12);
        // Un giro completo: (40 cm / 2,54) * 800 count.
        let count = 40.0 / 2.54 * 800.0;
        vicino(gradi_per_count(Gioco::Cm360, 40.0, 800.0) * count, 360.0, 1e-9);
        // Coi DPI doppi, a parità di centimetri, ogni count gira la metà.
        vicino(
            gradi_per_count(Gioco::Cm360, 40.0, 1600.0) * 2.0,
            gradi_per_count(Gioco::Cm360, 40.0, 800.0),
            1e-12,
        );
    }

    #[test]
    fn conversione_da_e_verso_cm360() {
        let cm = converti(Gioco::Cs2, 1.0, Gioco::Cm360, 800.0);
        vicino(cm, 51.95, 0.005);
        vicino(converti(Gioco::Cm360, cm, Gioco::Cs2, 800.0), 1.0, 1e-9);
    }

    #[test]
    fn overwatch_e_destiny_un_terzo_di_cs2_per_tre_virgola_33() {
        // 0.022 / 0.0066 = 3.333: CS2 1.0 = Overwatch 2 3.333 = Destiny 2 3.333.
        for g in [Gioco::Overwatch2, Gioco::Destiny2] {
            vicino(converti(Gioco::Cs2, 1.0, g, 800.0), 0.022 / 0.0066, 1e-9);
        }
        vicino(converti(Gioco::Cs2, 1.0, Gioco::Apex, 800.0), 1.0, 1e-12);
    }

    #[test]
    fn marathon_come_i_convertitori() {
        // mouse-sensitivity.com: Overwatch 15 a 800 DPI = Marathon 30, 11.5455 cm per giro.
        vicino(converti(Gioco::Overwatch2, 15.0, Gioco::Marathon, 800.0), 30.0, 1e-9);
        vicino(cm_per_360(Gioco::Marathon, 30.0, 800.0), 11.5455, 1e-4);
        // aiming.pro: Marathon 1 = Overwatch 2 0.5.
        vicino(converti(Gioco::Marathon, 1.0, Gioco::Overwatch2, 800.0), 0.5, 1e-9);
        assert_eq!(Gioco::Marathon.sensibilita_predefinita(), 8.0);
    }

    #[test]
    fn fov_di_marathon_e_un_moltiplicatore() {
        // mouse-sensitivity.com: Field of View 90 = 86.4 gradi orizzontali e 55.688 verticali su 16:9;
        // 105, il massimo, = 100.8 orizzontali su 16:9.
        assert_eq!(Gioco::Marathon.fov_predefinito(), 90.0);
        vicino(fov_verticale(Gioco::Marathon, 90.0).to_degrees(), 55.688059, 1e-5);
        vicino(fov_verticale(Gioco::Marathon, 90.0), fov_verticale(Gioco::Destiny2, 86.4), 1e-12);
        vicino(fov_verticale(Gioco::Marathon, 105.0), fov_verticale(Gioco::Destiny2, 100.8), 1e-12);
    }

    #[test]
    fn fov_overwatch_destiny_su_16_9() {
        vicino(fov_verticale(Gioco::Overwatch2, 103.0).to_degrees(), 70.5328, 1e-3);
        // Destiny 2: 105 in orizzontale su 16:9 = circa 73 in verticale (tabella di esportstales).
        vicino(fov_verticale(Gioco::Destiny2, 105.0).to_degrees(), 72.5, 0.1);
        vicino(fov_verticale(Gioco::Destiny2, 85.0).to_degrees(), 54.5, 0.1);
    }

    #[test]
    fn fov_apex_dal_valore_del_menu() {
        // 110 nel menu = cl_fovScale 1.55 = 108,5 gradi orizzontali su 4:3; 70 = 70.
        vicino(fov_apex_4_3(110.0), 108.5, 1e-9);
        vicino(fov_apex_4_3(70.0), 70.0, 1e-9);
        let v = fov_verticale(Gioco::Apex, 110.0);
        vicino(v, fov_verticale(Gioco::Cs2, 108.5), 1e-12);
    }

    #[test]
    fn ogni_gioco_ha_nome_e_predefiniti_nei_limiti() {
        for g in Gioco::TUTTI {
            let (a, b) = g.limiti_sensibilita();
            let s = g.sensibilita_predefinita();
            assert!(s >= a && s <= b, "{}", g.nome());
            assert!(!g.nome().is_empty());
            // I predefiniti sono vicini fra loro: fra 35 e 55 cm per giro a 800 DPI.
            let cm = cm_per_360(g, s, 800.0);
            assert!((35.0..=55.0).contains(&cm), "{} {cm}", g.nome());
        }
    }

    #[test]
    fn i_nomi_nel_file_restano_quelli_di_prima() {
        #[derive(Serialize, Deserialize)]
        struct T {
            g: Gioco,
        }
        assert_eq!(toml::to_string(&T { g: Gioco::Cs2 }).unwrap().trim(), "g = \"cs2\"");
        assert_eq!(toml::to_string(&T { g: Gioco::Valorant }).unwrap().trim(), "g = \"valorant\"");
        assert_eq!(toml::to_string(&T { g: Gioco::Overwatch2 }).unwrap().trim(), "g = \"overwatch2\"");
        assert_eq!(toml::to_string(&T { g: Gioco::Cm360 }).unwrap().trim(), "g = \"cm360\"");
    }

    // ------------------------------------------------ sensibilità a caso

    fn var(percento: f64, ogni: Ogni, secondi: f64) -> Variazione {
        Variazione { attiva: true, percento, ogni, secondi }
    }

    #[test]
    fn spenta_il_fattore_e_1() {
        let mut c = Casuale::new(1);
        assert_eq!(c.inizia(&Variazione::default()), 1.0);
        assert_eq!(c.aggiorna(&Variazione::default(), 100.0, 600.0), None);
        assert_eq!(testo_variazione(&c, Gioco::Cs2, 1.0), None);
    }

    #[test]
    fn resta_entro_piu_o_meno_n_per_cento() {
        let mut c = Casuale::new(7);
        let v = var(15.0, Ogni::Giro, 10.0);
        let mut visti = (f64::INFINITY, f64::NEG_INFINITY);
        for _ in 0..20_000 {
            let f = c.inizia(&v);
            assert!((0.85..=1.15).contains(&f), "{f}");
            visti = (visti.0.min(f), visti.1.max(f));
        }
        // E li copre quasi tutti: non è un fattore fisso.
        assert!(visti.0 < 0.86 && visti.1 > 1.14, "{visti:?}");
    }

    #[test]
    fn a_ogni_giro_non_cambia_durante_il_giro() {
        let mut c = Casuale::new(3);
        let v = var(10.0, Ogni::Giro, 5.0);
        let f = c.inizia(&v);
        for s in 0..60 {
            assert_eq!(c.aggiorna(&v, f64::from(s), 60.0), None);
        }
        assert_eq!(c.fattore, f);
        assert_eq!(c.valori, 1);
    }

    #[test]
    fn ogni_n_secondi_cambia_al_secondo_giusto() {
        let mut c = Casuale::new(4);
        let v = var(20.0, Ogni::Secondi, 10.0);
        c.inizia(&v);
        assert_eq!(c.aggiorna(&v, 9.99, 60.0), None);
        let f = c.aggiorna(&v, 10.0, 60.0).expect("a 10 s cambia");
        assert!((0.8..=1.2).contains(&f));
        assert_eq!(c.aggiorna(&v, 19.0, 60.0), None);
        assert!(c.aggiorna(&v, 20.5, 60.0).is_some());
        // Un salto lungo (un frame lento) dà un valore solo, non uno per ogni passo saltato.
        assert!(c.aggiorna(&v, 55.0, 60.0).is_some());
        assert_eq!(c.aggiorna(&v, 59.0, 60.0), None);
        assert_eq!(c.valori, 4);
        assert!(c.minimo <= c.fattore && c.fattore <= c.massimo);
        assert!(c.minimo >= 0.8 && c.massimo <= 1.2);
    }

    #[test]
    fn alla_fine_del_giro_non_esce_un_valore_in_piu() {
        // Ogni 10 s su un giro da 30: valori a 0, 10 e 20 s. A 30 s (l'ultimo frame) niente.
        let mut c = Casuale::new(9);
        let v = var(10.0, Ogni::Secondi, 10.0);
        c.inizia(&v);
        assert!(c.aggiorna(&v, 10.0, 30.0).is_some());
        assert!(c.aggiorna(&v, 20.0, 30.0).is_some());
        assert_eq!(c.aggiorna(&v, 30.0, 30.0), None);
        assert_eq!(c.valori, 3);
    }

    #[test]
    fn coi_cm360_il_riepilogo_va_dal_piu_piccolo() {
        let mut c = Casuale::new(2);
        let v = var(20.0, Ogni::Secondi, 1.0);
        c.inizia(&v);
        for s in 1..20 {
            c.aggiorna(&v, f64::from(s), 30.0);
        }
        let t = testo_variazione(&c, Gioco::Cm360, 40.0).unwrap();
        let numeri: Vec<f64> = t.split(' ').filter_map(|p| p.parse::<f64>().ok()).take(2).collect();
        assert!(numeri[0] < numeri[1], "{t}");
    }

    #[test]
    fn il_riepilogo_dice_il_valore_usato() {
        let mut c = Casuale::new(5);
        let f = c.inizia(&var(10.0, Ogni::Giro, 10.0));
        let t = testo_variazione(&c, Gioco::Cs2, 2.0).unwrap();
        assert!(t.starts_with(&format!("{:.3}", 2.0 * f)), "{t}");
        assert!(t.contains('%'), "{t}");
        let v = var(10.0, Ogni::Secondi, 5.0);
        c.inizia(&v);
        c.aggiorna(&v, 5.0, 60.0);
        let t = testo_variazione(&c, Gioco::Cs2, 2.0).unwrap();
        assert!(t.starts_with("da ") && t.contains("2 valori"), "{t}");
    }

    #[test]
    fn variazione_fuori_dai_limiti_torna_al_predefinito() {
        let v =
            Variazione { attiva: true, percento: 400.0, ogni: Ogni::Secondi, secondi: 0.0 }.normalizzata();
        assert_eq!(v.percento, 10.0);
        assert_eq!(v.secondi, 10.0);
        assert!(v.attiva);
    }
}
