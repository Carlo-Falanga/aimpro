//! Il registro di un giro: per ogni bersaglio colpito il tempo fra la comparsa e il colpo, la distanza angolare dal
//! bersaglio precedente e gli overshoot (il mirino entra nella sfera e ne esce senza colpirla).
//!
//! Sta fuori da `giro.rs` e legge il giro solo dalla sua parte pubblica (bersaglio, colpi, riepilogo), così uno
//! scenario nuovo lo riusa. Gira nel frame del giro: la memoria si prepara una volta sola (`Registro::new`), e
//! `inizia`, `clic` e `osserva` non allocano.

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::camera::{avanti, colpisce_sfera};
use crate::giro::{Esito, Giro};

/// Bersagli tenuti per giro: in 30 secondi sono un colpo ogni 29 ms; oltre si contano ma non si registrano.
pub const MASSIMO_BERSAGLI: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bersaglio {
    /// Dalla comparsa del bersaglio al colpo, con le pause tolte.
    pub tempo_ms: f64,
    /// Angolo fra questo bersaglio e il precedente; per il primo, fra il bersaglio e la visuale di partenza.
    pub distanza_gradi: f64,
    /// Volte che il mirino è entrato nella sfera e ne è uscito senza colpirla.
    pub overshoot: u32,
}

#[derive(Clone, Debug)]
pub struct Registro {
    occhio: DVec3,
    bersagli: Vec<Bersaglio>,
    /// Colpi oltre `MASSIMO_BERSAGLI`, contati ma non registrati.
    persi: u32,
    /// Direzione del bersaglio precedente (o della visuale di partenza).
    precedente: DVec3,
    /// Direzione del bersaglio in campo.
    attuale: DVec3,
    dentro: bool,
    overshoot: u32,
    overshoot_totale: u32,
    /// Somma dei tempi comparsa→colpo del giro, letta dal riepilogo dopo l'ultimo colpo.
    somma_tempi: f64,
}

/// Somma dei tempi comparsa→colpo del giro, ricavata dal suo riepilogo.
fn somma_tempi(g: &Giro) -> f64 {
    let r = g.riepilogo();
    r.tempo_medio_ms.map_or(0.0, |t| t * f64::from(r.colpi))
}

fn angolo_gradi(a: DVec3, b: DVec3) -> f64 {
    a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
}

impl Registro {
    pub fn new(occhio: DVec3) -> Self {
        Registro {
            occhio,
            bersagli: Vec::with_capacity(MASSIMO_BERSAGLI),
            persi: 0,
            precedente: avanti(0.0, 0.0),
            attuale: avanti(0.0, 0.0),
            dentro: false,
            overshoot: 0,
            overshoot_totale: 0,
            somma_tempi: 0.0,
        }
    }

    fn direzione(&self, punto: DVec3) -> DVec3 {
        (punto - self.occhio).normalize_or_zero()
    }

    /// A giro appena iniziato, con la visuale di partenza `d`.
    pub fn inizia(&mut self, g: &Giro, d: DVec3) {
        self.bersagli.clear();
        self.persi = 0;
        self.precedente = d;
        self.attuale = self.direzione(g.bersaglio);
        self.dentro = false;
        self.overshoot = 0;
        self.overshoot_totale = 0;
        self.somma_tempi = somma_tempi(g);
    }

    /// Dopo ogni clic valutato dal giro, con la direzione del mirino all'istante del clic.
    pub fn clic(&mut self, g: &Giro, esito: Esito, d: DVec3) {
        if esito != Esito::Colpo {
            self.osserva(g, d);
            return;
        }
        let somma = somma_tempi(g);
        let b = Bersaglio {
            tempo_ms: (somma - self.somma_tempi).max(0.0),
            distanza_gradi: angolo_gradi(self.precedente, self.attuale),
            overshoot: self.overshoot,
        };
        if self.bersagli.len() < MASSIMO_BERSAGLI {
            self.bersagli.push(b);
        } else {
            self.persi += 1;
        }
        self.somma_tempi = somma;
        self.precedente = self.attuale;
        self.attuale = self.direzione(g.bersaglio);
        self.dentro = false;
        self.overshoot = 0;
    }

    /// A ogni frame, dopo i clic, con la direzione del mirino già calcolata: conta un overshoot quando il mirino
    /// esce dalla sfera in cui era entrato.
    pub fn osserva(&mut self, g: &Giro, d: DVec3) {
        let dentro = colpisce_sfera(self.occhio, d, g.bersaglio, g.raggio());
        if self.dentro && !dentro {
            self.overshoot += 1;
            self.overshoot_totale += 1;
        }
        self.dentro = dentro;
    }

    pub fn bersagli(&self) -> &[Bersaglio] {
        &self.bersagli
    }

    /// Tutti gli overshoot del giro, compresi quelli sul bersaglio rimasto in campo alla fine.
    pub fn overshoot_totale(&self) -> u32 {
        self.overshoot_totale
    }

    #[cfg(test)]
    pub fn persi(&self) -> u32 {
        self.persi
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::giro::Caso;

    const OCCHIO: DVec3 = DVec3::new(0.0, 1.7, 0.0);

    fn mira(g: &Giro) -> DVec3 {
        (g.bersaglio - OCCHIO).normalize()
    }

    /// Una direzione appena fuori dalla sfera (circa 3 gradi a lato del centro: il raggio ne copre 1,9).
    fn accanto(g: &Giro) -> DVec3 {
        let m = mira(g);
        (m + m.cross(DVec3::Y).normalize() * 0.05).normalize()
    }

    fn pronto(seme: u64) -> (Giro, Registro) {
        let mut g = Giro::new(OCCHIO, Caso::new(seme));
        g.inizia(0.0);
        let mut r = Registro::new(OCCHIO);
        r.inizia(&g, avanti(0.0, 0.0));
        (g, r)
    }

    fn clic(g: &mut Giro, r: &mut Registro, t: f64, d: DVec3) -> Esito {
        let e = g.clic(t, d);
        r.clic(g, e, d);
        e
    }

    #[test]
    fn ogni_colpo_registra_il_suo_tempo() {
        let (mut g, mut r) = pronto(1);
        let d = mira(&g);
        clic(&mut g, &mut r, 400.0, d);
        let d = accanto(&g);
        clic(&mut g, &mut r, 500.0, d);
        let d = mira(&g);
        clic(&mut g, &mut r, 1150.0, d);
        let tempi: Vec<f64> = r.bersagli().iter().map(|b| b.tempo_ms).collect();
        assert_eq!(tempi.len(), 2);
        assert!((tempi[0] - 400.0).abs() < 1e-6, "{tempi:?}");
        assert!((tempi[1] - 750.0).abs() < 1e-6, "{tempi:?}");
    }

    #[test]
    fn la_pausa_non_entra_nel_tempo_del_bersaglio() {
        let (mut g, mut r) = pronto(2);
        g.pausa(100.0);
        g.riprendi(5100.0);
        let d = mira(&g);
        clic(&mut g, &mut r, 5300.0, d);
        assert!((r.bersagli()[0].tempo_ms - 300.0).abs() < 1e-6);
    }

    #[test]
    fn la_distanza_e_quella_dal_bersaglio_precedente() {
        let (mut g, mut r) = pronto(3);
        let primo = mira(&g);
        clic(&mut g, &mut r, 100.0, primo);
        let secondo = mira(&g);
        clic(&mut g, &mut r, 200.0, secondo);
        let b = r.bersagli();
        let attesa_primo = avanti(0.0, 0.0).dot(primo).clamp(-1.0, 1.0).acos().to_degrees();
        let attesa_secondo = primo.dot(secondo).clamp(-1.0, 1.0).acos().to_degrees();
        assert!((b[0].distanza_gradi - attesa_primo).abs() < 1e-9);
        assert!((b[1].distanza_gradi - attesa_secondo).abs() < 1e-9);
        assert!(b[1].distanza_gradi >= 10.0 - 1e-9, "i bersagli distano almeno 10 gradi");
    }

    #[test]
    fn entrare_e_uscire_senza_colpire_e_un_overshoot() {
        let (mut g, mut r) = pronto(4);
        let (dentro, fuori) = (mira(&g), accanto(&g));
        r.osserva(&g, fuori);
        r.osserva(&g, dentro);
        r.osserva(&g, dentro);
        r.osserva(&g, fuori); // 1
        r.osserva(&g, dentro);
        r.osserva(&g, fuori); // 2
        r.osserva(&g, dentro);
        clic(&mut g, &mut r, 900.0, dentro);
        assert_eq!(r.bersagli()[0].overshoot, 2);
        assert_eq!(r.overshoot_totale(), 2);
    }

    #[test]
    fn colpire_entrando_non_e_un_overshoot_e_il_conto_riparte() {
        let (mut g, mut r) = pronto(5);
        let d = mira(&g);
        r.osserva(&g, d);
        clic(&mut g, &mut r, 100.0, d);
        // Il mirino è rimasto dove c'era il bersaglio colpito: fuori dal nuovo, nessun overshoot.
        r.osserva(&g, d);
        let d = mira(&g);
        clic(&mut g, &mut r, 200.0, d);
        assert_eq!(r.bersagli()[0].overshoot, 0);
        assert_eq!(r.bersagli()[1].overshoot, 0);
    }

    #[test]
    fn un_clic_a_vuoto_dopo_essere_entrati_e_un_overshoot() {
        let (mut g, mut r) = pronto(6);
        r.osserva(&g, mira(&g));
        let d = accanto(&g);
        assert_eq!(clic(&mut g, &mut r, 100.0, d), Esito::Errore);
        assert_eq!(r.overshoot_totale(), 1);
    }

    #[test]
    fn la_memoria_non_cresce_nel_giro() {
        let (mut g, mut r) = pronto(7);
        let capacita = r.bersagli.capacity();
        for i in 0..(MASSIMO_BERSAGLI + 10) {
            let d = mira(&g);
            clic(&mut g, &mut r, i as f64 + 1.0, d);
        }
        assert_eq!(r.bersagli().len(), MASSIMO_BERSAGLI);
        assert_eq!(r.persi(), 10);
        assert_eq!(r.bersagli.capacity(), capacita, "nessuna riallocazione");
    }

    #[test]
    fn inizia_azzera() {
        let (mut g, mut r) = pronto(8);
        r.osserva(&g, mira(&g));
        r.osserva(&g, accanto(&g));
        let d = mira(&g);
        clic(&mut g, &mut r, 100.0, d);
        g.inizia(1000.0);
        r.inizia(&g, avanti(0.0, 0.0));
        assert!(r.bersagli().is_empty());
        assert_eq!(r.overshoot_totale(), 0);
        let d = mira(&g);
        clic(&mut g, &mut r, 1250.0, d);
        assert!((r.bersagli()[0].tempo_ms - 250.0).abs() < 1e-6);
    }
}
