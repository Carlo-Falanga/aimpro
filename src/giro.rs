//! Il giro di uno scenario: le sfere, il loro moto, i clic, il tracking, il conto alla rovescia e il tempo.
//! Logica pura: i tempi (millisecondi) arrivano da fuori, dall'istante degli eventi, così si prova nei test.
//! Nessuna allocazione: le sfere stanno in un vettore fisso di `MAX_SFERE`.
//!
//! Il tempo di gioco (`tg`) parte da 0 alla fine del conto alla rovescia e si ferma in pausa. Il moto di ogni sfera
//! è un tratto: angoli di partenza, velocità angolare e istante `t0`; la posizione a un istante si calcola da lì, con
//! il rimbalzo ai bordi della zona (`rifletti`). Lo shader fa lo stesso conto con gli stessi numeri, quindi il
//! buffer delle sfere si riscrive solo quando un tratto cambia (sfera colpita, cambio di direzione), non a ogni frame.

use glam::DVec3;

use crate::camera::{avanti, colpisce_sfera};
use crate::scenari::{MAX_SFERE, Regole, TipoMovimento};

/// Il conto alla rovescia prima del giro.
pub const CONTO_MS: f64 = 3000.0;

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
    /// Media fra un colpo e il precedente (o la partenza); con una sfera sola è il tempo dalla comparsa. `None`
    /// senza colpi.
    pub tempo_medio_ms: Option<f64>,
    /// Negli scenari di tracking: la frazione del giro con il mirino sulla sfera e il tasto premuto.
    pub tracking: Option<f64>,
}

impl Riepilogo {
    /// Il punteggio del giro: i colpi, o la percentuale del tracking al decimo, come si mostra (così un record
    /// nuovo non compare mai "pari al record").
    pub fn punteggio(&self) -> f64 {
        self.tracking.map_or(f64::from(self.colpi), |t| (t * 1000.0).round() / 10.0)
    }
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

/// `x` riportato in `[lo, hi]` come una palla che rimbalza ai bordi. Lo stesso conto sta in `scena.wgsl`.
pub fn rifletti(x: f64, lo: f64, hi: f64) -> f64 {
    let w = hi - lo;
    if w <= 0.0 {
        return lo;
    }
    let u = (x - lo).rem_euclid(2.0 * w);
    if u <= w { lo + u } else { lo + 2.0 * w - u }
}

/// Il moto di una sfera da `t0` (ms di gioco): angoli di partenza e velocità in radianti al secondo.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tratto {
    pub imbardata: f64,
    pub beccheggio: f64,
    pub v_imbardata: f64,
    pub v_beccheggio: f64,
    pub t0: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Sfera {
    tratto: Tratto,
    /// Quando cambia direzione (ms di gioco), negli scenari a moto casuale.
    cambio: f64,
    /// Quando compare (ms di gioco): dopo un colpo, negli scenari di reazione, passa un'attesa a caso.
    comparsa: f64,
}

#[derive(Clone, Debug)]
pub struct Giro {
    pub regole: Regole,
    pub colpi: u32,
    pub errori: u32,
    pub finito: bool,
    pub in_corso: bool,
    pub in_pausa: bool,
    occhio: DVec3,
    caso: Caso,
    sfere: [Sfera; MAX_SFERE],
    /// I centri all'ultimo `avanza`, e il suo tempo di gioco.
    centri: [DVec3; MAX_SFERE],
    tg: f64,
    /// All'ultimo `avanza` il conto alla rovescia era finito: prima le sfere non si vedono e non si guardano.
    partito: bool,
    /// Un bit per sfera: visibile all'ultimo `avanza` (una sfera colpita lo perde e lo riprende quando ricompare).
    visibili: u8,
    /// Quante volte una sfera è comparsa da `inizia`: il suono di comparsa parte quando cambia.
    comparse: u32,
    /// Cresce a ogni tratto nuovo: chi disegna riscrive il buffer delle sfere solo quando cambia.
    pub versione: u64,
    /// Istante (ms) in cui parte il tempo di gioco; durante il conto alla rovescia è nel futuro. Si sposta in avanti
    /// a ogni ripresa dalla pausa.
    inizio: f64,
    inizio_pausa: f64,
    /// Tempo di gioco dell'ultimo colpo (0 alla partenza).
    ultimo_colpo: f64,
    somma_tempi: f64,
    /// Il centro della sfera dell'ultimo colpo, all'istante del colpo.
    colpita: DVec3,
    /// Il tasto sinistro è giù (dal Raw Input, giù e su).
    premuto: bool,
    /// Fin dove è arrivato il conto del tracking (ms, come gli istanti degli eventi).
    tracciato: f64,
    sul_bersaglio_ms: f64,
}

impl Giro {
    pub fn new(occhio: DVec3, caso: Caso, regole: Regole) -> Self {
        Giro {
            regole,
            colpi: 0,
            errori: 0,
            finito: false,
            in_corso: false,
            in_pausa: false,
            occhio,
            caso,
            sfere: [Sfera::default(); MAX_SFERE],
            centri: [occhio + DVec3::NEG_Z * regole.distanza; MAX_SFERE],
            tg: 0.0,
            partito: false,
            visibili: 0,
            comparse: 0,
            versione: 0,
            inizio: 0.0,
            inizio_pausa: 0.0,
            ultimo_colpo: 0.0,
            somma_tempi: 0.0,
            colpita: occhio,
            premuto: false,
            tracciato: 0.0,
            sul_bersaglio_ms: 0.0,
        }
    }

    pub fn durata_ms(&self) -> f64 {
        self.regole.durata_ms
    }

    #[cfg(test)]
    pub fn raggio(&self) -> f64 {
        self.regole.raggio
    }

    pub fn n_sfere(&self) -> usize {
        self.regole.sfere
    }

    #[cfg(test)]
    pub fn centro(&self, i: usize) -> DVec3 {
        self.centri[i]
    }

    pub fn tratto(&self, i: usize) -> Tratto {
        self.sfere[i].tratto
    }

    /// Quando compare la sfera `i` (ms di gioco).
    pub fn comparsa(&self, i: usize) -> f64 {
        self.sfere[i].comparsa
    }

    /// La sfera `i` si vede, all'ultimo `avanza`?
    pub fn visibile(&self, i: usize) -> bool {
        self.partito && self.tg >= self.sfere[i].comparsa
    }

    /// Il giro parte con il conto alla rovescia: il tempo di gioco comincia fra `CONTO_MS`.
    pub fn inizia(&mut self, t: f64) {
        self.colpi = 0;
        self.errori = 0;
        self.somma_tempi = 0.0;
        self.ultimo_colpo = 0.0;
        self.sul_bersaglio_ms = 0.0;
        self.finito = false;
        self.in_corso = true;
        self.in_pausa = false;
        self.inizio = t + CONTO_MS;
        self.partito = false;
        self.visibili = 0;
        self.comparse = 0;
        self.tracciato = t;
        self.colpita = self.occhio;
        for i in 0..self.regole.sfere {
            self.nuova_sfera(i, 0.0, true);
        }
        self.avanza(t);
    }

    /// Il conto alla rovescia è in corso all'istante `t`?
    pub fn nel_conto(&self, t: f64) -> bool {
        self.in_corso && !self.in_pausa && t < self.inizio
    }

    /// I secondi del conto alla rovescia da mostrare (3, 2, 1), se è in corso.
    pub fn conto(&self, t: f64) -> Option<u32> {
        let adesso = if self.in_pausa { self.inizio_pausa } else { t };
        (self.in_corso && adesso < self.inizio)
            .then(|| ((self.inizio - adesso) / 1000.0).ceil().max(1.0) as u32)
    }

    /// Salta il conto alla rovescia: il giro parte adesso.
    pub fn salta_conto(&mut self, t: f64) {
        if self.nel_conto(t) {
            self.inizio = t;
            self.avanza(t);
        }
    }

    /// Il tempo di gioco all'istante `t` (fermo in pausa, 0 durante il conto alla rovescia).
    pub fn tempo_di_gioco(&self, t: f64) -> f64 {
        let adesso = if self.in_pausa { self.inizio_pausa } else { t };
        (adesso - self.inizio).clamp(0.0, self.regole.durata_ms)
    }

    pub fn restante_ms(&self, t: f64) -> f64 {
        if !self.in_corso && !self.finito {
            return self.regole.durata_ms;
        }
        self.regole.durata_ms - self.tempo_di_gioco(t)
    }

    /// Va chiamata a ogni frame: dice `true` quando il giro è finito.
    pub fn aggiorna(&mut self, t: f64) -> bool {
        if self.in_corso && !self.in_pausa && t - self.inizio >= self.regole.durata_ms {
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
            self.inizio += t - self.inizio_pausa;
            self.in_pausa = false;
            // In pausa il Raw Input è spento: lo stato del tasto non si sa, si riparte col tasto su.
            self.premuto = false;
            self.tracciato = t;
        }
    }

    /// Lascia il giro a metà: niente riepilogo.
    pub fn annulla(&mut self) {
        self.in_corso = false;
        self.in_pausa = false;
        self.finito = false;
    }

    /// Porta il moto delle sfere all'istante `t` (cambi di direzione compresi) e ne calcola i centri.
    pub fn avanza(&mut self, t: f64) {
        let tg = self.tempo_di_gioco(t);
        if self.regole.movimento == TipoMovimento::Casuale {
            for i in 0..self.regole.sfere {
                // Al più qualche cambio per frame; un frame lunghissimo non fa girare il ciclo all'infinito.
                for _ in 0..16 {
                    if tg < self.sfere[i].cambio {
                        break;
                    }
                    self.cambia_direzione(i, self.sfere[i].cambio);
                }
            }
        }
        for i in 0..self.regole.sfere {
            self.centri[i] = self.centro_a(i, tg);
        }
        self.tg = tg;
        self.partito = self.in_corso && t >= self.inizio;
        for i in 0..self.regole.sfere {
            let bit = 1u8 << i;
            if self.visibile(i) && self.visibili & bit == 0 {
                self.comparse = self.comparse.wrapping_add(1);
            }
            self.visibili = if self.visibile(i) { self.visibili | bit } else { self.visibili & !bit };
        }
    }

    /// Gli angoli della sfera `i` al tempo di gioco `tg`.
    fn angoli_a(&self, i: usize, tg: f64) -> (f64, f64) {
        let m = self.sfere[i].tratto;
        let r = &self.regole;
        let dt = (tg - m.t0) / 1000.0;
        (
            rifletti(m.imbardata + m.v_imbardata * dt, -r.imbardata_max, r.imbardata_max),
            rifletti(m.beccheggio + m.v_beccheggio * dt, r.beccheggio_min, r.beccheggio_max),
        )
    }

    fn centro_a(&self, i: usize, tg: f64) -> DVec3 {
        let (imb, bec) = self.angoli_a(i, tg);
        self.occhio + avanti(imb, bec) * self.regole.distanza
    }

    /// La sfera che il raggio dall'occhio in direzione `d` tocca, con i centri dell'ultimo `avanza`; se ne tocca più
    /// d'una, quella col centro più vicino alla direzione.
    pub fn guarda(&self, d: DVec3) -> Option<usize> {
        let mut scelta = None;
        let mut migliore = f64::NEG_INFINITY;
        for i in 0..self.regole.sfere {
            let c = self.centri[i];
            if self.visibile(i) && colpisce_sfera(self.occhio, d, c, self.regole.raggio) {
                let coseno = (c - self.occhio).normalize_or_zero().dot(d);
                if coseno > migliore {
                    migliore = coseno;
                    scelta = Some(i);
                }
            }
        }
        scelta
    }

    /// Quante volte una sfera è comparsa (diventata visibile) da `inizia`.
    pub fn comparse(&self) -> u32 {
        self.comparse
    }

    /// Il centro della sfera dell'ultimo colpo, all'istante del colpo.
    pub fn colpita(&self) -> DVec3 {
        self.colpita
    }

    /// Il tasto sinistro all'istante `t` (giù o su), guardando nella direzione `d`. Il tasto giù è un clic; durante il
    /// conto alla rovescia lo salta e non conta.
    pub fn tasto(&mut self, t: f64, giu: bool, d: DVec3) -> Esito {
        self.traccia(t, d);
        self.premuto = giu;
        if !giu {
            return Esito::Ignorato;
        }
        if self.nel_conto(t) {
            self.salta_conto(t);
            return Esito::Ignorato;
        }
        self.clic(t, d)
    }

    /// Un clic all'istante `t` (quello dell'evento) guardando nella direzione `d` (versore), con le sfere dove sono a
    /// quell'istante. Negli scenari di tracking i clic non contano.
    pub fn clic(&mut self, t: f64, d: DVec3) -> Esito {
        if !self.in_corso
            || self.in_pausa
            || self.regole.tracking()
            || t < self.inizio
            || t - self.inizio >= self.regole.durata_ms
        {
            return Esito::Ignorato;
        }
        self.avanza(t);
        let tg = self.tempo_di_gioco(t);
        match self.guarda(d) {
            Some(i) => {
                self.colpi += 1;
                // Dal colpo prima o, se è comparsa dopo, dalla comparsa: con l'attesa è il tempo di reazione.
                self.somma_tempi += tg - self.ultimo_colpo.max(self.sfere[i].comparsa);
                self.ultimo_colpo = tg;
                self.colpita = self.centri[i];
                self.nuova_sfera(i, tg, false);
                // La sfera nuova è un'altra comparsa, anche se si vede subito.
                self.visibili &= !(1u8 << i);
                self.centri[i] = self.centro_a(i, tg);
                Esito::Colpo
            }
            None => {
                self.errori += 1;
                Esito::Errore
            }
        }
    }

    /// Il tracking fino all'istante `t`, con il mirino in direzione `d`: il tratto da dove era arrivato conta se il
    /// tasto era giù e, a `t`, il mirino è sulla sfera. Si chiama a ogni evento del tasto e a ogni frame.
    pub fn traccia(&mut self, t: f64, d: DVec3) {
        if !self.regole.tracking() || !self.in_corso || self.in_pausa {
            return;
        }
        let da = self.tracciato.max(self.inizio);
        let a = t.min(self.inizio + self.regole.durata_ms);
        if self.premuto && a > da {
            self.avanza(a);
            if self.guarda(d).is_some() {
                self.sul_bersaglio_ms += a - da;
            }
        }
        self.tracciato = self.tracciato.max(t);
    }

    /// Punteggio e precisione dal vivo, in interi per il disegno: i colpi e la precisione in percento (senza clic,
    /// `None`); nel tracking la percentuale del tempo giocato finora con il mirino sulla sfera, e niente precisione.
    pub fn dal_vivo(&self, t: f64) -> (u32, Option<u32>) {
        if self.regole.tracking() {
            let giocato = self.tempo_di_gioco(t);
            let p = if giocato > 0.0 { (self.sul_bersaglio_ms / giocato).clamp(0.0, 1.0) } else { 0.0 };
            return ((p * 100.0).round() as u32, None);
        }
        let clic = self.colpi + self.errori;
        let precisione = (clic > 0).then(|| (f64::from(self.colpi) * 100.0 / f64::from(clic)).round() as u32);
        (self.colpi, precisione)
    }

    pub fn riepilogo(&self) -> Riepilogo {
        let clic = self.colpi + self.errori;
        let tracking = self.regole.tracking();
        Riepilogo {
            colpi: self.colpi,
            errori: self.errori,
            precisione: (!tracking && clic > 0).then(|| f64::from(self.colpi) / f64::from(clic)),
            tempo_medio_ms: (self.colpi > 0).then(|| self.somma_tempi / f64::from(self.colpi)),
            tracking: tracking.then(|| (self.sul_bersaglio_ms / self.regole.durata_ms).clamp(0.0, 1.0)),
        }
    }

    /// Una sfera nuova al posto della `i`, lontana dalle altre e (se non è la prima) da dove era; con il suo moto.
    fn nuova_sfera(&mut self, i: usize, tg: f64, primo: bool) {
        let r = self.regole;
        let vecchia = self.angoli_a(i, tg);
        let mut scelta = vecchia;
        for _ in 0..64 {
            let imb = (self.caso.prossimo() * 2.0 - 1.0) * r.imbardata_max;
            let bec = r.beccheggio_min + self.caso.prossimo() * (r.beccheggio_max - r.beccheggio_min);
            scelta = (imb, bec);
            let dir = avanti(imb, bec);
            let minimo = r.distanza_minima.cos();
            let lontana_da_dove_era = primo || dir.dot(avanti(vecchia.0, vecchia.1)) <= minimo;
            // Le altre sfere: quelle già messe (alla partenza si mettono in ordine).
            let altre = if primo { i } else { r.sfere };
            let lontana_dalle_altre = (0..altre).filter(|&j| j != i).all(|j| {
                let (a, b) = self.angoli_a(j, tg);
                dir.dot(avanti(a, b)) <= minimo
            });
            if lontana_da_dove_era && lontana_dalle_altre {
                break;
            }
        }
        let comparsa = if r.ritardo_max_ms > 0.0 {
            tg + r.ritardo_min_ms + self.caso.prossimo() * (r.ritardo_max_ms - r.ritardo_min_ms)
        } else {
            tg
        };
        let mut tratto =
            Tratto { imbardata: scelta.0, beccheggio: scelta.1, t0: comparsa, ..Tratto::default() };
        if r.movimento != TipoMovimento::Fermo {
            let direzione = self.caso.prossimo() * std::f64::consts::TAU;
            (tratto.v_imbardata, tratto.v_beccheggio) = self.velocita_verso(direzione);
        }
        self.sfere[i].tratto = tratto;
        self.sfere[i].comparsa = comparsa;
        self.sfere[i].cambio = comparsa + self.intervallo_cambio();
        self.versione += 1;
    }

    /// La velocità dello scenario nella `direzione` (radianti, 0 = verso destra). Se la zona è senza altezza (o senza
    /// larghezza) tutta la velocità va sull'altro asse: altrimenti la parte persa contro il bordo farebbe una sfera più
    /// lenta a caso, e un record che dipende dalla fortuna.
    fn velocita_verso(&self, direzione: f64) -> (f64, f64) {
        let r = self.regole;
        let segno = |x: f64| if x < 0.0 { -1.0 } else { 1.0 };
        if r.beccheggio_max - r.beccheggio_min < 1e-9 {
            (r.velocita * segno(direzione.cos()), 0.0)
        } else if r.imbardata_max < 1e-9 {
            (0.0, r.velocita * segno(direzione.sin()))
        } else {
            (r.velocita * direzione.cos(), r.velocita * direzione.sin())
        }
    }

    fn intervallo_cambio(&mut self) -> f64 {
        let r = self.regole;
        if r.movimento != TipoMovimento::Casuale {
            return f64::INFINITY;
        }
        r.cambio_min_ms + self.caso.prossimo() * (r.cambio_max_ms - r.cambio_min_ms)
    }

    /// Al tempo di gioco `tg` la sfera `i` gira di colpo: fra 60 e 300 gradi rispetto a dove stava andando.
    fn cambia_direzione(&mut self, i: usize, tg: f64) {
        let (imb, bec) = self.angoli_a(i, tg);
        // La direzione in cui andava davvero, dopo i rimbalzi: la si ricava da due istanti vicini.
        let (imb2, bec2) = self.angoli_a(i, tg + 1.0);
        let attuale = (bec2 - bec).atan2(imb2 - imb);
        let svolta = (60.0 + self.caso.prossimo() * 240.0).to_radians();
        let (v_imbardata, v_beccheggio) = self.velocita_verso(attuale + svolta);
        self.sfere[i].tratto = Tratto { imbardata: imb, beccheggio: bec, v_imbardata, v_beccheggio, t0: tg };
        self.sfere[i].cambio = tg + self.intervallo_cambio();
        self.versione += 1;
    }

    /// Imbardata e beccheggio della sfera `i` visti dall'occhio, all'ultimo `avanza` (per i test).
    #[cfg(test)]
    pub fn angoli_sfera(&self, i: usize) -> (f64, f64) {
        let d = (self.centri[i] - self.occhio).normalize();
        (d.x.atan2(-d.z), d.y.asin())
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::scenari::{Parametri, Regole, di_aimpro};

    pub const OCCHIO: DVec3 = DVec3::new(0.0, 1.7, 0.0);

    pub fn regole(id: &str) -> Regole {
        di_aimpro().into_iter().find(|s| s.id == id).unwrap().regole
    }

    /// Un giro dello scenario `id` partito a `t`, senza conto alla rovescia.
    pub fn partito(id: &str, seme: u64, t: f64) -> Giro {
        let mut g = Giro::new(OCCHIO, Caso::new(seme), regole(id));
        g.inizia(t - CONTO_MS);
        g.avanza(t);
        g
    }

    fn giro(seme: u64) -> Giro {
        partito("scatto", seme, 0.0)
    }

    pub fn mira_a(g: &Giro, i: usize) -> DVec3 {
        (g.centro(i) - OCCHIO).normalize()
    }

    fn mira(g: &Giro) -> DVec3 {
        mira_a(g, 0)
    }

    fn lontano(g: &Giro) -> DVec3 {
        -mira(g)
    }

    #[test]
    fn dura_30_secondi() {
        assert_eq!(giro(1).durata_ms(), 30_000.0);
    }

    #[test]
    fn un_colpo_conta_e_il_bersaglio_si_sposta() {
        let mut g = partito("scatto", 1, 1000.0);
        let prima = g.centro(0);
        assert_eq!(g.clic(1500.0, mira(&g)), Esito::Colpo);
        assert_eq!(g.colpi, 1);
        assert_ne!(g.centro(0), prima);
        assert_eq!(g.colpita(), prima);
    }

    #[test]
    fn un_clic_a_vuoto_e_un_errore_e_il_bersaglio_resta() {
        let mut g = giro(2);
        let prima = g.centro(0);
        assert_eq!(g.clic(100.0, lontano(&g)), Esito::Errore);
        assert_eq!(g.errori, 1);
        assert_eq!(g.centro(0), prima);
    }

    #[test]
    fn bersagli_nella_zona_e_lontani_dal_precedente() {
        let mut g = giro(3);
        let r = g.regole;
        for i in 0..2000 {
            let (imb, bec) = g.angoli_sfera(0);
            assert!(imb.abs() <= r.imbardata_max + 1e-9);
            assert!((r.beccheggio_min - 1e-9..=r.beccheggio_max + 1e-9).contains(&bec));
            let prima = mira(&g);
            assert_eq!(g.clic(f64::from(i) + 1.0, prima), Esito::Colpo);
            let angolo = prima.dot(mira(&g)).min(1.0).acos();
            assert!(angolo >= r.distanza_minima - 1e-9, "solo {angolo} rad dal precedente");
        }
    }

    #[test]
    fn finisce_a_30_secondi_e_dopo_i_clic_non_contano() {
        let mut g = partito("scatto", 4, 1000.0);
        assert!(!g.aggiorna(30_999.0));
        assert!(g.aggiorna(31_000.0));
        assert_eq!(g.clic(31_001.0, mira(&g)), Esito::Ignorato);
        assert_eq!(g.colpi, 0);
    }

    #[test]
    fn conta_l_istante_dell_evento() {
        let mut g = giro(5);
        assert_eq!(g.clic(29_999.0, mira(&g)), Esito::Colpo);
        assert_eq!(g.clic(30_000.0, mira(&g)), Esito::Ignorato);
        assert_eq!(g.clic(-1.0, mira(&g)), Esito::Ignorato);
    }

    #[test]
    fn riepilogo_coerente_con_i_clic() {
        let mut g = giro(6);
        g.clic(400.0, mira(&g)); // bersaglio 1 in 400 ms
        g.clic(500.0, lontano(&g));
        g.clic(900.0, mira(&g)); // bersaglio 2 in 500 ms
        g.clic(1000.0, lontano(&g));
        g.clic(1100.0, lontano(&g));
        g.clic(1500.0, mira(&g)); // bersaglio 3 in 600 ms
        g.aggiorna(30_000.0);
        let r = g.riepilogo();
        assert_eq!((r.colpi, r.errori), (3, 3));
        assert!((r.precisione.unwrap() - 0.5).abs() < 1e-12);
        assert!((r.tempo_medio_ms.unwrap() - 500.0).abs() < 1e-9);
        assert_eq!((r.tracking, r.punteggio()), (None, 3.0));
    }

    #[test]
    fn senza_clic_niente_precisione_ne_tempo() {
        let mut g = giro(7);
        g.aggiorna(30_000.0);
        let r = g.riepilogo();
        assert_eq!(r.precisione, None);
        assert_eq!(r.tempo_medio_ms, None);
    }

    #[test]
    fn la_pausa_ferma_il_tempo() {
        let mut g = giro(8);
        g.pausa(10_000.0);
        assert_eq!(g.clic(20_000.0, mira(&g)), Esito::Ignorato);
        g.riprendi(40_000.0);
        assert_eq!(g.restante_ms(40_000.0), 20_000.0);
        assert!(!g.aggiorna(59_999.0));
        g.clic(40_500.0, mira(&g));
        assert!((g.riepilogo().tempo_medio_ms.unwrap() - 10_500.0).abs() < 1e-9);
        assert!(g.aggiorna(60_000.0));
    }

    #[test]
    fn rigiocare_azzera() {
        let mut g = giro(9);
        g.clic(100.0, mira(&g));
        g.clic(200.0, lontano(&g));
        g.aggiorna(30_000.0);
        g.inizia(70_000.0);
        assert_eq!((g.colpi, g.errori, g.finito), (0, 0, false));
        assert_eq!(g.restante_ms(70_000.0), 30_000.0);
    }

    #[test]
    fn uscire_a_meta() {
        let mut g = giro(10);
        g.annulla();
        assert!(!g.in_corso && !g.finito);
        assert_eq!(g.clic(10.0, mira(&g)), Esito::Ignorato);
    }

    #[test]
    fn il_bersaglio_si_colpisce_guardando_nei_suoi_angoli() {
        let mut g = giro(11);
        let (imb, bec) = g.angoli_sfera(0);
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

    // ------------------------------------------------------------ conto alla rovescia

    #[test]
    fn il_conto_alla_rovescia_dura_3_secondi_e_poi_parte_il_giro() {
        let mut g = Giro::new(OCCHIO, Caso::new(12), regole("scatto"));
        g.inizia(1000.0);
        assert_eq!(g.conto(1000.0), Some(3));
        assert_eq!(g.conto(2500.0), Some(2));
        assert_eq!(g.conto(3999.0), Some(1));
        assert_eq!(g.conto(4000.0), None);
        assert_eq!(g.restante_ms(2000.0), 30_000.0, "il tempo del giro non scorre durante il conto");
        assert!(!g.aggiorna(33_999.0));
        assert!(g.aggiorna(34_000.0));
    }

    #[test]
    fn un_clic_salta_il_conto_e_non_conta() {
        let mut g = Giro::new(OCCHIO, Caso::new(13), regole("scatto"));
        g.inizia(0.0);
        let d = mira(&g);
        assert_eq!(g.tasto(500.0, true, d), Esito::Ignorato, "il clic che salta il conto non è un colpo");
        assert_eq!(g.conto(500.0), None);
        assert_eq!((g.colpi, g.errori), (0, 0));
        assert_eq!(g.restante_ms(500.0), 30_000.0);
        g.tasto(510.0, false, d);
        assert_eq!(g.tasto(600.0, true, d), Esito::Colpo, "dopo, i clic contano");
        assert!(g.aggiorna(30_500.0));
    }

    #[test]
    fn saltare_il_conto_da_tastiera() {
        let mut g = Giro::new(OCCHIO, Caso::new(14), regole("scatto"));
        g.inizia(0.0);
        g.salta_conto(1200.0);
        assert_eq!(g.conto(1200.0), None);
        assert_eq!(g.clic(1300.0, mira(&g)), Esito::Colpo);
        assert!(g.aggiorna(31_200.0));
    }

    #[test]
    fn la_pausa_nel_conto_lo_ferma() {
        let mut g = Giro::new(OCCHIO, Caso::new(15), regole("scatto"));
        g.inizia(0.0);
        g.pausa(1000.0);
        assert_eq!(g.conto(5000.0), Some(2));
        g.riprendi(9000.0);
        assert_eq!(g.conto(9000.0), Some(2));
        assert_eq!(g.conto(11_000.0), None);
    }

    // ------------------------------------------------------------ moto

    #[test]
    fn rifletti_rimbalza_ai_bordi() {
        assert_eq!(rifletti(0.5, 0.0, 1.0), 0.5);
        assert!((rifletti(1.25, 0.0, 1.0) - 0.75).abs() < 1e-12);
        assert!((rifletti(-0.25, 0.0, 1.0) - 0.25).abs() < 1e-12);
        assert!((rifletti(2.5, 0.0, 1.0) - 0.5).abs() < 1e-12);
        assert!((rifletti(-3.0, -1.0, 1.0) - 1.0).abs() < 1e-12);
        assert_eq!(rifletti(5.0, 2.0, 2.0), 2.0);
    }

    fn velocita_angolare(g: &mut Giro, i: usize, t: f64) -> f64 {
        g.avanza(t);
        let a = mira_a(g, i);
        g.avanza(t + 10.0);
        let b = mira_a(g, i);
        a.dot(b).clamp(-1.0, 1.0).acos() / 0.010
    }

    #[test]
    fn nelle_zone_e_alla_velocita_dello_scenario() {
        for id in ["deriva", "filo", "guizzo", "sciame"] {
            let mut g = partito(id, 16, 0.0);
            let r = g.regole;
            for passo in 0..3000 {
                let t = f64::from(passo) * 10.0;
                g.avanza(t);
                for i in 0..r.sfere {
                    let (imb, bec) = g.angoli_sfera(i);
                    assert!(imb.abs() <= r.imbardata_max + 1e-9, "{id}: imbardata {imb}");
                    assert!(
                        (r.beccheggio_min - 1e-9..=r.beccheggio_max + 1e-9).contains(&bec),
                        "{id}: {bec}"
                    );
                }
            }
            // Lontano dai rimbalzi la velocità sullo schermo è quella dello scenario (a meno del coseno del
            // beccheggio, piccolo nella zona).
            let v = velocita_angolare(&mut g, 0, 5.0);
            assert!(v <= r.velocita * 1.01 && v >= r.velocita * 0.9, "{id}: {v} contro {}", r.velocita);
        }
    }

    #[test]
    fn le_sfere_ferme_non_si_muovono() {
        let mut g = partito("terna", 17, 0.0);
        let prima: Vec<DVec3> = (0..3).map(|i| g.centro(i)).collect();
        g.avanza(20_000.0);
        assert_eq!((0..3).map(|i| g.centro(i)).collect::<Vec<_>>(), prima);
        assert_eq!(g.versione, 3, "un tratto per sfera, nessun cambio");
    }

    #[test]
    fn il_moto_casuale_cambia_direzione_agli_intervalli_dello_scenario() {
        let mut g = partito("guizzo", 18, 0.0);
        let r = g.regole;
        let mut cambi = 0;
        let mut v = g.versione;
        let mut direzione_prima = None;
        for passo in 1..=30_000 {
            g.avanza(f64::from(passo));
            if g.versione != v {
                v = g.versione;
                cambi += 1;
                let m = g.tratto(0);
                let d = m.v_beccheggio.atan2(m.v_imbardata);
                direzione_prima = Some(d);
            }
        }
        assert!(direzione_prima.is_some());
        let attesi = r.durata_ms / ((r.cambio_min_ms + r.cambio_max_ms) / 2.0);
        assert!(
            (attesi * 0.7..attesi * 1.3).contains(&f64::from(cambi)),
            "{cambi} cambi, attesi circa {attesi}"
        );
    }

    #[test]
    fn il_moto_e_continuo_anche_ai_cambi_di_direzione() {
        let mut g = partito("guizzo", 19, 0.0);
        let mut prima = mira_a(&g, 0);
        let massimo = g.regole.velocita * 0.001 * 1.05;
        for passo in 1..=30_000 {
            g.avanza(f64::from(passo));
            let ora = mira_a(&g, 0);
            let salto = prima.dot(ora).clamp(-1.0, 1.0).acos();
            assert!(salto <= massimo, "salto di {salto} rad al ms {passo}");
            prima = ora;
        }
    }

    #[test]
    fn la_velocita_segue_il_parametro() {
        let base = regole("deriva");
        let mut g = Giro::new(OCCHIO, Caso::new(20), base.con(Parametri { dimensione: 1.0, velocita: 2.0 }));
        g.inizia(-CONTO_MS);
        let v = velocita_angolare(&mut g, 0, 5.0);
        assert!((v / base.velocita - 2.0).abs() < 0.2, "{v}");
    }

    // ------------------------------------------------------------ più sfere

    #[test]
    fn tre_sfere_lontane_fra_loro() {
        for seme in 0..50 {
            let g = partito("terna", seme, 0.0);
            let min = g.regole.distanza_minima;
            for a in 0..3 {
                for b in (a + 1)..3 {
                    let angolo = mira_a(&g, a).dot(mira_a(&g, b)).clamp(-1.0, 1.0).acos();
                    assert!(angolo >= min - 1e-9, "seme {seme}: {a} e {b} a {angolo} rad");
                }
            }
        }
    }

    #[test]
    fn con_tre_sfere_ferme_il_clic_colpisce_quella_guardata() {
        let mut g = partito("terna", 21, 0.0);
        for k in 0..300 {
            let i = k % 3;
            let altre: Vec<DVec3> = (0..3).filter(|&j| j != i).map(|j| g.centro(j)).collect();
            let prima = g.centro(i);
            assert_eq!(g.clic(f64::from(k as u32) * 10.0 + 1.0, mira_a(&g, i)), Esito::Colpo);
            assert_eq!(g.colpita(), prima, "colpita quella guardata");
            assert_ne!(g.centro(i), prima, "rinasce altrove");
            let ora: Vec<DVec3> = (0..3).filter(|&j| j != i).map(|j| g.centro(j)).collect();
            assert_eq!(ora, altre, "le altre restano dove sono");
            let min = g.regole.distanza_minima;
            for j in (0..3).filter(|&j| j != i) {
                let angolo = mira_a(&g, i).dot(mira_a(&g, j)).clamp(-1.0, 1.0).acos();
                assert!(angolo >= min - 1e-9);
            }
        }
        assert_eq!(g.colpi, 300);
    }

    #[test]
    fn con_tre_sfere_in_movimento_conta_la_visuale_dell_istante_del_clic() {
        for id in ["sciame", "stormo"] {
            let mut g = partito(id, 22, 0.0);
            let r = g.regole.raggio / g.regole.distanza;
            // L'ultimo frame è a 1000 ms; il clic arriva a 1600 ms, con la mira dove una sfera è in quell'istante.
            g.avanza(1000.0);
            let mut futuro = g.clone();
            futuro.avanza(1600.0);
            // La sfera che fra il frame e il clic si è spostata di più: oltre due raggi, così con le posizioni del
            // frame il clic sarebbe a vuoto.
            let spostamento = |i: usize| mira_a(&g, i).dot(mira_a(&futuro, i)).clamp(-1.0, 1.0).acos();
            let i = (0..3).max_by(|a, b| spostamento(*a).total_cmp(&spostamento(*b))).unwrap();
            assert!(spostamento(i) > 2.0 * r, "{id}: la sfera si è mossa poco");
            let al_clic = mira_a(&futuro, i);
            assert_eq!(g.guarda(al_clic), None, "{id}: alle posizioni del frame è a vuoto");
            assert_eq!(g.clic(1600.0, al_clic), Esito::Colpo, "{id}");
            assert_eq!(g.colpita(), futuro.centro(i), "{id}");
        }
    }

    #[test]
    fn nel_binario_la_sfera_va_sempre_alla_velocita_dello_scenario() {
        // La zona di binario non ha altezza: tutta la velocità va in orizzontale, con ogni seme.
        for seme in 1..=40 {
            let mut g = partito("binario", seme, 0.0);
            let r = g.regole;
            assert!(r.beccheggio_max - r.beccheggio_min < 1e-9);
            let v = velocita_angolare(&mut g, 0, 2000.0);
            assert!(
                v >= r.velocita * 0.95 && v <= r.velocita * 1.01,
                "seme {seme}: {v} contro {}",
                r.velocita
            );
        }
    }

    #[test]
    fn nel_conto_alla_rovescia_le_sfere_non_si_guardano() {
        let mut g = Giro::new(OCCHIO, Caso::new(9), regole("scatto"));
        g.inizia(0.0);
        g.avanza(1000.0);
        let d = mira_a(&g, 0);
        assert_eq!(g.guarda(d), None, "nel conto la sfera non c'è");
        assert!(!g.visibile(0));
        g.avanza(CONTO_MS + 1.0);
        assert_eq!(g.guarda(d), Some(0));
    }

    #[test]
    fn il_punteggio_del_tracking_e_al_decimo_come_si_mostra() {
        let r = |t: f64| Riepilogo {
            colpi: 0,
            errori: 0,
            precisione: None,
            tempo_medio_ms: None,
            tracking: Some(t),
        };
        assert_eq!(r(0.70004).punteggio(), 70.0);
        assert_eq!(r(0.70051).punteggio(), 70.1);
        assert_eq!(r(1.0).punteggio(), 100.0);
    }

    #[test]
    fn il_punteggio_dal_vivo_segue_colpi_errori_e_tracking() {
        let mut g = partito("scatto", 3, 0.0);
        assert_eq!(g.dal_vivo(0.0), (0, None), "senza clic niente precisione");
        assert_eq!(g.clic(100.0, mira_a(&g, 0)), Esito::Colpo);
        assert_eq!(g.clic(200.0, mira_a(&g, 0)), Esito::Colpo);
        assert_eq!(g.clic(300.0, -mira_a(&g, 0)), Esito::Errore);
        assert_eq!(g.dal_vivo(300.0), (2, Some(67)));

        let mut g = partito("filo", 5, 0.0);
        g.tasto(0.0, true, mira_a(&g, 0));
        for ms in 1..=1000 {
            let t = f64::from(ms);
            g.avanza(t);
            let d = if ms <= 500 { mira_a(&g, 0) } else { -mira_a(&g, 0) };
            g.traccia(t, d);
        }
        assert_eq!(g.dal_vivo(1000.0), (50, None), "metà del tempo giocato finora");
    }

    #[test]
    fn la_dimensione_cambia_il_raggio() {
        let base = regole("scatto");
        let g = Giro::new(OCCHIO, Caso::new(23), base.con(Parametri { dimensione: 1.5, velocita: 1.0 }));
        assert!((g.raggio() - 0.6).abs() < 1e-12);
    }

    #[test]
    fn nella_reazione_la_sfera_compare_dopo_un_attesa_e_conta_da_li() {
        let mut g = Giro::new(OCCHIO, Caso::new(28), regole("riflesso"));
        let r = g.regole;
        g.inizia(-CONTO_MS);
        g.avanza(0.0);
        let attesa = g.comparsa(0);
        assert!((r.ritardo_min_ms..=r.ritardo_max_ms).contains(&attesa), "{attesa}");
        assert!(!g.visibile(0));
        // Prima che compaia, mirarla non è un colpo.
        assert_eq!(g.clic(attesa - 10.0, mira_a(&g, 0)), Esito::Errore);
        g.avanza(attesa + 250.0);
        assert!(g.visibile(0));
        assert_eq!(g.clic(attesa + 250.0, mira_a(&g, 0)), Esito::Colpo);
        // Il tempo è dalla comparsa al colpo, non dalla partenza.
        assert!((g.riepilogo().tempo_medio_ms.unwrap() - 250.0).abs() < 1e-9);
        let dopo = g.comparsa(0) - (attesa + 250.0);
        assert!((r.ritardo_min_ms..=r.ritardo_max_ms).contains(&dopo), "{dopo}");
    }

    #[test]
    fn le_comparse_si_contano_quando_la_sfera_si_vede() {
        // Nel conto alla rovescia nessuna comparsa; alla partenza una per sfera.
        let mut g = Giro::new(OCCHIO, Caso::new(29), regole("terna"));
        g.inizia(0.0);
        g.avanza(CONTO_MS - 1.0);
        assert_eq!(g.comparse(), 0);
        g.avanza(CONTO_MS);
        assert_eq!(g.comparse(), 3);
        g.avanza(CONTO_MS + 16.0);
        assert_eq!(g.comparse(), 3, "una sfera che resta visibile non ricompare");
        // Un colpo con la sfera nuova subito visibile: una comparsa in più al frame dopo.
        assert_eq!(g.clic(CONTO_MS + 20.0, mira_a(&g, 1)), Esito::Colpo);
        g.avanza(CONTO_MS + 21.0);
        assert_eq!(g.comparse(), 4);

        // Con l'attesa della reazione la comparsa arriva quando la sfera si vede, non al colpo.
        let mut g = Giro::new(OCCHIO, Caso::new(30), regole("riflesso"));
        g.inizia(-CONTO_MS);
        g.avanza(0.0);
        let attesa = g.comparsa(0);
        assert_eq!(g.comparse(), 0);
        g.avanza(attesa);
        assert_eq!(g.comparse(), 1);
        assert_eq!(g.clic(attesa + 100.0, mira_a(&g, 0)), Esito::Colpo);
        g.avanza(attesa + 101.0);
        assert_eq!(g.comparse(), 1, "la sfera nuova aspetta");
        g.avanza(g.comparsa(0));
        assert_eq!(g.comparse(), 2);
    }

    // ------------------------------------------------------------ tracking

    /// Un giro di tracking da 0 a 30 s a passi di 1 ms: `mira(t)` dice se a `t` il mirino è sulla sfera, `giu(t)` se
    /// il tasto è giù. Gli eventi del tasto si mandano quando cambia, come fa il Raw Input.
    fn tracking(id: &str, mira: impl Fn(f64) -> bool, giu: impl Fn(f64) -> bool) -> f64 {
        let mut g = partito(id, 24, 0.0);
        let mut tasto = false;
        for passo in 0..=30_000 {
            let t = f64::from(passo);
            g.avanza(t);
            let d = if mira(t) { mira_a(&g, 0) } else { -mira_a(&g, 0) };
            if giu(t) != tasto {
                tasto = giu(t);
                g.tasto(t, tasto, d);
            }
            g.traccia(t, d);
            g.aggiorna(t);
        }
        assert!(g.finito);
        g.riepilogo().tracking.unwrap()
    }

    #[test]
    fn tracking_sempre_sopra_e_premuto_e_il_100_per_cento() {
        for id in ["filo", "guizzo"] {
            let p = tracking(id, |_| true, |_| true);
            assert!((p - 1.0).abs() < 1e-3, "{id}: {p}");
        }
    }

    #[test]
    fn tracking_sopra_ma_senza_tasto_non_conta() {
        assert_eq!(tracking("filo", |_| true, |_| false), 0.0);
    }

    #[test]
    fn tracking_col_tasto_ma_fuori_non_conta() {
        assert_eq!(tracking("guizzo", |_| false, |_| true), 0.0);
    }

    #[test]
    fn tracking_meta_tempo_sopra_e_tasto_sempre_giu_e_il_50_per_cento() {
        let p = tracking("filo", |t| t < 15_000.0, |_| true);
        assert!((p - 0.5).abs() < 1e-3, "{p}");
    }

    #[test]
    fn tracking_sopra_sempre_e_tasto_a_meta_e_il_50_per_cento() {
        let p = tracking("guizzo", |_| true, |t| (t / 1000.0).floor() as i64 % 2 == 0);
        assert!((p - 0.5).abs() < 2e-3, "{p}");
    }

    #[test]
    fn tracking_sopra_per_20_secondi_col_tasto_a_secondi_alterni_e_un_terzo() {
        // Sopra nei primi 20 s, tasto giù nei secondi pari (0-1, 2-3, ... 18-19): 10 s di 30.
        let p = tracking("filo", |t| t < 20_000.0, |t| (t / 1000.0).floor() as i64 % 2 == 0);
        assert!((p - 1.0 / 3.0).abs() < 2e-3, "{p}");
    }

    #[test]
    fn nel_tracking_i_clic_non_sono_colpi_ne_errori() {
        let mut g = partito("filo", 25, 0.0);
        let d = mira_a(&g, 0);
        assert_eq!(g.tasto(100.0, true, d), Esito::Ignorato);
        g.tasto(200.0, false, d);
        assert_eq!(g.tasto(300.0, true, -d), Esito::Ignorato);
        assert_eq!((g.colpi, g.errori), (0, 0));
        let r = g.riepilogo();
        assert_eq!((r.precisione, r.tempo_medio_ms), (None, None));
    }

    #[test]
    fn la_pausa_non_entra_nel_tracking() {
        let mut g = partito("filo", 26, 0.0);
        let d = mira_a(&g, 0);
        g.tasto(0.0, true, d);
        // A ogni frame la sfera si muove e il mirino la segue.
        for passo in 1..=10_000 {
            let t = f64::from(passo);
            g.avanza(t);
            g.traccia(t, mira_a(&g, 0));
        }
        g.pausa(10_000.0);
        g.traccia(50_000.0, d);
        g.riprendi(50_000.0);
        // Dopo la ripresa il tasto si ripreme.
        g.avanza(50_000.0);
        g.tasto(50_000.0, true, mira_a(&g, 0));
        for passo in 1..=20_000 {
            let t = 50_000.0 + f64::from(passo);
            g.avanza(t);
            let d = mira_a(&g, 0);
            g.traccia(t, d);
        }
        assert!(g.aggiorna(70_000.0));
        let p = g.riepilogo().tracking.unwrap();
        assert!((p - 1.0).abs() < 1e-3, "{p}");
    }

    #[test]
    fn il_tracking_del_conto_alla_rovescia_non_conta() {
        let mut g = Giro::new(OCCHIO, Caso::new(27), regole("filo"));
        g.inizia(0.0);
        g.avanza(0.0);
        let d = mira_a(&g, 0);
        g.tasto(10.0, true, d);
        // Il clic ha saltato il conto: da qui il tempo di gioco parte, col tasto giù.
        for passo in 11..=3010 {
            let t = f64::from(passo);
            g.avanza(t);
            g.traccia(t, mira_a(&g, 0));
        }
        let fatto = g.riepilogo().tracking.unwrap() * 30_000.0;
        assert!((fatto - 3000.0).abs() < 2.0, "{fatto} ms");
    }
}
