//! Il mirino a parametri: forma (croce, punto, cerchio), lunghezza, spessore, spazio dal centro, bordo, colore.
//!
//! `pezzi` fa la geometria in pixel una volta sola, e la usano sia il giro (`grafica.rs`, rettangoli e anelli
//! istanziati) sia l'anteprima del menu (egui): per questo l'anteprima è uguale al giro, pixel per pixel.
//! Si ricalcola solo quando cambiano il mirino o le dimensioni dello schermo, mai a ogni frame.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Forma {
    Croce,
    Punto,
    Cerchio,
}

impl Forma {
    pub const TUTTE: [Forma; 3] = [Forma::Croce, Forma::Punto, Forma::Cerchio];

    pub fn nome(self) -> &'static str {
        match self {
            Forma::Croce => "Croce",
            Forma::Punto => "Punto",
            Forma::Cerchio => "Cerchio",
        }
    }
}

pub const LIMITI_LUNGHEZZA: (f32, f32) = (1.0, 40.0);
pub const LIMITI_SPESSORE: (f32, f32) = (1.0, 12.0);
pub const LIMITI_SPAZIO: (f32, f32) = (0.0, 30.0);
pub const LIMITI_BORDO: (f32, f32) = (0.0, 4.0);

/// Le misure sono in pixel a scala 1 (schermo a 100%).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mirino {
    pub forma: Forma,
    /// Croce: lunghezza di ogni braccio. Cerchio: il raggio.
    pub lunghezza: f32,
    pub spessore: f32,
    /// Croce: lo spazio vuoto fra il centro e i bracci.
    pub spazio: f32,
    /// Il contorno scuro intorno, per vederlo su ogni sfondo (0 = niente).
    pub bordo: f32,
    pub colore: [u8; 3],
}

/// Il mirino di prima di questa versione: una croce bianca con bracci da 6, spessore 2, spazio 4, bordo 1.
impl Default for Mirino {
    fn default() -> Self {
        Mirino {
            forma: Forma::Croce,
            lunghezza: 6.0,
            spessore: 2.0,
            spazio: 4.0,
            bordo: 1.0,
            colore: [255, 255, 255],
        }
    }
}

fn dentro(v: f32, (a, b): (f32, f32)) -> bool {
    v.is_finite() && v >= a && v <= b
}

impl Mirino {
    pub fn normalizzato(mut self) -> Self {
        let p = Mirino::default();
        if !dentro(self.lunghezza, LIMITI_LUNGHEZZA) {
            self.lunghezza = p.lunghezza;
        }
        if !dentro(self.spessore, LIMITI_SPESSORE) {
            self.spessore = p.spessore;
        }
        if !dentro(self.spazio, LIMITI_SPAZIO) {
            self.spazio = p.spazio;
        }
        if !dentro(self.bordo, LIMITI_BORDO) {
            self.bordo = p.bordo;
        }
        self
    }
}

/// Un pezzo del mirino in pixel dello schermo: un rettangolo pieno, o un anello (`anello` = raggio interno ed
/// esterno, centrato nel rettangolo).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pezzo {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub colore: [f32; 4],
    pub anello: Option<(f32, f32)>,
}

/// Il massimo dei pezzi: 4 bracci col bordo sotto.
pub const MAX_PEZZI: usize = 8;

const COLORE_BORDO: [f32; 4] = [0.0, 0.0, 0.0, 0.75];

fn colore(m: &Mirino) -> [f32; 4] {
    [f32::from(m.colore[0]) / 255.0, f32::from(m.colore[1]) / 255.0, f32::from(m.colore[2]) / 255.0, 1.0]
}

/// I pezzi del mirino centrato in (`cx`, `cy`), prima il bordo e poi il colore sopra. Restituisce quanti.
pub fn pezzi(m: &Mirino, cx: f32, cy: f32, scala: f32, out: &mut [Pezzo; MAX_PEZZI]) -> usize {
    let m = m.normalizzato();
    let s = scala.max(1.0);
    let cx = cx.round();
    let cy = cy.round();
    let spessore = (m.spessore * s).round().max(1.0);
    let lunghezza = (m.lunghezza * s).round().max(1.0);
    let vuoto = (m.spazio * s).round();
    let bordo = (m.bordo * s).round();
    let mut n = 0;
    let mut metti = |p: Pezzo| {
        if n < MAX_PEZZI {
            out[n] = p;
            n += 1;
        }
    };
    let c = colore(&m);
    match m.forma {
        Forma::Croce => {
            let meta = (spessore / 2.0).floor();
            for (e_bordo, o, col) in [(true, bordo, COLORE_BORDO), (false, 0.0, c)] {
                if e_bordo && bordo == 0.0 {
                    continue;
                }
                let r = |x: f32, y: f32, w: f32, h: f32| Pezzo { x, y, w, h, colore: col, anello: None };
                let (l, t) = (lunghezza + 2.0 * o, spessore + 2.0 * o);
                metti(r(cx + vuoto - o, cy - meta - o, l, t));
                metti(r(cx - vuoto - lunghezza - o, cy - meta - o, l, t));
                metti(r(cx - meta - o, cy + vuoto - o, t, l));
                metti(r(cx - meta - o, cy - vuoto - lunghezza - o, t, l));
            }
        }
        Forma::Punto => {
            let meta = (spessore / 2.0).floor();
            if bordo > 0.0 {
                let l = spessore + 2.0 * bordo;
                metti(Pezzo {
                    x: cx - meta - bordo,
                    y: cy - meta - bordo,
                    w: l,
                    h: l,
                    colore: COLORE_BORDO,
                    anello: None,
                });
            }
            metti(Pezzo { x: cx - meta, y: cy - meta, w: spessore, h: spessore, colore: c, anello: None });
        }
        Forma::Cerchio => {
            // Il raggio è quello del centro della linea.
            let interno = (lunghezza - spessore / 2.0).max(0.0);
            let esterno = interno + spessore;
            let anello = |a: f32, b: f32, col: [f32; 4]| {
                // Un pixel in più per lato: il bordo morbido dell'anello.
                let lato = 2.0 * b + 2.0;
                Pezzo {
                    x: cx - lato / 2.0,
                    y: cy - lato / 2.0,
                    w: lato,
                    h: lato,
                    colore: col,
                    anello: Some((a, b)),
                }
            };
            if bordo > 0.0 {
                metti(anello((interno - bordo).max(0.0), esterno + bordo, COLORE_BORDO));
            }
            metti(anello(interno, esterno, c));
        }
    }
    n
}

#[cfg(test)]
mod test {
    use super::*;

    fn tutti(m: &Mirino) -> Vec<Pezzo> {
        let mut out = [Pezzo::default(); MAX_PEZZI];
        let n = pezzi(m, 960.0, 540.0, 1.0, &mut out);
        out[..n].to_vec()
    }

    #[test]
    fn il_mirino_di_base_e_quello_di_prima() {
        // Prima: bracci da 6, spessore 2, spazio 4, bordo 1, croce bianca col bordo nero al 75%.
        let p = tutti(&Mirino::default());
        assert_eq!(p.len(), 8);
        assert_eq!(p[0], Pezzo { x: 963.0, y: 538.0, w: 8.0, h: 4.0, colore: COLORE_BORDO, anello: None });
        assert_eq!(p[4], Pezzo { x: 964.0, y: 539.0, w: 6.0, h: 2.0, colore: [1.0; 4], anello: None });
        assert!(p[4..].iter().all(|q| q.colore == [1.0; 4]));
    }

    #[test]
    fn senza_bordo_solo_il_colore() {
        let p = tutti(&Mirino { bordo: 0.0, ..Mirino::default() });
        assert_eq!(p.len(), 4);
        assert!(p.iter().all(|q| q.colore != COLORE_BORDO));
    }

    #[test]
    fn la_croce_e_simmetrica_e_lascia_lo_spazio() {
        let m = Mirino { lunghezza: 10.0, spessore: 2.0, spazio: 5.0, bordo: 0.0, ..Mirino::default() };
        let p = tutti(&m);
        // Destra parte a cx + spazio, sinistra finisce a cx - spazio.
        assert_eq!(p[0].x, 965.0);
        assert_eq!(p[1].x + p[1].w, 955.0);
        assert_eq!(p[2].y, 545.0);
        assert_eq!(p[3].y + p[3].h, 535.0);
        assert_eq!((p[0].w, p[0].h), (10.0, 2.0));
        assert_eq!((p[2].w, p[2].h), (2.0, 10.0));
    }

    #[test]
    fn il_punto_e_un_quadrato_al_centro() {
        let m = Mirino { forma: Forma::Punto, spessore: 4.0, bordo: 1.0, ..Mirino::default() };
        let p = tutti(&m);
        assert_eq!(p.len(), 2);
        assert_eq!(p[1], Pezzo { x: 958.0, y: 538.0, w: 4.0, h: 4.0, colore: [1.0; 4], anello: None });
        assert_eq!((p[0].w, p[0].colore), (6.0, COLORE_BORDO));
    }

    #[test]
    fn il_cerchio_ha_raggio_e_spessore() {
        let m =
            Mirino { forma: Forma::Cerchio, lunghezza: 8.0, spessore: 2.0, bordo: 1.0, ..Mirino::default() };
        let p = tutti(&m);
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].anello, Some((7.0, 9.0)));
        assert_eq!(p[0].anello, Some((6.0, 10.0)));
        // Centrato.
        assert_eq!(p[1].x + p[1].w / 2.0, 960.0);
        assert_eq!(p[1].y + p[1].h / 2.0, 540.0);
    }

    #[test]
    fn il_colore_scelto_arriva_ai_pezzi() {
        let m = Mirino { colore: [0, 255, 0], bordo: 0.0, ..Mirino::default() };
        assert!(tutti(&m).iter().all(|q| q.colore == [0.0, 1.0, 0.0, 1.0]));
    }

    #[test]
    fn la_scala_ingrandisce() {
        let mut out = [Pezzo::default(); MAX_PEZZI];
        let n = pezzi(&Mirino { bordo: 0.0, ..Mirino::default() }, 960.0, 540.0, 2.0, &mut out);
        assert_eq!(n, 4);
        assert_eq!((out[0].w, out[0].h), (12.0, 4.0));
    }

    #[test]
    fn valori_assurdi_tornano_ai_predefiniti() {
        let m =
            Mirino { lunghezza: -1.0, spessore: 500.0, spazio: f32::NAN, bordo: 9.0, ..Mirino::default() }
                .normalizzato();
        let p = Mirino::default();
        assert_eq!(
            (m.lunghezza, m.spessore, m.spazio, m.bordo),
            (p.lunghezza, p.spessore, p.spazio, p.bordo)
        );
    }
}
