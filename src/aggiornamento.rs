//! L'aggiornamento dall'app: all'avvio un thread chiede a GitHub l'ultima release; se la sua build è più nuova di
//! quella scritta nell'.exe, il menu lo dice e "Aggiorna e riavvia" scarica il nuovo `aimpro.exe`, ne controlla lo
//! SHA-256 con il file `aimpro.exe.sha256` della stessa release, lo mette al posto di quello in uso e lo riavvia.
//!
//! - Il numero di build lo scrive la CI (`AIMPRO_BUILD` = numero della run, lo stesso del tag
//!   `v<versione>-build.<numero>`). Una build locale non ha numero: niente controllo e niente messaggio.
//! - La rete lavora solo in un thread suo e mai durante il giro: entrando nel giro il thread si ferma (fra una
//!   lettura e l'altra) e riparte quando si torna fuori. Nel frame non c'è niente di questo modulo.
//! - La sostituzione: `<exe>.nuovo` accanto a quello in uso, poi `<exe>` diventa `<exe>.vecchio` (un .exe in uso
//!   si può rinominare, non cancellare) e `<exe>.nuovo` diventa `<exe>`. A ogni errore si torna all'.exe di prima.
//!   `<exe>.vecchio` si cancella all'avvio dopo.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::rete;

/// L'ultima release del repo, dalle API di GitHub (senza autenticazione). Una build di prova può puntare altrove
/// con `AIMPRO_API_PROVA` al momento della compilazione: le release non ce l'hanno.
const API: &str = "https://api.github.com/repos/Carlo-Falanga/aimpro/releases/latest";
const API_PROVA: Option<&str> = option_env!("AIMPRO_API_PROVA");
const NOME_EXE: &str = "aimpro.exe";
const NOME_SHA: &str = "aimpro.exe.sha256";
/// Più grande di così non è un aimpro.exe (la release 0.1.0 è sotto i 10 MB).
const LIMITE_EXE: usize = 64 << 20;
const LIMITE_TESTO: usize = 1 << 20;

/// Il numero di build scritto dalla CI; `None` per una build locale.
pub fn build_attuale() -> Option<u32> {
    option_env!("AIMPRO_BUILD").and_then(|s| s.trim().parse().ok()).filter(|&n| n > 0)
}

/// Una release con i due file che servono.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub build: u32,
    pub tag: String,
    pub url_exe: String,
    pub url_sha: String,
}

/// Cosa mostra il menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stato {
    /// Niente da dire: controllo spento, in corso, ultima versione, build locale o rete assente.
    Nessuno,
    Disponibile(Release),
    /// Byte scaricati e totali (se il server li dice).
    Scarico {
        release: Release,
        scaricati: usize,
        totali: Option<usize>,
    },
    /// L'.exe nuovo è al suo posto: il thread principale salva le impostazioni, lo avvia ed esce.
    Pronto(Release),
    /// L'.exe nuovo è al suo posto ma non si è avviato: va riaperto a mano.
    NonRiparte {
        release: Release,
        motivo: String,
    },
    Errore {
        release: Release,
        motivo: String,
    },
}

/// Il numero di build dal tag `v<versione>-build.<numero>`.
pub fn build_dal_tag(tag: &str) -> Option<u32> {
    let (_, numero) = tag.rsplit_once("-build.")?;
    if numero.is_empty() || !numero.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    numero.parse().ok().filter(|&n| n > 0)
}

#[derive(serde::Deserialize)]
struct RispostaRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<File>,
}

#[derive(serde::Deserialize)]
struct File {
    name: String,
    browser_download_url: String,
}

/// Gli indirizzi accettati per i file: https; http solo in una build di prova con `AIMPRO_API_PROVA`.
fn indirizzo_permesso(url: &str) -> bool {
    url.starts_with("https://") || (API_PROVA.is_some() && url.starts_with("http://"))
}

/// Legge la risposta di `releases/latest`: tag, build e i due file. Una release senza i due file non vale.
pub fn leggi_release(json: &str) -> Result<Release, String> {
    let r: RispostaRelease =
        serde_json::from_str(json).map_err(|e| format!("risposta di GitHub illeggibile: {e}"))?;
    if r.draft || r.prerelease {
        return Err(format!("la release {} non è pubblicata", r.tag_name));
    }
    let build =
        build_dal_tag(&r.tag_name).ok_or_else(|| format!("tag senza numero di build: {}", r.tag_name))?;
    let url = |nome: &str| {
        r.assets
            .iter()
            .find(|a| a.name == nome)
            .map(|a| a.browser_download_url.clone())
            .filter(|u| indirizzo_permesso(u))
            .ok_or_else(|| format!("la release {} non ha {nome}", r.tag_name))
    };
    Ok(Release { build, url_exe: url(NOME_EXE)?, url_sha: url(NOME_SHA)?, tag: r.tag_name.clone() })
}

/// C'è da aggiornare solo con una build nota e più vecchia di quella della release.
pub fn da_aggiornare(attuale: Option<u32>, r: &Release) -> bool {
    attuale.is_some_and(|a| r.build > a)
}

/// Lo SHA-256 dal file `aimpro.exe.sha256` (`<64 cifre esadecimali>  aimpro.exe`).
pub fn sha_atteso(testo: &str) -> Option<[u8; 32]> {
    let h = testo.trim_start_matches('\u{feff}').split_whitespace().next()?;
    if h.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(h.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

pub fn esadecimale(h: &[u8]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 con le API di crittografia di Windows (CNG, `BCryptHash`): niente librerie in più.
pub fn sha256(dati: &[u8]) -> [u8; 32] {
    use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
    let mut out = [0u8; 32];
    // SAFETY: la pseudo-maniglia di SHA-256 esiste da Windows 10; ingresso e uscita sono slice valide.
    let stato = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, dati, &mut out) };
    assert!(stato.is_ok(), "BCryptHash: {stato:?}");
    out
}

/// Controlla l'.exe scaricato: SHA-256 uguale a quello della release e intestazione di un eseguibile.
pub fn controlla(exe: &[u8], testo_sha: &str) -> Result<(), String> {
    let atteso = sha_atteso(testo_sha).ok_or("il file aimpro.exe.sha256 della release non si legge")?;
    let vero = sha256(exe);
    if vero != atteso {
        return Err(format!(
            "il file scaricato non corrisponde allo SHA-256 della release ({} invece di {})",
            &esadecimale(&vero)[..12],
            &esadecimale(&atteso)[..12]
        ));
    }
    if !exe.starts_with(b"MZ") {
        return Err("il file scaricato non è un eseguibile".to_string());
    }
    Ok(())
}

fn accanto(exe: &Path, estensione: &str) -> PathBuf {
    let mut nome = exe.as_os_str().to_os_string();
    nome.push(estensione);
    PathBuf::from(nome)
}

pub fn percorso_vecchio(exe: &Path) -> PathBuf {
    accanto(exe, ".vecchio")
}

/// Mette `nuovo` al posto di `exe`. Se qualcosa va storto `exe` resta quello di prima.
pub fn sostituisci(exe: &Path, nuovo: &[u8]) -> Result<(), String> {
    let provvisorio = accanto(exe, ".nuovo");
    let vecchio = percorso_vecchio(exe);
    let cartella = exe.parent().map_or_else(|| ".".to_string(), |p| p.display().to_string());
    // Scritto e portato su disco prima di rinominare: dopo un'interruzione di corrente non resta un .exe a metà.
    let scritto = std::fs::File::create(&provvisorio).and_then(|mut f| {
        std::io::Write::write_all(&mut f, nuovo)?;
        f.sync_all()
    });
    if let Err(e) = scritto {
        let _ = std::fs::remove_file(&provvisorio);
        return Err(format!("la cartella {cartella} non è scrivibile: {e}"));
    }
    // Le rinomine si fanno tenendo USCITA: chi chiude aimpro le aspetta, e dopo che ha chiuso non ne partono altre.
    // Così l'.exe non resta solo come .vecchio.
    let in_uscita = USCITA.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if *in_uscita {
        let _ = std::fs::remove_file(&provvisorio);
        return Err("aimpro si sta chiudendo".to_string());
    }
    rinomina(exe, &provvisorio, &vecchio)
}

/// Le due rinomine: `exe` → `vecchio`, `provvisorio` → `exe`; a ogni errore si torna com'era.
fn rinomina(exe: &Path, provvisorio: &Path, vecchio: &Path) -> Result<(), String> {
    match std::fs::remove_file(vecchio) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            let _ = std::fs::remove_file(provvisorio);
            return Err(format!("{} non si cancella: {e}", vecchio.display()));
        }
    }
    if let Err(e) = std::fs::rename(exe, vecchio) {
        let _ = std::fs::remove_file(provvisorio);
        return Err(format!("{} non si rinomina: {e}", exe.display()));
    }
    if let Err(e) = std::fs::rename(provvisorio, exe) {
        let _ = std::fs::rename(vecchio, exe);
        let _ = std::fs::remove_file(provvisorio);
        return Err(format!("il nuovo aimpro non va al suo posto: {e}"));
    }
    Ok(())
}

/// Cancella `<exe>.vecchio` lasciato da un aggiornamento. Il processo di prima può essere ancora in chiusura (il
/// file in uso non si cancella): si riprova per qualche secondo, in un thread, fuori dall'avvio.
pub fn pulisci_vecchio(exe: &Path, tentativi: u32) -> bool {
    let vecchio = percorso_vecchio(exe);
    for i in 0..tentativi.max(1) {
        match std::fs::remove_file(&vecchio) {
            Ok(()) => return true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
            Err(_) if i + 1 < tentativi => std::thread::sleep(Duration::from_millis(250)),
            Err(_) => {}
        }
    }
    false
}

/// Vero quando aimpro sta uscendo; `sostituisci` lo tiene bloccato per tutte le rinomine.
static USCITA: Mutex<bool> = Mutex::new(false);

/// Prima di uscire: se le rinomine sono a metà le lascia finire (questione di millisecondi), poi non ne fa partire
/// altre.
pub fn aspetta_sostituzione() {
    *USCITA.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
}

/// Vero mentre si gioca: niente rete.
static NEL_GIRO: AtomicBool = AtomicBool::new(false);

/// Lo chiama l'app entrando nel giro e uscendone. Entrando, la richiesta in corso (se c'è) si chiude subito: la
/// rete non lavora durante il giro; il controllo la rifà quando si esce.
pub fn nel_giro(si: bool) {
    NEL_GIRO.store(si, Ordering::SeqCst);
    if si {
        rete::annulla();
    }
}

pub fn si_gioca() -> bool {
    NEL_GIRO.load(Ordering::SeqCst)
}

/// Aspetta, se serve, che il giro finisca.
fn aspetta_fuori_dal_giro() {
    while si_gioca() {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Una GET fuori dal giro: se un giro la interrompe, la rifà dopo (al più 5 volte).
fn scarica_fuori_dal_giro(
    url: &str,
    accetta: Option<&str>,
    limite: usize,
    progresso: &mut dyn FnMut(usize, Option<usize>),
) -> Result<Vec<u8>, String> {
    let mut esito = Err(rete::ANNULLATA.to_string());
    for _ in 0..5 {
        aspetta_fuori_dal_giro();
        esito = rete::scarica(url, accetta, limite, progresso);
        if !matches!(&esito, Err(e) if e == rete::ANNULLATA) {
            break;
        }
    }
    esito
}

type Sveglia = Box<dyn Fn() + Send + Sync>;

/// Lo stato condiviso fra il thread della rete e il menu.
pub struct Aggiornamento {
    stato: Mutex<Stato>,
    attuale: Option<u32>,
    /// Sveglia il ciclo degli eventi (il menu è fermo in attesa) perché ridisegni.
    sveglia: Sveglia,
}

impl Aggiornamento {
    /// Parte il controllo, se acceso e se la build è nota. Non aspetta niente: la rete è nel suo thread.
    pub fn avvia(controlla: bool, sveglia: Sveglia) -> Arc<Aggiornamento> {
        let attuale = build_attuale();
        let a = Arc::new(Aggiornamento { stato: Mutex::new(Stato::Nessuno), attuale, sveglia });
        if controlla && attuale.is_some() {
            let a2 = a.clone();
            let _ = std::thread::Builder::new().name("aggiornamento".into()).spawn(move || a2.controlla());
        }
        a
    }

    pub fn attuale(&self) -> Option<u32> {
        self.attuale
    }

    pub fn stato(&self) -> Stato {
        self.stato.lock().map_or(Stato::Nessuno, |s| s.clone())
    }

    fn imposta(&self, s: Stato) {
        if let Ok(mut g) = self.stato.lock() {
            *g = s;
        }
        (self.sveglia)();
    }

    /// Chiede l'ultima release. Ogni errore resta muto: senza rete non compare niente.
    fn controlla(&self) {
        let Ok(testo) = scarica_fuori_dal_giro(
            API_PROVA.unwrap_or(API),
            Some("application/vnd.github+json"),
            LIMITE_TESTO,
            &mut |_, _| {},
        ) else {
            return;
        };
        let Ok(r) = leggi_release(&String::from_utf8_lossy(&testo)) else {
            return;
        };
        if da_aggiornare(self.attuale, &r) {
            self.imposta(Stato::Disponibile(r));
        }
    }

    /// "Aggiorna e riavvia": scarica, controlla e sostituisce in un thread. Alla fine lo stato è `Pronto` o
    /// `Errore`.
    pub fn scarica_e_sostituisci(self: &Arc<Self>) {
        let release = match self.stato() {
            Stato::Disponibile(r) | Stato::Errore { release: r, .. } => r,
            _ => return,
        };
        self.imposta(Stato::Scarico { release: release.clone(), scaricati: 0, totali: None });
        let a = self.clone();
        let avviato = std::thread::Builder::new().name("aggiornamento".into()).spawn(move || {
            let esito = a.esegui(&release);
            a.imposta(match esito {
                Ok(()) => Stato::Pronto(release),
                Err(motivo) => Stato::Errore { release, motivo },
            });
        });
        if let Err(e) = avviato {
            self.errore(format!("il download non parte: {e}"));
        }
    }

    fn errore(&self, motivo: String) {
        let release = match self.stato() {
            Stato::Disponibile(r)
            | Stato::Scarico { release: r, .. }
            | Stato::Pronto(r)
            | Stato::NonRiparte { release: r, .. }
            | Stato::Errore { release: r, .. } => r,
            Stato::Nessuno => return,
        };
        self.imposta(Stato::Errore { release, motivo });
    }

    fn esegui(&self, r: &Release) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| format!("non trovo l'.exe in uso: {e}"))?;
        let sha = scarica_fuori_dal_giro(&r.url_sha, None, LIMITE_TESTO, &mut |_, _| {})
            .map_err(|e| format!("download di {NOME_SHA} non riuscito: {e}"))?;
        // Un ridisegno ogni 5% circa: il menu resta fermo fra un passo e l'altro.
        let mut ultimo = 0usize;
        let mut progresso = |scaricati: usize, totali: Option<usize>| {
            let passo = totali.map_or(1 << 20, |t| (t / 20).max(1));
            if scaricati >= ultimo + passo {
                ultimo = scaricati;
                self.imposta(Stato::Scarico { release: r.clone(), scaricati, totali });
            }
        };
        let nuovo = scarica_fuori_dal_giro(&r.url_exe, None, LIMITE_EXE, &mut progresso)
            .map_err(|e| format!("download di {NOME_EXE} non riuscito: {e}"))?;
        controlla(&nuovo, &String::from_utf8_lossy(&sha))?;
        sostituisci(&exe, &nuovo)
    }

    /// Avvia l'.exe nuovo con gli stessi argomenti. Se riesce, chi chiama esce.
    pub fn riavvia(&self) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        match std::process::Command::new(&exe).args(std::env::args_os().skip(1)).spawn() {
            Ok(_) => Ok(()),
            Err(e) => {
                // L'.exe nuovo è già al suo posto: niente "Riprova", basta riaprire.
                if let Stato::Pronto(r) = self.stato() {
                    self.imposta(Stato::NonRiparte { release: r, motivo: e.to_string() });
                }
                Err(e.to_string())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn cartella_di_prova(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aimpro-test-agg-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn la_build_viene_dal_tag() {
        assert_eq!(build_dal_tag("v0.1.0-build.11"), Some(11));
        assert_eq!(build_dal_tag("v1.2.3-build.250"), Some(250));
        assert_eq!(build_dal_tag("v0.1.0"), None);
        assert_eq!(build_dal_tag("v0.1.0-build."), None);
        assert_eq!(build_dal_tag("v0.1.0-build.1x"), None);
        assert_eq!(build_dal_tag("v0.1.0-build.0"), None);
    }

    const RISPOSTA: &str = r#"{
        "tag_name": "v0.1.0-build.11", "draft": false, "prerelease": false, "name": "aimpro 0.1.0 (build 11)",
        "assets": [
            {"name": "aimpro.exe", "size": 9000000,
             "browser_download_url": "https://github.com/Carlo-Falanga/aimpro/releases/download/v0.1.0-build.11/aimpro.exe"},
            {"name": "aimpro.exe.sha256", "size": 77,
             "browser_download_url": "https://github.com/Carlo-Falanga/aimpro/releases/download/v0.1.0-build.11/aimpro.exe.sha256"}
        ]
    }"#;

    #[test]
    fn legge_la_risposta_di_github() {
        let r = leggi_release(RISPOSTA).unwrap();
        assert_eq!(r.build, 11);
        assert_eq!(r.tag, "v0.1.0-build.11");
        assert!(r.url_exe.ends_with("/v0.1.0-build.11/aimpro.exe"));
        assert!(r.url_sha.ends_with("/v0.1.0-build.11/aimpro.exe.sha256"));
    }

    #[test]
    fn una_release_senza_i_file_o_rovinata_non_vale() {
        let senza_sha = RISPOSTA.replace("\"aimpro.exe.sha256\"", "\"altro.txt\"");
        assert!(leggi_release(&senza_sha).is_err());
        assert!(leggi_release("{\"message\": \"API rate limit exceeded\"}").is_err());
        assert!(leggi_release("<html>").is_err());
        assert!(leggi_release(&RISPOSTA.replace("\"prerelease\": false", "\"prerelease\": true")).is_err());
        let in_chiaro = RISPOSTA.replace("https://github.com", "http://github.com");
        assert_eq!(leggi_release(&in_chiaro).is_ok(), API_PROVA.is_some(), "http solo nelle build di prova");
    }

    #[test]
    fn si_aggiorna_solo_da_una_build_nota_e_piu_vecchia() {
        let r = leggi_release(RISPOSTA).unwrap();
        assert!(da_aggiornare(Some(9), &r));
        assert!(!da_aggiornare(Some(11), &r));
        assert!(!da_aggiornare(Some(12), &r));
        assert!(!da_aggiornare(None, &r), "build locale: niente aggiornamenti");
    }

    #[test]
    fn sha256_dei_vettori_noti() {
        assert_eq!(
            esadecimale(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            esadecimale(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn legge_il_file_sha256_della_release() {
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha_atteso(&format!("{h}  aimpro.exe\r\n")), Some(sha256(b"abc")));
        assert_eq!(sha_atteso(&format!("\u{feff}{}  aimpro.exe", h.to_uppercase())), Some(sha256(b"abc")));
        assert_eq!(sha_atteso("abc  aimpro.exe"), None);
        assert_eq!(sha_atteso(""), None);
        assert_eq!(sha_atteso(&format!("{}zz  aimpro.exe", &h[..62])), None);
    }

    #[test]
    fn il_controllo_rifiuta_sha_diverso_e_file_non_eseguibili() {
        let exe = b"MZ un eseguibile finto".to_vec();
        let giusto = format!("{}  aimpro.exe", esadecimale(&sha256(&exe)));
        assert_eq!(controlla(&exe, &giusto), Ok(()));
        let sbagliato = format!("{}  aimpro.exe", esadecimale(&sha256(b"altro")));
        assert!(controlla(&exe, &sbagliato).unwrap_err().contains("SHA-256"));
        let non_exe = b"<html>404</html>".to_vec();
        let suo = format!("{}  aimpro.exe", esadecimale(&sha256(&non_exe)));
        assert!(controlla(&non_exe, &suo).is_err());
    }

    #[test]
    fn la_sostituzione_mette_il_nuovo_e_tiene_il_vecchio_accanto() {
        let d = cartella_di_prova("sostituisci");
        let exe = d.join("aimpro.exe");
        std::fs::write(&exe, b"MZ vecchio").unwrap();
        sostituisci(&exe, b"MZ nuovo").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ nuovo");
        assert_eq!(std::fs::read(percorso_vecchio(&exe)).unwrap(), b"MZ vecchio");
        assert!(!d.join("aimpro.exe.nuovo").exists());
        // Un secondo aggiornamento trova il .vecchio di prima e lo sostituisce.
        sostituisci(&exe, b"MZ ancora piu nuovo").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ ancora piu nuovo");
        assert_eq!(std::fs::read(percorso_vecchio(&exe)).unwrap(), b"MZ nuovo");
        assert!(pulisci_vecchio(&exe, 1));
        assert!(!percorso_vecchio(&exe).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn se_la_sostituzione_non_riesce_resta_il_vecchio() {
        let d = cartella_di_prova("non-riesce");
        let exe = d.join("aimpro.exe");
        std::fs::write(&exe, b"MZ vecchio").unwrap();
        // Un .nuovo che è una cartella: la scrittura non riesce.
        std::fs::create_dir_all(d.join("aimpro.exe.nuovo")).unwrap();
        let e = sostituisci(&exe, b"MZ nuovo").unwrap_err();
        assert!(e.contains("non è scrivibile"), "{e}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ vecchio");
        std::fs::remove_dir_all(d.join("aimpro.exe.nuovo")).unwrap();
        // Un .vecchio che non si cancella (una cartella non vuota): niente cambia.
        std::fs::create_dir_all(d.join("aimpro.exe.vecchio").join("dentro")).unwrap();
        assert!(sostituisci(&exe, b"MZ nuovo").is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ vecchio");
        assert!(!d.join("aimpro.exe.nuovo").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn durante_il_giro_nessuna_richiesta_parte() {
        let _uno_alla_volta = rete::PROVE_DI_RETE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        nel_giro(true);
        let e = rete::scarica("http://127.0.0.1:9/", None, 16, &mut |_, _| {});
        nel_giro(false);
        assert_eq!(e, Err(rete::ANNULLATA.to_string()));
    }

    #[test]
    fn senza_vecchio_la_pulizia_non_fa_niente() {
        let d = cartella_di_prova("pulizia");
        assert!(pulisci_vecchio(&d.join("aimpro.exe"), 1));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn una_build_locale_non_ha_numero() {
        // I test girano senza AIMPRO_BUILD (in locale) o con il numero della run (in CI): mai zero.
        match option_env!("AIMPRO_BUILD") {
            None => assert_eq!(build_attuale(), None),
            Some(_) => assert!(build_attuale().is_some_and(|n| n > 0)),
        }
    }
}
