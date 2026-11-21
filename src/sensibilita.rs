//! Sensibilità e FOV come nei giochi: stessi gradi per count del mouse, stessa visuale.
//!
//! Yaw (gradi di rotazione per ogni count del sensore, a sensibilità 1):
//! - CS2: `m_yaw` 0.022, valore predefinito del Source engine;
//! - Valorant: 0.07, il valore usato dai convertitori di sensibilità (CS2 / 3.1818 = Valorant).
//!   Riot non lo pubblica in un documento ufficiale.
//!
//! Entrambi i giochi usano lo stesso valore in orizzontale e in verticale.
//!
//! FOV:
//! - CS2 lo esprime in orizzontale su un'immagine 4:3 (90 di base) e allarga in Hor+ sugli schermi più larghi;
//! - Valorant lo esprime in orizzontale su 16:9 (103, fisso nel gioco).
//!
//! Il FOV del gioco si porta in verticale; poi la proiezione usa le proporzioni vere dello schermo (Hor+).
//! I gradi per count non dipendono dal FOV, come nei due giochi: il FOV cambia quanto si vede, non quanto si gira.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gioco {
    Cs2,
    Valorant,
}

impl Gioco {
    pub const TUTTI: [Gioco; 2] = [Gioco::Cs2, Gioco::Valorant];

    pub fn nome(self) -> &'static str {
        match self {
            Gioco::Cs2 => "CS2",
            Gioco::Valorant => "Valorant",
        }
    }

    /// Gradi per count a sensibilità 1.
    pub fn yaw(self) -> f64 {
        match self {
            Gioco::Cs2 => 0.022,
            Gioco::Valorant => 0.07,
        }
    }

    /// Proporzioni su cui il gioco esprime il FOV orizzontale.
    pub fn proporzione_fov(self) -> f64 {
        match self {
            Gioco::Cs2 => 4.0 / 3.0,
            Gioco::Valorant => 16.0 / 9.0,
        }
    }

    pub fn sensibilita_predefinita(self) -> f64 {
        match self {
            Gioco::Cs2 => 1.25,
            Gioco::Valorant => 0.4,
        }
    }

    pub fn fov_predefinito(self) -> f64 {
        match self {
            Gioco::Cs2 => 90.0,
            Gioco::Valorant => 103.0,
        }
    }
}

/// Gradi di rotazione per un count del mouse.
pub fn gradi_per_count(gioco: Gioco, sensibilita: f64) -> f64 {
    gioco.yaw() * sensibilita
}

/// Radianti per count: quello che usa il ciclo dei frame.
pub fn radianti_per_count(gioco: Gioco, sensibilita: f64) -> f64 {
    gradi_per_count(gioco, sensibilita).to_radians()
}

/// Centimetri di tappetino per un giro completo, a un dato DPI.
pub fn cm_per_360(gioco: Gioco, sensibilita: f64, dpi: f64) -> f64 {
    360.0 / gradi_per_count(gioco, sensibilita) / dpi * 2.54
}

/// FOV verticale in radianti a partire dal FOV orizzontale del gioco (sulla proporzione del gioco).
pub fn fov_verticale(gioco: Gioco, fov_gioco: f64) -> f64 {
    let orizzontale = fov_gioco.to_radians();
    2.0 * ((orizzontale / 2.0).tan() / gioco.proporzione_fov()).atan()
}

/// La sensibilità nel gioco `a` che dà gli stessi gradi per count della sensibilità `s` nel gioco `da`.
pub fn converti(da: Gioco, s: f64, a: Gioco) -> f64 {
    if da == a {
        return s;
    }
    s * da.yaw() / a.yaw()
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
        vicino(gradi_per_count(Gioco::Cs2, 1.0), 0.022, 1e-12);
        vicino(gradi_per_count(Gioco::Cs2, 2.5), 0.055, 1e-12);
    }

    #[test]
    fn valorant_yaw_0_07() {
        assert_eq!(Gioco::Valorant.yaw(), 0.07);
        vicino(gradi_per_count(Gioco::Valorant, 0.4), 0.028, 1e-12);
    }

    #[test]
    fn radianti_coerenti_con_i_gradi() {
        vicino(radianti_per_count(Gioco::Cs2, 1.0), 0.022f64.to_radians(), 1e-15);
    }

    #[test]
    fn raddoppiare_la_sensibilita_raddoppia_i_gradi() {
        let a = gradi_per_count(Gioco::Cs2, 1.0);
        let b = gradi_per_count(Gioco::Cs2, 2.0);
        vicino(b / a, 2.0, 1e-12);
    }

    #[test]
    fn conversione_cs2_valorant_tiene_i_gradi() {
        let v = converti(Gioco::Cs2, 1.5, Gioco::Valorant);
        vicino(v, 1.5 / (0.07 / 0.022), 1e-12);
        vicino(gradi_per_count(Gioco::Valorant, v), gradi_per_count(Gioco::Cs2, 1.5), 1e-12);
        vicino(converti(Gioco::Valorant, v, Gioco::Cs2), 1.5, 1e-12);
        vicino(converti(Gioco::Cs2, 2.0, Gioco::Cs2), 2.0, 0.0);
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
}
