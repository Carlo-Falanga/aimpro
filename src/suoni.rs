//! I suoni di colpo, errore e comparsa: originali, sintetizzati qui all'avvio (niente file né suoni di altri), e
//! riprodotti da un thread audio tutto suo con WASAPI in modalità condivisa.
//!
//! Il frame del giro non aspetta niente: `suona` incrementa un contatore atomico e, solo se il thread audio dorme,
//! lo sveglia con un `SetEvent`. Niente allocazioni, lock o chiamate a WASAPI nel frame.
//!
//! Il thread audio dorme sull'evento finché non arriva una richiesta; allora apre WASAPI sull'uscita predefinita del
//! momento, tiene in coda un periodo del motore audio (di solito 10 ms) e lo ricarica a ogni evento di WASAPI; dopo
//! 3 secondi di silenzio chiude il flusso e torna a dormire. Fuori dal giro quindi non consuma niente, e un'uscita
//! cambiata o staccata si prende alla riapertura. Il formato è il nostro (48 kHz, stereo, float) e Windows lo
//! converte in quello del dispositivo (`AUTOCONVERTPCM`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suono {
    Colpo = 0,
    Errore = 1,
    Comparsa = 2,
}

pub const FREQUENZA: u32 = 48_000;
const CANALI: usize = 2;
/// Voci che suonano insieme al massimo: oltre, la più vecchia lascia il posto.
const VOCI: usize = 16;

// ---------------------------------------------------------------- sintesi

/// Un suono come campioni mono a `frequenza`, fra -1 e 1, con l'inizio e la fine a zero (niente clic).
pub fn genera(s: Suono, frequenza: u32) -> Vec<f32> {
    // (durata s, frequenza iniziale e finale Hz, attacco s, decadimento s, ampiezza, peso dell'armonica)
    let (durata, f0, f1, attacco, decadimento, ampiezza, armonica) = match s {
        // Un "tic" chiaro e breve, che scende appena di tono.
        Suono::Colpo => (0.075, 1500.0, 1050.0, 0.0015, 0.018, 0.55, 0.3),
        // Un tonfo basso e sordo.
        Suono::Errore => (0.11, 210.0, 130.0, 0.003, 0.035, 0.5, 0.15),
        // Un "blip" leggero che sale.
        Suono::Comparsa => (0.06, 680.0, 980.0, 0.004, 0.02, 0.22, 0.0),
    };
    let fr = f64::from(frequenza);
    let n = (durata * fr) as usize;
    let dissolvenza = (0.005 * fr) as usize;
    let mut fase = 0.0f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / fr;
        let f = f0 + (f1 - f0) * (t / durata);
        fase += std::f64::consts::TAU * f / fr;
        let inviluppo = (1.0 - (-t / attacco).exp()) * (-t / decadimento).exp();
        let coda = if i + dissolvenza >= n { (n - i) as f64 / dissolvenza as f64 } else { 1.0 };
        let onda = fase.sin() + armonica * (2.0 * fase).sin();
        out.push((ampiezza * inviluppo * coda * onda / (1.0 + armonica)) as f32);
    }
    out
}

/// Il guadagno per un volume da 0 a 100: al quadrato, come lo sente l'orecchio.
pub fn guadagno(volume: f64) -> f32 {
    let v = (volume.clamp(0.0, 100.0) / 100.0) as f32;
    v * v
}

/// Una voce che suona: quale suono e dove è arrivata.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Voce {
    pub suono: usize,
    pub pos: usize,
    pub attiva: bool,
}

/// Mescola le voci attive in `uscita` (stereo interlacciato), con il guadagno, tagliando a ±1. Le voci finite si
/// spengono. Dice se resta qualcosa da suonare.
pub fn mescola(voci: &mut [Voce], campioni: &[Vec<f32>], guadagno: f32, uscita: &mut [f32]) -> bool {
    uscita.fill(0.0);
    let frame = uscita.len() / CANALI;
    for v in voci.iter_mut().filter(|v| v.attiva) {
        let c = &campioni[v.suono];
        let quanti = frame.min(c.len().saturating_sub(v.pos));
        for i in 0..quanti {
            let s = c[v.pos + i] * guadagno;
            uscita[i * CANALI] += s;
            uscita[i * CANALI + 1] += s;
        }
        v.pos += quanti;
        if v.pos >= c.len() {
            v.attiva = false;
        }
    }
    for s in uscita.iter_mut() {
        *s = s.clamp(-1.0, 1.0);
    }
    voci.iter().any(|v| v.attiva)
}

/// Aggiunge una voce; se sono tutte occupate prende il posto di quella più avanti (la più vecchia).
pub fn aggiungi(voci: &mut [Voce], suono: usize) {
    let i = voci
        .iter()
        .position(|v| !v.attiva)
        .unwrap_or_else(|| voci.iter().enumerate().max_by_key(|(_, v)| v.pos).map_or(0, |(i, _)| i));
    voci[i] = Voce { suono, pos: 0, attiva: true };
}

// ---------------------------------------------------------------- il thread

/// Quello che il frame e il menu vedono del thread audio.
pub struct Suoni {
    richieste: [AtomicU32; 3],
    /// Il volume (0-100) in bit di f32; 0 o `acceso` falso = niente richieste.
    volume: AtomicU32,
    acceso: AtomicBool,
    /// Il thread dorme sull'evento: va svegliato.
    dorme: AtomicBool,
    sveglia: AtomicIsize,
    /// Il flusso WASAPI è partito almeno una volta (per il banco).
    pub suonati: AtomicU32,
    /// WASAPI non è partito: il motivo resta nel registro, il gioco va avanti muto.
    pub guasto: AtomicBool,
}

impl Suoni {
    /// Senza thread né dispositivo (per i test e se l'audio non parte).
    pub fn muti() -> Arc<Self> {
        Arc::new(Suoni {
            richieste: [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)],
            volume: AtomicU32::new(0f32.to_bits()),
            acceso: AtomicBool::new(false),
            dorme: AtomicBool::new(false),
            sveglia: AtomicIsize::new(0),
            suonati: AtomicU32::new(0),
            guasto: AtomicBool::new(false),
        })
    }

    pub fn avvia(acceso: bool, volume: f64) -> Arc<Self> {
        let s = Suoni::muti();
        s.imposta(acceso, volume);
        // Il thread parte subito ma dorme: WASAPI si apre alla prima richiesta.
        let evento = unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) };
        match evento {
            Ok(e) => s.sveglia.store(e.0 as isize, Ordering::Release),
            Err(e) => {
                log::warn!("suoni: evento non creato: {e}");
                s.guasto.store(true, Ordering::Release);
                return s;
            }
        }
        let t = s.clone();
        let partito = std::thread::Builder::new().name("aimpro-suoni".into()).spawn(move || {
            wasapi::gira(&t);
        });
        if partito.is_err() {
            s.guasto.store(true, Ordering::Release);
        }
        s
    }

    pub fn imposta(&self, acceso: bool, volume: f64) {
        self.volume.store((volume as f32).to_bits(), Ordering::Release);
        self.acceso.store(acceso && volume > 0.0, Ordering::Release);
    }

    fn volume(&self) -> f64 {
        f64::from(f32::from_bits(self.volume.load(Ordering::Acquire)))
    }

    /// Dal frame: niente attese, niente allocazioni.
    #[inline]
    pub fn suona(&self, s: Suono) {
        if !self.acceso.load(Ordering::Relaxed) {
            return;
        }
        self.richieste[s as usize].fetch_add(1, Ordering::SeqCst);
        if self.dorme.load(Ordering::SeqCst) {
            let h = self.sveglia.load(Ordering::Acquire);
            if h != 0 {
                let _ = unsafe {
                    windows::Win32::System::Threading::SetEvent(windows::Win32::Foundation::HANDLE(
                        h as *mut core::ffi::c_void,
                    ))
                };
            }
        }
    }

    pub fn richieste_totali(&self) -> u32 {
        self.richieste.iter().map(|r| r.load(Ordering::Relaxed)).sum()
    }
}

mod wasapi {
    use std::sync::atomic::Ordering;

    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Media::Audio::{
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator,
        MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::System::Threading::{
        CreateEventW, GetCurrentThread, INFINITE, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
        WaitForSingleObject,
    };

    /// L'evento di WASAPI, chiuso comunque esca il flusso.
    struct Evento(HANDLE);

    impl Drop for Evento {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    use super::{CANALI, FREQUENZA, Suoni, VOCI, Voce, aggiungi, genera, guadagno, mescola};

    /// 20 ms di buffer, in unità da 100 ns.
    const BUFFER_100NS: i64 = 200_000;
    /// Dopo quanto silenzio il flusso si chiude.
    const SILENZIO_MS: u32 = 3000;

    /// Ci sono richieste non ancora viste?
    fn nuove_richieste(s: &Suoni, visti: &[u32; 3]) -> bool {
        s.richieste.iter().zip(visti).any(|(r, v)| r.load(Ordering::SeqCst) != *v)
    }

    /// Dorme finché non arriva una richiesta: prima si dice, poi si ricontrolla, così una richiesta arrivata in
    /// mezzo non si perde.
    fn dormi(s: &Suoni, visti: &[u32; 3], sveglia: HANDLE) {
        s.dorme.store(true, Ordering::SeqCst);
        // Un SetEvent rimasto acceso senza richieste nuove (due suoni nello stesso frame) non apre WASAPI per niente.
        while !nuove_richieste(s, visti) {
            unsafe { WaitForSingleObject(sveglia, INFINITE) };
        }
        s.dorme.store(false, Ordering::SeqCst);
    }

    /// Il thread audio. WASAPI si apre solo alla richiesta (coi suoni spenti e nel banco senza suoni non si apre
    /// mai), ogni volta sull'uscita predefinita del momento.
    pub fn gira(s: &Suoni) -> ! {
        // Sopra il normale: con un solo periodo in coda, un ritardo del thread sarebbe un buco nel suono. Il thread
        // lavora pochi microsecondi ogni 10 ms e dorme il resto.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };
        let sveglia = HANDLE(s.sveglia.load(Ordering::Acquire) as *mut core::ffi::c_void);
        let campioni = [
            genera(super::Suono::Colpo, FREQUENZA),
            genera(super::Suono::Errore, FREQUENZA),
            genera(super::Suono::Comparsa, FREQUENZA),
        ];
        let mut visti = [0u32; 3];
        loop {
            dormi(s, &visti, sveglia);
            let mut aperto = false;
            let Err(e) = flusso(s, &mut visti, &campioni, &mut aperto) else {
                continue;
            };
            // Un flusso che funzionava e si è rotto (cuffie staccate): si riapre subito, coi suoni ancora da fare.
            let e = if aperto {
                // Un attimo perché Windows passi all'altra uscita predefinita.
                std::thread::sleep(std::time::Duration::from_millis(150));
                let mut di_nuovo = false;
                match flusso(s, &mut visti, &campioni, &mut di_nuovo) {
                    Ok(()) => continue,
                    Err(e) => e,
                }
            } else {
                e
            };
            log::warn!("suoni: {e}; riprovo alla prossima richiesta");
            s.guasto.store(true, Ordering::Release);
            std::thread::sleep(std::time::Duration::from_millis(500));
            // Le richieste arrivate mentre l'uscita mancava (anche durante la pausa) non si suonano dopo.
            for (v, r) in visti.iter_mut().zip(&s.richieste) {
                *v = r.load(Ordering::SeqCst);
            }
        }
    }

    /// Un flusso, dall'apertura alla chiusura dopo il silenzio. `aperto` diventa vero quando il flusso è partito.
    fn flusso(
        s: &Suoni,
        visti: &mut [u32; 3],
        campioni: &[Vec<f32>; 3],
        aperto: &mut bool,
    ) -> Result<(), String> {
        let e = |cosa: &'static str| move |x: windows::core::Error| format!("{cosa}: {x}");
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(e("COM"))?;
            let elenco: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e("dispositivi audio"))?;
            let dispositivo =
                elenco.GetDefaultAudioEndpoint(eRender, eConsole).map_err(e("nessuna uscita audio"))?;
            let client: IAudioClient = dispositivo.Activate(CLSCTX_ALL, None).map_err(e("client audio"))?;
            let blocco = (CANALI * 4) as u16;
            let formato = WAVEFORMATEX {
                wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
                nChannels: CANALI as u16,
                nSamplesPerSec: FREQUENZA,
                nAvgBytesPerSec: FREQUENZA * u32::from(blocco),
                nBlockAlign: blocco,
                wBitsPerSample: 32,
                cbSize: 0,
            };
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    BUFFER_100NS,
                    0,
                    &formato,
                    None,
                )
                .map_err(e("formato audio"))?;
            let pronto = Evento(CreateEventW(None, false, false, None).map_err(e("evento audio"))?);
            client.SetEventHandle(pronto.0).map_err(e("evento audio"))?;
            let render: IAudioRenderClient = client.GetService().map_err(e("render audio"))?;
            let lato = client.GetBufferSize().map_err(e("buffer audio"))? as usize;
            // In coda si tiene un periodo del motore audio (di solito 10 ms), non tutto il buffer: un suono chiesto
            // adesso esce dopo al più un periodo, invece di finire dietro al silenzio già scritto.
            let mut periodo_100ns = 100_000i64;
            let _ = client.GetDevicePeriod(Some(&mut periodo_100ns), None);
            let obiettivo =
                ((periodo_100ns as f64 / 1e7 * f64::from(FREQUENZA)).ceil() as usize).clamp(1, lato);
            let periodo_ms = ((periodo_100ns / 10_000) as u32).max(1);
            s.guasto.store(false, Ordering::Release);

            let mut voci = [Voce::default(); VOCI];
            let mut misto = vec![0f32; lato * CANALI];
            let mut partito = false;
            let mut silenzio_ms = 0u32;

            loop {
                // Le richieste nuove diventano voci.
                let mut nuove = false;
                for (i, r) in s.richieste.iter().enumerate() {
                    let n = r.load(Ordering::SeqCst);
                    while visti[i] != n {
                        visti[i] = visti[i].wrapping_add(1);
                        aggiungi(&mut voci, i);
                        nuove = true;
                    }
                }
                if !partito && !voci.iter().any(|v| v.attiva) {
                    return Ok(()); // svegliato senza niente da suonare
                }
                if nuove {
                    silenzio_ms = 0;
                }
                // Si riempie fino a un periodo in coda.
                let occupato = client.GetCurrentPadding().map_err(e("audio"))? as usize;
                let liberi = obiettivo.saturating_sub(occupato).min(lato.saturating_sub(occupato));
                if liberi > 0 {
                    let dati = render.GetBuffer(liberi as u32).map_err(e("audio"))?;
                    let parte = &mut misto[..liberi * CANALI];
                    let resta = mescola(&mut voci, campioni, guadagno(s.volume()), parte);
                    std::ptr::copy_nonoverlapping(parte.as_ptr(), dati.cast::<f32>(), parte.len());
                    render.ReleaseBuffer(liberi as u32, 0).map_err(e("audio"))?;
                    if !resta {
                        silenzio_ms += periodo_ms;
                    }
                }
                if !partito {
                    client.Start().map_err(e("avvio audio"))?;
                    partito = true;
                    *aperto = true;
                    s.guasto.store(false, Ordering::Release);
                    s.suonati.fetch_add(1, Ordering::Relaxed);
                }
                if silenzio_ms >= SILENZIO_MS && !voci.iter().any(|v| v.attiva) {
                    // Si chiude: la prossima richiesta riapre sull'uscita predefinita di quel momento.
                    let _ = client.Stop();
                    return Ok(());
                }
                if WaitForSingleObject(pronto.0, 200) != WAIT_OBJECT_0 {
                    // Nessun evento in 200 ms: conta come silenzio. Un'uscita persa dà errore alla prossima chiamata.
                    silenzio_ms += 200;
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn i_suoni_stanno_fra_meno_1_e_1_e_non_fanno_clic() {
        for s in [Suono::Colpo, Suono::Errore, Suono::Comparsa] {
            let c = genera(s, FREQUENZA);
            assert!(c.len() > 1000 && c.len() < FREQUENZA as usize / 5, "{s:?} {}", c.len());
            assert!(c.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
            // Inizio e fine a zero: niente scatto quando parte o finisce.
            assert!(c[0].abs() < 0.01 && c[c.len() - 1].abs() < 0.01, "{s:?}");
            let picco = c.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(picco > 0.1, "{s:?} quasi muto: {picco}");
        }
    }

    fn passaggi_per_zero(c: &[f32]) -> usize {
        c.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    #[test]
    fn colpo_acuto_errore_grave() {
        let colpo = genera(Suono::Colpo, FREQUENZA);
        let errore = genera(Suono::Errore, FREQUENZA);
        // Passaggi per zero al secondo: circa il doppio della frequenza.
        let hz = |c: &[f32]| passaggi_per_zero(c) as f64 / (c.len() as f64 / f64::from(FREQUENZA)) / 2.0;
        assert!(hz(&colpo) > 900.0, "{}", hz(&colpo));
        assert!(hz(&errore) < 260.0, "{}", hz(&errore));
        assert!(colpo != genera(Suono::Comparsa, FREQUENZA));
    }

    #[test]
    fn il_volume_va_da_zero_al_pieno() {
        assert_eq!(guadagno(0.0), 0.0);
        assert_eq!(guadagno(100.0), 1.0);
        assert_eq!(guadagno(50.0), 0.25);
        assert_eq!(guadagno(400.0), 1.0);
    }

    #[test]
    fn la_mescola_suona_tutto_e_poi_si_spegne() {
        let campioni = vec![vec![0.5f32; 10], vec![0.75f32; 4], vec![]];
        let mut voci = [Voce::default(); 4];
        aggiungi(&mut voci, 0);
        aggiungi(&mut voci, 1);
        let mut uscita = [0f32; 12]; // 6 frame stereo
        assert!(mescola(&mut voci, &campioni, 1.0, &mut uscita));
        // I primi 4 frame hanno le due voci insieme, tagliate a 1.
        assert_eq!(&uscita[..2], &[1.0, 1.0]);
        assert_eq!(&uscita[8..10], &[0.5, 0.5]);
        assert!(!voci[1].attiva, "la seconda voce è finita");
        assert!(!mescola(&mut voci, &campioni, 0.5, &mut uscita));
        assert_eq!(&uscita[..2], &[0.25, 0.25]);
        assert_eq!(&uscita[8..], &[0.0; 4]);
        assert!(voci.iter().all(|v| !v.attiva));
    }

    #[test]
    fn con_tutte_le_voci_occupate_cede_la_piu_vecchia() {
        let mut voci = [Voce { suono: 0, pos: 5, attiva: true }; 3];
        voci[1].pos = 50;
        aggiungi(&mut voci, 2);
        assert_eq!(voci[1], Voce { suono: 2, pos: 0, attiva: true });
    }

    #[test]
    fn spenti_non_si_chiede_niente() {
        let s = Suoni::muti();
        s.suona(Suono::Colpo);
        assert_eq!(s.richieste_totali(), 0);
        s.imposta(true, 60.0);
        s.suona(Suono::Colpo);
        s.suona(Suono::Errore);
        assert_eq!(s.richieste_totali(), 2);
        s.imposta(true, 0.0);
        s.suona(Suono::Colpo);
        assert_eq!(s.richieste_totali(), 2, "volume a 0 vale spento");
    }
}
