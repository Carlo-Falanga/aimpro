//! I colori dei bersagli (normale, guardato, colpito) e i temi della stanza.
//!
//! Vanno alla scheda video nel buffer delle uniformi, riscritto solo quando cambiano (`grafica.rs`): lo shader
//! sceglie il colore di ogni sfera dal suo stato con `colore_bersaglio` (guardata dall'indice della sfera sotto il
//! mirino, colpita per l'istanza del lampo), quindi regge anche con più sfere.

use serde::{Deserialize, Serialize};

/// Quanto dura il lampo del bersaglio colpito, nel punto dove era.
pub const LAMPO_MS: f64 = 120.0;

/// Quanto è ancora visibile il lampo di un colpo dato `ms` fa: da 1 a 0, `None` quando è finito.
pub fn lampo(ms_dal_colpo: f64) -> Option<f32> {
    (0.0..LAMPO_MS).contains(&ms_dal_colpo).then(|| (1.0 - ms_dal_colpo / LAMPO_MS) as f32)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColoriBersaglio {
    pub normale: [u8; 3],
    pub guardato: [u8; 3],
    pub colpito: [u8; 3],
}

/// Il normale è l'arancione di prima; guardato più chiaro; colpito verde.
impl Default for ColoriBersaglio {
    fn default() -> Self {
        ColoriBersaglio { normale: [255, 176, 33], guardato: [255, 222, 140], colpito: [96, 224, 128] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tema {
    /// La stanza grigia di prima.
    Grafite,
    /// Blu scuro, poca luce.
    Notte,
    /// Chiara, calda.
    Sabbia,
}

/// I colori della stanza in gamma (la superficie non è sRGB), da 0 a 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColoriStanza {
    pub pavimento: [f32; 3],
    pub soffitto: [f32; 3],
    pub pareti: [f32; 3],
    pub linee: [f32; 3],
    /// Il fondo dietro i pannelli (si vede solo fuori dalla stanza, all'avvio).
    pub fondo: [f32; 3],
}

impl Tema {
    pub const TUTTI: [Tema; 3] = [Tema::Grafite, Tema::Notte, Tema::Sabbia];

    pub fn nome(self) -> &'static str {
        match self {
            Tema::Grafite => "Grafite",
            Tema::Notte => "Notte",
            Tema::Sabbia => "Sabbia",
        }
    }

    pub fn colori(self) -> ColoriStanza {
        match self {
            Tema::Grafite => ColoriStanza {
                pavimento: [0.105, 0.11, 0.122],
                soffitto: [0.13, 0.135, 0.15],
                pareti: [0.16, 0.168, 0.185],
                linee: [0.27, 0.285, 0.31],
                fondo: [0.07, 0.07, 0.08],
            },
            Tema::Notte => ColoriStanza {
                pavimento: [0.055, 0.07, 0.105],
                soffitto: [0.04, 0.05, 0.08],
                pareti: [0.07, 0.085, 0.125],
                linee: [0.16, 0.22, 0.33],
                fondo: [0.03, 0.035, 0.05],
            },
            Tema::Sabbia => ColoriStanza {
                pavimento: [0.6, 0.56, 0.5],
                soffitto: [0.7, 0.67, 0.62],
                pareti: [0.74, 0.71, 0.66],
                linee: [0.5, 0.46, 0.4],
                fondo: [0.62, 0.6, 0.56],
            },
        }
    }
}

/// Un colore di 8 bit per canale in un vec4 per lo shader (in gamma, come la superficie).
pub fn vec4(c: [u8; 3]) -> [f32; 4] {
    [f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0, 1.0]
}

pub fn vec4_f(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn il_normale_e_l_arancione_di_prima() {
        // Lo shader aveva (1.0, 0.69, 0.13): 255, 176, 33 in 8 bit.
        let c = vec4(ColoriBersaglio::default().normale);
        assert!((c[0] - 1.0).abs() < 0.005 && (c[1] - 0.69).abs() < 0.005 && (c[2] - 0.13).abs() < 0.005);
    }

    #[test]
    fn grafite_e_la_stanza_di_prima() {
        let g = Tema::Grafite.colori();
        assert_eq!(g.pareti, [0.16, 0.168, 0.185]);
        assert_eq!(g.linee, [0.27, 0.285, 0.31]);
        assert_eq!(g.fondo, [0.07, 0.07, 0.08]);
    }

    #[test]
    fn tre_temi_diversi() {
        let c: Vec<ColoriStanza> = Tema::TUTTI.iter().map(|t| t.colori()).collect();
        assert_eq!(c.len(), 3);
        assert!(c[0] != c[1] && c[1] != c[2] && c[0] != c[2]);
        // In ogni tema le linee si staccano dalle pareti.
        for t in &c {
            let d: f32 = (0..3).map(|i| (t.linee[i] - t.pareti[i]).abs()).sum();
            assert!(d > 0.15, "{d}");
        }
    }

    #[test]
    fn il_lampo_finisce() {
        assert_eq!(lampo(0.0), Some(1.0));
        assert!(lampo(60.0).unwrap() > 0.4 && lampo(60.0).unwrap() < 0.6);
        assert_eq!(lampo(LAMPO_MS), None);
        assert_eq!(lampo(-1.0), None);
    }
}
