//! Una GET con WinHTTP, la libreria HTTP di Windows: niente librerie di rete in più. Solo per l'aggiornamento,
//! in un thread suo. Segue i redirect (i file delle release di GitHub passano da un altro dominio) e usa il proxy
//! di sistema. Nessun dato personale nella richiesta: solo `User-Agent: aimpro/<versione>`.

use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTP_PORT, INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR};

use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

/// L'errore di una richiesta chiusa perché è cominciato un giro.
pub const ANNULLATA: &str = "annullata: è cominciato un giro";

/// La richiesta in corso, per poterla chiudere da fuori (`annulla`). Una alla volta.
static ATTIVA: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Chiude la richiesta in corso, se c'è: WinHTTP interrompe subito anche le chiamate sincrone (nome, connessione,
/// TLS, attesa della risposta, lettura) e quelle tornano con un errore.
/// Quante richieste ha chiuso `annulla`: un errore arrivato dopo un annullamento è un annullamento, anche se il
/// giro è già finito (per esempio subito in pausa).
static ANNULLATE: AtomicU64 = AtomicU64::new(0);

pub fn annulla() {
    let h = ATTIVA.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if !h.is_null() {
        ANNULLATE.fetch_add(1, Ordering::SeqCst);
        // SAFETY: la maniglia è stata tolta da ATTIVA con lo swap: la chiude solo chi l'ha presa.
        let _ = unsafe { WinHttpCloseHandle(h) };
    }
}

/// La maniglia della richiesta, registrata in ATTIVA: si chiude qui o in `annulla`, mai due volte.
struct Registrata(*mut core::ffi::c_void);

impl Drop for Registrata {
    fn drop(&mut self) {
        if ATTIVA.compare_exchange(self.0, std::ptr::null_mut(), Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            // SAFETY: tolta da ATTIVA da noi: `annulla` non la chiuderà.
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

/// Un indirizzo diviso: https?, host, porta, percorso con la query.
#[derive(Debug, PartialEq, Eq)]
pub struct Indirizzo<'a> {
    pub sicuro: bool,
    pub host: &'a str,
    pub porta: u16,
    pub percorso: &'a str,
}

pub fn dividi(url: &str) -> Option<Indirizzo<'_>> {
    let (sicuro, resto) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else {
        (false, url.strip_prefix("http://")?)
    };
    let (autorita, percorso) = match resto.find('/') {
        Some(i) => (&resto[..i], &resto[i..]),
        None => (resto, "/"),
    };
    let (host, porta) = match autorita.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (autorita, if sicuro { INTERNET_DEFAULT_HTTPS_PORT } else { INTERNET_DEFAULT_HTTP_PORT }),
    };
    if host.is_empty() || autorita.contains('@') {
        return None;
    }
    Some(Indirizzo { sicuro, host, porta, percorso })
}

struct Maniglia(*mut core::ffi::c_void);

impl Maniglia {
    fn nuova(h: *mut core::ffi::c_void) -> Result<Maniglia, String> {
        if h.is_null() { Err(windows::core::Error::from_thread().message()) } else { Ok(Maniglia(h)) }
    }
}

impl Drop for Maniglia {
    fn drop(&mut self) {
        // SAFETY: la maniglia è di WinHTTP, aperta da noi e chiusa una volta sola.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn numero(richiesta: &Registrata, informazione: u32) -> Option<u32> {
    let mut valore = 0u32;
    let mut lunghezza = size_of::<u32>() as u32;
    // SAFETY: buffer di 4 byte per un'intestazione chiesta come numero.
    unsafe {
        WinHttpQueryHeaders(
            richiesta.0,
            informazione | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&raw mut valore).cast()),
            &mut lunghezza,
            std::ptr::null_mut(),
        )
    }
    .ok()
    .map(|()| valore)
}

/// GET di `url`, al più `limite` byte. `progresso(scaricati, totali)` dopo ogni pezzo. Se comincia un giro la
/// richiesta si chiude e torna `ANNULLATA`.
pub fn scarica(
    url: &str,
    accetta: Option<&str>,
    limite: usize,
    progresso: &mut dyn FnMut(usize, Option<usize>),
) -> Result<Vec<u8>, String> {
    let prima = ANNULLATE.load(Ordering::SeqCst);
    let esito = scarica_senza_giro(url, accetta, limite, progresso);
    let annullata = crate::aggiornamento::si_gioca() || ANNULLATE.load(Ordering::SeqCst) != prima;
    if esito.is_err() && annullata { Err(ANNULLATA.to_string()) } else { esito }
}

fn scarica_senza_giro(
    url: &str,
    accetta: Option<&str>,
    limite: usize,
    progresso: &mut dyn FnMut(usize, Option<usize>),
) -> Result<Vec<u8>, String> {
    let u = dividi(url).ok_or_else(|| format!("indirizzo non valido: {url}"))?;
    let agente = HSTRING::from(format!("aimpro/{}", env!("CARGO_PKG_VERSION")));
    // SAFETY: chiamate WinHTTP sincrone con stringhe che vivono fino alla fine della funzione; ogni maniglia si
    // chiude nel suo Drop, nell'ordine inverso di apertura.
    unsafe {
        let sessione = Maniglia::nuova(WinHttpOpen(
            &agente,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        // Nome, connessione, invio: 10 s; ogni lettura: 30 s.
        WinHttpSetTimeouts(sessione.0, 10_000, 10_000, 10_000, 30_000).map_err(|e| e.message())?;
        let connessione = Maniglia::nuova(WinHttpConnect(sessione.0, &HSTRING::from(u.host), u.porta, 0))?;
        let h = WinHttpOpenRequest(
            connessione.0,
            &HSTRING::from("GET"),
            &HSTRING::from(u.percorso),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            if u.sicuro { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) },
        );
        if h.is_null() {
            return Err(windows::core::Error::from_thread().message());
        }
        // Prima si registra, poi si guarda il giro: chi entra nel giro dopo la chiude, chi c'era già si vede qui.
        ATTIVA.store(h, Ordering::SeqCst);
        let richiesta = Registrata(h);
        if crate::aggiornamento::si_gioca() {
            return Err(ANNULLATA.to_string());
        }
        let intestazioni: Option<Vec<u16>> =
            accetta.map(|a| format!("Accept: {a}\r\n").encode_utf16().collect());
        WinHttpSendRequest(richiesta.0, intestazioni.as_deref(), None, 0, 0, 0).map_err(|e| e.message())?;
        WinHttpReceiveResponse(richiesta.0, std::ptr::null_mut()).map_err(|e| e.message())?;
        let codice = numero(&richiesta, WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
        if codice != 200 {
            return Err(format!("il server ha risposto {codice}"));
        }
        let totali = numero(&richiesta, WINHTTP_QUERY_CONTENT_LENGTH).map(|n| n as usize);
        if totali.is_some_and(|t| t > limite) {
            return Err(format!("file troppo grande ({} byte)", totali.unwrap_or(0)));
        }
        let mut dati = Vec::with_capacity(totali.unwrap_or(0));
        let mut pezzo = vec![0u8; 64 * 1024];
        loop {
            if crate::aggiornamento::si_gioca() {
                return Err(ANNULLATA.to_string());
            }
            let mut letti = 0u32;
            WinHttpReadData(richiesta.0, pezzo.as_mut_ptr().cast(), pezzo.len() as u32, &mut letti)
                .map_err(|e| format!("interrotto: {}", e.message()))?;
            if letti == 0 {
                break;
            }
            dati.extend_from_slice(&pezzo[..letti as usize]);
            if dati.len() > limite {
                return Err(format!("file troppo grande (oltre {limite} byte)"));
            }
            progresso(dati.len(), totali);
        }
        if totali.is_some_and(|t| t != dati.len()) {
            let mb = |n: usize| n as f64 / 1e6;
            return Err(format!("interrotto a {:.1} MB su {:.1}", mb(dati.len()), mb(totali.unwrap_or(0))));
        }
        Ok(dati)
    }
}

/// I test che toccano la richiesta attiva o il flag del giro (globali) girano uno alla volta.
#[cfg(test)]
pub static PROVE_DI_RETE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn annulla_chiude_subito_una_richiesta_ferma_in_attesa() {
        let _uno_alla_volta = PROVE_DI_RETE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Un server che accetta la connessione e non risponde mai: la richiesta resta dentro WinHTTP.
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest.json", server.local_addr().unwrap());
        let tiene = std::thread::spawn(move || server.accept().map(|(c, _)| c));
        let richiesta = std::thread::spawn(move || {
            let t = std::time::Instant::now();
            (scarica(&url, None, 1024, &mut |_, _| {}), t)
        });
        let _connessione = tiene.join().unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let t = std::time::Instant::now();
        annulla();
        let chiusura = t.elapsed();
        let (esito, partita) = richiesta.join().unwrap();
        assert_eq!(esito, Err(ANNULLATA.to_string()));
        assert!(chiusura < std::time::Duration::from_millis(50), "annulla ha bloccato {chiusura:?}");
        assert!(partita.elapsed() < std::time::Duration::from_secs(3), "la richiesta non si è fermata");
    }

    #[test]
    fn divide_gli_indirizzi() {
        assert_eq!(
            dividi("https://api.github.com/repos/Carlo-Falanga/aimpro/releases/latest"),
            Some(Indirizzo {
                sicuro: true,
                host: "api.github.com",
                porta: 443,
                percorso: "/repos/Carlo-Falanga/aimpro/releases/latest"
            })
        );
        assert_eq!(
            dividi("http://127.0.0.1:8765/latest.json?x=1"),
            Some(Indirizzo { sicuro: false, host: "127.0.0.1", porta: 8765, percorso: "/latest.json?x=1" })
        );
        assert_eq!(dividi("https://github.com").map(|i| i.percorso), Some("/"));
        assert_eq!(dividi("ftp://x/y"), None);
        assert_eq!(dividi("https:///y"), None);
        assert_eq!(dividi("https://a:b@host/"), None);
        assert_eq!(dividi("https://host:porta/"), None);
    }
}
