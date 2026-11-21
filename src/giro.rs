//! Il giro dello scenario "Scatto": una sfera alla volta in un punto a caso davanti al giocatore, 60 secondi.
//! Logica pura: i tempi (millisecondi) arrivano da fuori, dall'istante degli eventi, così si prova nei test.

use glam::DVec3;

use crate::camera::{avanti, colpisce_sfera};

pub const DURATA_GIRO_MS: f64 = 60_000.0;

/// Dove compaiono i bersagli, in angoli rispetto a chi guarda dritto davanti (-Z).
pub mod zona {
    pub const IMBARDATA_MAX: f64 = 40.0 * std::f64::consts::PI / 180.0;
    pub const BECCHEGGIO_MIN: f64 = -10.0 * std::f64::consts::PI / 180.0;
    pub const BECCHEGGIO_MAX: f64 = 20.0 * std::f64::consts::PI / 180.0;
    /// Distanza angolare minima fra un bersaglio e il successivo.
    pub const DISTANZA_MINIMA: f64 = 10.0 * std::f64::consts::PI / 180.0;
    pub const DISTANZA: f64 = 12.0;
    pub const RAGGIO: f64 = 0.4;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Esito {
    Colpo,
    Errore,
    Ignorato,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Riepilogo {
    pub colpi: u32,
    pub errori: u32,
    /// Colpi su clic totali, fra 0 e 1; `None` senza clic.
    pub precisione: Option<f64>,
    /// Media fra la comparsa di un bersaglio e il colpo; `None` senza colpi.
    pub tempo_medio_ms: Option<f64>,
}

/// Generatore di numeri a caso piccolo e senza allocazioni (xorshift64*); il seme viene da fuori.
#[derive(Clone, Debug)]
pub struct Caso(u64);

impl Caso {
    pub fn new(seme: u64) -> Self {
        Caso(seme.max(1))
    }

    /// Un numero in [0, 1).
    pub fn prossimo(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Clone, Debug)]
pub struct Giro {
    pub bersaglio: DVec3,
    pub colpi: u32,
    pub errori: u32,
    pub finito: bool,
    pub in_corso: bool,
    pub in_pausa: bool,
    occhio: DVec3,
    caso: Caso,
    inizio: f64,
    inizio_pausa: f64,
    comparsa: f64,
    somma_tempi: f64,
    angoli: (f64, f64),
}

impl Giro {
    pub fn new(occhio: DVec3, caso: Caso) -> Self {
        Giro {
            bersaglio: occhio + DVec3::NEG_Z * zona::DISTANZA,
            colpi: 0,
            errori: 0,
            finito: false,
            in_corso: false,
            in_pausa: false,
            occhio,
            caso,
            inizio: 0.0,
            inizio_pausa: 0.0,
            comparsa: 0.0,
            somma_tempi: 0.0,
            angoli: (0.0, 0.0),
        }
    }

    pub fn raggio(&self) -> f64 {
        zona::RAGGIO
    }

    pub fn inizia(&mut self, t: f64) {
        self.colpi = 0;
        self.errori = 0;
        self.somma_tempi = 0.0;
        self.finito = false;
        self.in_corso = true;
        self.in_pausa = false;
        self.inizio = t;
        self.nuovo_bersaglio(t, true);
    }

    pub fn restante_ms(&self, t: f64) -> f64 {
        if !self.in_corso && !self.finito {
            return DURATA_GIRO_MS;
        }
        let adesso = if self.in_pausa { self.inizio_pausa } else { t };
        (DURATA_GIRO_MS - (adesso - self.inizio)).clamp(0.0, DURATA_GIRO_MS)
    }

    /// Va chiamata a ogni frame: dice `true` quando il giro è finito.
    pub fn aggiorna(&mut self, t: f64) -> bool {
        if self.in_corso && !self.in_pausa && t - self.inizio >= DURATA_GIRO_MS {
            self.in_corso = false;
            self.finito = true;
        }
        self.finito
    }

    pub fn pausa(&mut self, t: f64) {
        if self.in_corso && !self.in_pausa {
            self.in_pausa = true;
            self.inizio_pausa = t;
        }
    }

    pub fn riprendi(&mut self, t: f64) {
        if self.in_pausa {
            let ferma = t - self.inizio_pausa;
            self.inizio += ferma;
            self.comparsa += ferma;
            self.in_pausa = false;
        }
    }

    /// Lascia il giro a metà: niente riepilogo.
    pub fn annulla(&mut self) {
        self.in_corso = false;
        self.in_pausa = false;
        self.finito = false;
    }

    /// Un clic all'istante `t` (quello dell'evento) guardando nella direzione `d` (versore).
    pub fn clic(&mut self, t: f64, d: DVec3) -> Esito {
        if !self.in_corso || self.in_pausa || t < self.inizio || t - self.inizio >= DURATA_GIRO_MS {
            return Esito::Ignorato;
        }
        if colpisce_sfera(self.occhio, d, self.bersaglio, zona::RAGGIO) {
            self.colpi += 1;
            self.somma_tempi += t - self.comparsa;
            self.nuovo_bersaglio(t, false);
            Esito::Colpo
        } else {
            self.errori += 1;
            Esito::Errore
        }
    }

    pub fn riepilogo(&self) -> Riepilogo {
        let clic = self.colpi + self.errori;
        Riepilogo {
            colpi: self.colpi,
            errori: self.errori,
            precisione: (clic > 0).then(|| f64::from(self.colpi) / f64::from(clic)),
            tempo_medio_ms: (self.colpi > 0).then(|| self.somma_tempi / f64::from(self.colpi)),
        }
    }

    /// Imbardata e beccheggio del bersaglio visto dall'occhio (per i test).
    #[cfg(test)]
    pub fn angoli_bersaglio(&self) -> (f64, f64) {
        self.angoli
    }

    fn nuovo_bersaglio(&mut self, t: f64, primo: bool) {
        let vecchia = avanti(self.angoli.0, self.angoli.1);
        let mut scelta = self.angoli;
        for _ in 0..64 {
            let imb = (self.caso.prossimo() * 2.0 - 1.0) * zona::IMBARDATA_MAX;
            let bec =
                zona::BECCHEGGIO_MIN + self.caso.prossimo() * (zona::BECCHEGGIO_MAX - zona::BECCHEGGIO_MIN);
            scelta = (imb, bec);
            if primo || avanti(imb, bec).dot(vecchia) <= zona::DISTANZA_MINIMA.cos() {
                break;
            }
        }
        self.angoli = scelta;
        self.bersaglio = self.occhio + avanti(scelta.0, scelta.1) * zona::DISTANZA;
        self.comparsa = t;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const OCCHIO: DVec3 = DVec3::new(0.0, 1.7, 0.0);

    fn giro(seme: u64) -> Giro {
        Giro::new(OCCHIO, Caso::new(seme))
    }

    fn mira(g: &Giro) -> DVec3 {
        (g.bersaglio - OCCHIO).normalize()
    }

    fn lontano(g: &Giro) -> DVec3 {
        -mira(g)
    }

    #[test]
    fn dura_60_secondi() {
        assert_eq!(DURATA_GIRO_MS, 60_000.0);
    }

    #[test]
    fn un_colpo_conta_e_il_bersaglio_si_sposta() {
        let mut g = giro(1);
        g.inizia(1000.0);
        let prima = g.bersaglio;
        assert_eq!(g.clic(1500.0, mira(&g)), Esito::Colpo);
        assert_eq!(g.colpi, 1);
        assert_ne!(g.bersaglio, prima);
    }

    #[test]
    fn un_clic_a_vuoto_e_un_errore_e_il_bersaglio_resta() {
        let mut g = giro(2);
        g.inizia(0.0);
        let prima = g.bersaglio;
        assert_eq!(g.clic(100.0, lontano(&g)), Esito::Errore);
        assert_eq!(g.errori, 1);
        assert_eq!(g.bersaglio, prima);
    }

    #[test]
    fn bersagli_nella_zona_e_lontani_dal_precedente() {
        let mut g = giro(3);
        g.inizia(0.0);
        for i in 0..2000 {
            let (imb, bec) = g.angoli_bersaglio();
            assert!(imb.abs() <= zona::IMBARDATA_MAX + 1e-12);
            assert!((zona::BECCHEGGIO_MIN - 1e-12..=zona::BECCHEGGIO_MAX + 1e-12).contains(&bec));
            let prima = mira(&g);
            assert_eq!(g.clic(f64::from(i) + 1.0, prima), Esito::Colpo);
            let angolo = prima.dot(mira(&g)).min(1.0).acos();
            assert!(angolo >= zona::DISTANZA_MINIMA - 1e-9, "solo {angolo} rad dal precedente");
        }
    }

    #[test]
    fn finisce_a_60_secondi_e_dopo_i_clic_non_contano() {
        let mut g = giro(4);
        g.inizia(1000.0);
        assert!(!g.aggiorna(60_999.0));
        assert!(g.aggiorna(61_000.0));
        assert_eq!(g.clic(61_001.0, mira(&g)), Esito::Ignorato);
        assert_eq!(g.colpi, 0);
    }

    #[test]
    fn conta_l_istante_dell_evento() {
        let mut g = giro(5);
        g.inizia(0.0);
        assert_eq!(g.clic(59_999.0, mira(&g)), Esito::Colpo);
        assert_eq!(g.clic(60_000.0, mira(&g)), Esito::Ignorato);
        assert_eq!(g.clic(-1.0, mira(&g)), Esito::Ignorato);
    }

    #[test]
    fn riepilogo_coerente_con_i_clic() {
        let mut g = giro(6);
        g.inizia(0.0);
        g.clic(400.0, mira(&g)); // bersaglio 1 in 400 ms
        g.clic(500.0, lontano(&g));
        g.clic(900.0, mira(&g)); // bersaglio 2 in 500 ms
        g.clic(1000.0, lontano(&g));
        g.clic(1100.0, lontano(&g));
        g.clic(1500.0, mira(&g)); // bersaglio 3 in 600 ms
        g.aggiorna(60_000.0);
        let r = g.riepilogo();
        assert_eq!((r.colpi, r.errori), (3, 3));
        assert!((r.precisione.unwrap() - 0.5).abs() < 1e-12);
        assert!((r.tempo_medio_ms.unwrap() - 500.0).abs() < 1e-9);
    }

    #[test]
    fn senza_clic_niente_precisione_ne_tempo() {
        let mut g = giro(7);
        g.inizia(0.0);
        g.aggiorna(60_000.0);
        let r = g.riepilogo();
        assert_eq!(r.precisione, None);
        assert_eq!(r.tempo_medio_ms, None);
    }

    #[test]
    fn la_pausa_ferma_il_tempo() {
        let mut g = giro(8);
        g.inizia(0.0);
        g.pausa(10_000.0);
        assert_eq!(g.clic(20_000.0, mira(&g)), Esito::Ignorato);
        g.riprendi(40_000.0);
        assert_eq!(g.restante_ms(40_000.0), 50_000.0);
        assert!(!g.aggiorna(89_999.0));
        g.clic(40_500.0, mira(&g));
        assert!((g.riepilogo().tempo_medio_ms.unwrap() - 10_500.0).abs() < 1e-9);
        assert!(g.aggiorna(90_000.0));
    }

    #[test]
    fn rigiocare_azzera() {
        let mut g = giro(9);
        g.inizia(0.0);
        g.clic(100.0, mira(&g));
        g.clic(200.0, lontano(&g));
        g.aggiorna(60_000.0);
        g.inizia(70_000.0);
        assert_eq!((g.colpi, g.errori, g.finito), (0, 0, false));
        assert_eq!(g.restante_ms(70_000.0), 60_000.0);
    }

    #[test]
    fn uscire_a_meta() {
        let mut g = giro(10);
        g.inizia(0.0);
        g.annulla();
        assert!(!g.in_corso && !g.finito);
        assert_eq!(g.clic(10.0, mira(&g)), Esito::Ignorato);
    }

    #[test]
    fn il_bersaglio_si_colpisce_guardando_nei_suoi_angoli() {
        let mut g = giro(11);
        g.inizia(0.0);
        let (imb, bec) = g.angoli_bersaglio();
        assert_eq!(g.clic(1.0, avanti(imb, bec)), Esito::Colpo);
    }

    #[test]
    fn il_caso_sta_fra_0_e_1() {
        let mut c = Caso::new(0);
        for _ in 0..10_000 {
            let v = c.prossimo();
            assert!((0.0..1.0).contains(&v));
        }
    }
}
