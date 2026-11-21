//! Le misure prese dal programma, a ogni frame, senza allocare (anelli di dimensione fissa).
//!
//! Tutti gli istanti vengono dal QPC:
//! - **frame time**: fra l'inizio di due frame consecutivi; il jitter è la deviazione standard;
//! - **count→present**: dall'istante in cui il thread del Raw Input riceve un evento del mouse a quando
//!   `present` ritorna per il frame che lo contiene. Due valori per frame: dall'evento più vecchio (il caso
//!   peggiore del frame) e dal più recente;
//! - **clic→present**: lo stesso per i clic (il bersaglio nuovo o l'errore sono nel frame dopo il clic);
//! - **attesa della swapchain**: quanto `get_current_texture` tiene fermo il frame (con il vsync è qui che si
//!   aspetta);
//! - **lavoro del frame**: dalla lettura del mouse al ritorno di `present` (CPU).
//!
//! Dopo `present` restano la coda della scheda video, la scansione dello schermo e il pannello: quelli li vede
//! PresentMon (display latency, click-to-photon), non il programma.

use crate::misure::{Anello, Riassunto, riassumi, riassumi_sul_posto, zeri};
use crate::tempo;

/// Campioni tenuti per ogni misura: 65 536, cioè circa 26 s a 2500 fps (una fase del banco intera).
pub const CAPACITA: usize = 1 << 16;
/// Il pannello F3 riassume solo gli ultimi campioni: ordinarli costa circa un decimo di millisecondo.
pub const CAMPIONI_PANNELLO: usize = 4096;

pub struct Latenza {
    pub frame: Anello<CAPACITA>,
    pub count_present_vecchio: Anello<CAPACITA>,
    pub count_present_recente: Anello<CAPACITA>,
    pub clic_present: Anello<4096>,
    pub attesa: Anello<CAPACITA>,
    pub lavoro: Anello<CAPACITA>,
    ultimo_inizio: i64,
    /// Cresce a ogni azzeramento: con il numero di frame dice se le misure sono cambiate.
    generazione: u64,
    scratch: Box<[f64; CAPACITA]>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Misure {
    pub frame: Riassunto,
    pub count_present_vecchio: Riassunto,
    pub count_present_recente: Riassunto,
    pub clic_present: Riassunto,
    pub attesa: Riassunto,
    pub lavoro: Riassunto,
}

impl Default for Latenza {
    fn default() -> Self {
        Self::new()
    }
}

impl Latenza {
    pub fn new() -> Self {
        Latenza {
            frame: Anello::new(),
            count_present_vecchio: Anello::new(),
            count_present_recente: Anello::new(),
            clic_present: Anello::new(),
            attesa: Anello::new(),
            lavoro: Anello::new(),
            ultimo_inizio: 0,
            scratch: zeri(),
            generazione: 0,
        }
    }

    pub fn azzera(&mut self) {
        self.frame.svuota();
        self.count_present_vecchio.svuota();
        self.count_present_recente.svuota();
        self.clic_present.svuota();
        self.attesa.svuota();
        self.lavoro.svuota();
        self.ultimo_inizio = 0;
        self.generazione += 1;
    }

    /// Dopo una pausa: il primo frame non deve contare la pausa come frame time.
    pub fn riparti(&mut self) {
        self.ultimo_inizio = 0;
    }

    /// Cambia ogni volta che le misure cambiano (azzeramento o frame nuovo): il pannello fuori dal giro si
    /// ricalcola solo allora.
    pub fn versione(&self) -> (u64, u64) {
        (self.generazione, self.lavoro.totale)
    }

    /// All'inizio del frame (prima dell'attesa della swapchain).
    #[inline]
    pub fn inizio_frame(&mut self, ora: i64) {
        if self.ultimo_inizio != 0 {
            self.frame.aggiungi(tempo::ms(ora - self.ultimo_inizio));
        }
        self.ultimo_inizio = ora;
    }

    /// Quando `present` è tornato. `primo`/`ultimo` sono gli istanti degli eventi di movimento letti per questo
    /// frame (0 se nessuno), `clic` quello del primo clic letto (0 se nessuno).
    #[inline]
    pub fn fine_frame(
        &mut self,
        presentato: i64,
        acquisito: i64,
        letto: i64,
        primo: i64,
        ultimo: i64,
        clic: i64,
    ) {
        self.attesa.aggiungi(tempo::ms(acquisito - self.ultimo_inizio));
        self.lavoro.aggiungi(tempo::ms(presentato - letto));
        if primo != 0 {
            self.count_present_vecchio.aggiungi(tempo::ms(presentato - primo));
            self.count_present_recente.aggiungi(tempo::ms(presentato - ultimo.max(primo)));
        }
        if clic != 0 {
            self.clic_present.aggiungi(tempo::ms(presentato - clic));
        }
    }

    /// Il riassunto degli ultimi `k` campioni di ogni misura (il pannello F3). Non alloca.
    pub fn misure_recenti(&mut self, k: usize) -> Misure {
        let s = &mut self.scratch[..];
        let mut r = |a: &dyn Fn(&mut [f64]) -> usize| {
            let n = a(&mut s[..]);
            riassumi_sul_posto(&mut s[..n])
        };
        Misure {
            frame: r(&|d| self.frame.ultimi(k, d)),
            count_present_vecchio: r(&|d| self.count_present_vecchio.ultimi(k, d)),
            count_present_recente: r(&|d| self.count_present_recente.ultimi(k, d)),
            clic_present: r(&|d| self.clic_present.ultimi(k, d)),
            attesa: r(&|d| self.attesa.ultimi(k, d)),
            lavoro: r(&|d| self.lavoro.ultimi(k, d)),
        }
    }

    /// Il riassunto di tutto. Ordina copie dei campioni: si chiama fuori dal ciclo dei frame (pannello, banco).
    pub fn misure(&mut self) -> Misure {
        let s = &mut self.scratch[..];
        Misure {
            frame: riassumi(self.frame.campioni(), s),
            count_present_vecchio: riassumi(self.count_present_vecchio.campioni(), s),
            count_present_recente: riassumi(self.count_present_recente.campioni(), s),
            clic_present: riassumi(self.clic_present.campioni(), s),
            attesa: riassumi(self.attesa.campioni(), s),
            lavoro: riassumi(self.lavoro.campioni(), s),
        }
    }
}

fn riga(r: &Riassunto) -> String {
    if r.n == 0 {
        "nessun campione".to_string()
    } else {
        format!("{:6.2} / {:6.2} ms  ({})", r.mediana, r.p99, r.n)
    }
}

/// Il testo del pannello della latenza.
pub fn testo_pannello(m: &Misure, presentazione: &str, limite_fps: u32, extra: &str) -> String {
    let fps = if m.frame.n > 0 && m.frame.mediana > 0.0 { 1000.0 / m.frame.mediana } else { 0.0 };
    format!(
        "LATENZA, mediana / 99° percentile\n\
         fotogrammi        {fps:7.0} fps   {}\n\
         frame time        {}\n\
         jitter            {:6.3} ms (dev. std)\n\
         count→present     {}  evento più vecchio\n\
         count→present     {}  evento più recente\n\
         clic→present      {}\n\
         attesa swapchain  {}\n\
         lavoro del frame  {}\n\
         limite fps        {}\n\
         {extra}\
         F3 chiude. Le misure ripartono a ogni giro.",
        presentazione,
        riga(&m.frame),
        m.frame.deviazione,
        riga(&m.count_present_vecchio),
        riga(&m.count_present_recente),
        riga(&m.clic_present),
        riga(&m.attesa),
        riga(&m.lavoro),
        if limite_fps == 0 { "nessuno".to_string() } else { format!("{limite_fps} fps") },
    )
}
