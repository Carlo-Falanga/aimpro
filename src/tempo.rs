//! Il tempo, sempre dal QueryPerformanceCounter di Windows: è lo stesso orologio che usano DXGI e PresentMon, così
//! gli istanti del programma si confrontano con i loro.
//!
//! Il limite dei fotogrammi aspetta con un timer ad alta risoluzione (`CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`,
//! Windows 10 1803 e successivi) e chiude l'ultimo tratto girando a vuoto per al massimo `SPIN_FINALE`: dorme
//! quasi tutto il tempo (consumi bassi) e si sveglia preciso.

use std::sync::OnceLock;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, INFINITE, SetWaitableTimer,
    TIMER_ALL_ACCESS, WaitForSingleObject,
};

fn frequenza() -> i64 {
    static F: OnceLock<i64> = OnceLock::new();
    *F.get_or_init(|| {
        let mut f = 0i64;
        // SAFETY: puntatore valido a un i64; la funzione non fallisce da Windows XP in poi.
        unsafe { QueryPerformanceFrequency(&mut f) }.expect("QueryPerformanceFrequency");
        f
    })
}

/// L'istante adesso, in tick del QPC.
#[inline]
pub fn qpc() -> i64 {
    let mut t = 0i64;
    // SAFETY: puntatore valido a un i64.
    let _ = unsafe { QueryPerformanceCounter(&mut t) };
    t
}

/// Tick del QPC in millisecondi.
#[inline]
pub fn ms(tick: i64) -> f64 {
    tick as f64 * 1000.0 / frequenza() as f64
}

/// Millisecondi in tick del QPC.
#[inline]
pub fn tick(ms: f64) -> i64 {
    (ms * frequenza() as f64 / 1000.0) as i64
}

/// Quanto prima della scadenza smettere di dormire e girare a vuoto.
const SPIN_FINALE_MS: f64 = 0.5;

/// Attende fino a un istante QPC preciso, dormendo il più possibile.
pub struct Attesa {
    timer: HANDLE,
}

impl Attesa {
    pub fn new() -> Self {
        // SAFETY: parametri validi; un handle nullo vuol dire timer non disponibile e si ripiega sul giro a vuoto.
        let timer = unsafe {
            CreateWaitableTimerExW(None, None, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0)
        }
        .unwrap_or_default();
        Attesa { timer }
    }

    /// Torna quando il QPC ha raggiunto `scadenza`.
    pub fn fino_a(&self, scadenza: i64) {
        let resto = ms(scadenza - qpc());
        if resto > SPIN_FINALE_MS && !self.timer.is_invalid() {
            // Valore negativo = tempo relativo, in unità da 100 ns.
            let dovuto = -(((resto - SPIN_FINALE_MS) * 10_000.0) as i64);
            // SAFETY: handle del timer valido, posseduto da questa struttura.
            unsafe {
                if SetWaitableTimer(self.timer, &dovuto, 0, None, None, false).is_ok() {
                    WaitForSingleObject(self.timer, INFINITE);
                }
            }
        }
        while qpc() < scadenza {
            std::hint::spin_loop();
        }
    }
}

impl Default for Attesa {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Attesa {
    fn drop(&mut self) {
        if !self.timer.is_invalid() {
            // SAFETY: handle valido, chiuso una volta sola.
            let _ = unsafe { CloseHandle(self.timer) };
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ms_e_tick_si_invertono() {
        let t = tick(16.0);
        assert!((ms(t) - 16.0).abs() < 1e-3);
    }

    #[test]
    fn l_attesa_arriva_alla_scadenza_senza_sforare_troppo() {
        let a = Attesa::new();
        for _ in 0..5 {
            let scadenza = qpc() + tick(3.0);
            a.fino_a(scadenza);
            let ritardo = ms(qpc() - scadenza);
            assert!(ritardo >= 0.0);
            // Largo: sulla CI (macchina virtuale condivisa) lo scheduler può fermare il thread per qualche ms.
            // La precisione vera la misura il banco (jitter col limite di fps).
            assert!(ritardo < 15.0, "sforato di {ritardo} ms");
        }
    }
}
