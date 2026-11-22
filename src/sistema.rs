//! Piccole chiamate a Windows: tempo di CPU del processo, finestra in primo piano, input simulato per il banco.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{FILETIME, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX2};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentThread, GetCurrentThreadId, GetProcessTimes,
    GetThreadTimes,
};
use windows::Win32::System::WindowsProgramming::{QueryProcessCycleTime, QueryThreadCycleTime};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, ClipCursor, GetClientRect, GetClipCursor, GetCursorPos, GetForegroundWindow,
    GetWindowThreadProcessId, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW, PostMessageW,
    SetCursorPos, SetForegroundWindow, WM_MOUSEMOVE,
};
use windows::core::HSTRING;

use crate::config;
use crate::tempo;

// La CPU si misura in due modi, perché nessuno dei due è esatto da solo:
// - a tempo (GetProcessTimes / GetThreadTimes): Windows lo aggiorna a scatti del timer di sistema (circa 15,6 ms)
//   e sbaglia con thread che si svegliano centinaia di volte al secondo, come il giro con il limite di fps;
// - a cicli (QueryProcessCycleTime / QueryThreadCycleTime): esatto nel conteggio, ma i cicli si convertono in
//   tempo con la frequenza calibrata a pieno carico; se il core rallenta, il tempo risulta più basso del vero.

fn cicli_thread() -> u64 {
    let mut c = 0u64;
    // SAFETY: pseudo-handle del thread corrente e puntatore valido.
    let _ = unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut c) };
    c
}

fn cicli_processo() -> u64 {
    let mut c = 0u64;
    // SAFETY: pseudo-handle del processo corrente e puntatore valido.
    let _ = unsafe { QueryProcessCycleTime(GetCurrentProcess(), &mut c) };
    c
}

/// Cicli per millisecondo, misurati una volta girando a vuoto per 3 x 20 ms (si tiene il massimo: un giro
/// interrotto dallo scheduler conta meno cicli, mai di più).
pub fn cicli_per_ms() -> f64 {
    static F: OnceLock<f64> = OnceLock::new();
    *F.get_or_init(|| {
        let mut massimo = 0.0f64;
        for _ in 0..3 {
            let (q0, c0) = (tempo::qpc(), cicli_thread());
            let fine = q0 + tempo::tick(20.0);
            while tempo::qpc() < fine {
                std::hint::spin_loop();
            }
            let (q1, c1) = (tempo::qpc(), cicli_thread());
            massimo = massimo.max((c1 - c0) as f64 / tempo::ms(q1 - q0));
        }
        massimo.max(1.0)
    })
}

/// Millisecondi di CPU, misurati a tempo e dai cicli.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cpu {
    pub tempo_ms: f64,
    pub cicli_ms: f64,
}

impl std::ops::Sub for Cpu {
    type Output = Cpu;
    fn sub(self, b: Cpu) -> Cpu {
        Cpu { tempo_ms: self.tempo_ms - b.tempo_ms, cicli_ms: self.cicli_ms - b.cicli_ms }
    }
}

fn in_ms(f: FILETIME) -> f64 {
    ((u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)) as f64 / 10_000.0
}

/// La CPU usata dal processo (tutti i thread).
pub fn cpu_processo() -> Cpu {
    let (mut c, mut e, mut k, mut u) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: pseudo-handle del processo corrente e puntatori validi.
    let tempo_ms = match unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) } {
        Ok(()) => in_ms(k) + in_ms(u),
        Err(_) => 0.0,
    };
    Cpu { tempo_ms, cicli_ms: cicli_processo() as f64 / cicli_per_ms() }
}

/// La CPU usata dal thread che chiama.
pub fn cpu_thread() -> Cpu {
    let (mut c, mut e, mut k, mut u) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: pseudo-handle del thread corrente e puntatori validi.
    let tempo_ms = match unsafe { GetThreadTimes(GetCurrentThread(), &mut c, &mut e, &mut k, &mut u) } {
        Ok(()) => in_ms(k) + in_ms(u),
        Err(_) => 0.0,
    };
    Cpu { tempo_ms, cicli_ms: cicli_thread() as f64 / cicli_per_ms() }
}

/// La memoria del processo, in byte.
#[derive(Clone, Copy, Debug, Default)]
pub struct Memoria {
    /// Il set di lavoro privato: le pagine in RAM che non condivide con altri processi. È il numero della colonna
    /// "Memoria" di Gestione attività.
    pub privata: usize,
    /// La memoria impegnata privata (commit): comprende anche le pagine non in RAM.
    pub impegnata: usize,
}

impl Memoria {
    pub fn mb(b: usize) -> f64 {
        b as f64 / (1024.0 * 1024.0)
    }
}

/// La memoria del processo adesso.
pub fn memoria() -> Memoria {
    let mut m = PROCESS_MEMORY_COUNTERS_EX2 {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        ..Default::default()
    };
    // SAFETY: pseudo-handle del processo corrente; la struttura EX2 comincia come PROCESS_MEMORY_COUNTERS e `cb`
    // dice a Windows quanto è grande (Windows 10 1809 e successivi).
    let ok = unsafe {
        GetProcessMemoryInfo(GetCurrentProcess(), (&mut m as *mut PROCESS_MEMORY_COUNTERS_EX2).cast(), m.cb)
    };
    if ok.is_err() {
        return Memoria::default();
    }
    Memoria { privata: m.PrivateWorkingSetSize, impegnata: m.PrivateUsage }
}

/// Le tappe dell'avvio con la memoria del processo in quel momento: il banco le riporta, per vedere da dove viene
/// la memoria (finestra, DirectX 12, swapchain, egui...).
static TAPPE: std::sync::Mutex<Vec<(&'static str, Memoria)>> = std::sync::Mutex::new(Vec::new());

/// Segna una tappa dell'avvio. Alloca: solo fuori dal ciclo dei frame.
pub fn segna_memoria(tappa: &'static str) {
    let m = memoria();
    if let Ok(mut t) = TAPPE.lock() {
        t.push((tappa, m));
    }
}

/// Le tappe segnate finora.
pub fn tappe_memoria() -> Vec<(&'static str, Memoria)> {
    TAPPE.lock().map(|t| t.clone()).unwrap_or_default()
}

/// La finestra `hwnd` è quella in primo piano?
pub fn in_primo_piano(hwnd: isize) -> bool {
    // SAFETY: nessun parametro.
    unsafe { GetForegroundWindow() }.0 as isize == hwnd
}

fn mouse(dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// Simula un movimento relativo del mouse, solo se `hwnd` è in primo piano. Dice se l'ha fatto.
pub fn simula_movimento(hwnd: isize, dx: i32, dy: i32) -> bool {
    if !in_primo_piano(hwnd) {
        return false;
    }
    // SAFETY: array di INPUT validi e dimensione giusta.
    unsafe { SendInput(&[mouse(dx, dy, MOUSEEVENTF_MOVE)], std::mem::size_of::<INPUT>() as i32) == 1 }
}

/// Simula un clic sinistro, solo se `hwnd` è in primo piano: un clic simulato non deve mai finire in un altro
/// programma.
pub fn simula_clic(hwnd: isize) -> bool {
    if !in_primo_piano(hwnd) {
        return false;
    }
    let eventi = [mouse(0, 0, MOUSEEVENTF_LEFTDOWN), mouse(0, 0, MOUSEEVENTF_LEFTUP)];
    // SAFETY: array di INPUT validi e dimensione giusta.
    unsafe { SendInput(&eventi, std::mem::size_of::<INPUT>() as i32) == 2 }
}

/// Porta `hwnd` in primo piano. Windows lo concede solo a chi ha l'input in quel momento: per un attimo il
/// thread si aggancia all'input della finestra che ha il primo piano (di solito la console che ha lanciato
/// aimpro), così la richiesta passa anche senza un clic. Dice se ci è riuscito.
pub fn porta_in_primo_piano(hwnd: isize) -> bool {
    if hwnd == 0 || in_primo_piano(hwnd) {
        return hwnd != 0;
    }
    let h = HWND(hwnd as *mut _);
    // SAFETY: handle di finestre esistenti (o nulli, che le funzioni rifiutano) e id di thread di Windows.
    unsafe {
        let davanti = GetForegroundWindow();
        let suo = GetWindowThreadProcessId(davanti, None);
        let mio = GetCurrentThreadId();
        let agganciato = suo != 0 && suo != mio && AttachThreadInput(mio, suo, true).as_bool();
        let _ = BringWindowToTop(h);
        let _ = SetForegroundWindow(h);
        if agganciato {
            let _ = AttachThreadInput(mio, suo, false);
        }
    }
    in_primo_piano(hwnd)
}

/// Il rettangolo, in coordinate dello schermo, in cui tenere il cursore: un punto al centro della finestra
/// (`lato` 1) o un quadrato di `lato` pixel intorno al centro.
fn rettangolo_centrale(hwnd: isize, lato: i32) -> Option<RECT> {
    let h = HWND(hwnd as *mut _);
    let mut r = RECT::default();
    // SAFETY: handle della nostra finestra e puntatori validi.
    unsafe { GetClientRect(h, &mut r) }.ok()?;
    let mut c = POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 };
    // SAFETY: come sopra.
    if !unsafe { ClientToScreen(h, &mut c) }.as_bool() {
        return None;
    }
    let m = lato / 2;
    Some(RECT { left: c.x - m, top: c.y - m, right: c.x - m + lato.max(1), bottom: c.y - m + lato.max(1) })
}

/// Tiene il cursore di Windows fermo al centro della finestra (`lato` 1), o dentro un quadrato di `lato` pixel.
/// Il Raw Input riceve comunque i count del mouse, che non dipendono dal cursore: nel giro il cursore non si
/// muove e non può finire su un altro schermo. Windows toglie il blocco quando la finestra perde il primo
/// piano: va rimesso alla ripresa.
pub fn blocca_cursore(hwnd: isize, lato: i32) -> bool {
    let Some(r) = rettangolo_centrale(hwnd, lato) else {
        return false;
    };
    // SAFETY: rettangolo valido.
    unsafe {
        let _ = SetCursorPos((r.left + r.right) / 2, (r.top + r.bottom) / 2);
        ClipCursor(Some(&r)).is_ok()
    }
}

/// Dov'è il cursore di Windows, in coordinate dello schermo.
pub fn posizione_cursore() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    // SAFETY: puntatore valido.
    unsafe { GetCursorPos(&mut p) }.ok().map(|()| (p.x, p.y))
}

/// Rimette il cursore di Windows in un punto dello schermo.
pub fn sposta_cursore(x: i32, y: i32) {
    // SAFETY: nessun puntatore.
    let _ = unsafe { SetCursorPos(x, y) };
}

/// Il cursore torna libero.
pub fn libera_cursore() {
    // SAFETY: None toglie il blocco.
    let _ = unsafe { ClipCursor(None) };
}

/// Il cursore è ancora bloccato come l'ha messo `blocca_cursore(hwnd, lato)`?
pub fn cursore_bloccato(hwnd: isize, lato: i32) -> bool {
    let (Some(atteso), mut ora) = (rettangolo_centrale(hwnd, lato), RECT::default()) else {
        return false;
    };
    // SAFETY: puntatore valido.
    unsafe { GetClipCursor(&mut ora) }.is_ok() && ora == atteso
}

/// Manda alla finestra un movimento del mouse come messaggio (WM_MOUSEMOVE), nel punto (`x`, `y`) dell'area
/// della finestra: il menu lo riceve come un movimento vero, ma il cursore di Windows non si sposta.
pub fn manda_movimento_alla_finestra(hwnd: isize, x: i32, y: i32) -> bool {
    if !in_primo_piano(hwnd) {
        return false;
    }
    let lparam = ((y as u32 & 0xFFFF) << 16 | (x as u32 & 0xFFFF)) as isize;
    // SAFETY: handle della nostra finestra; il messaggio non porta puntatori.
    unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), WM_MOUSEMOVE, WPARAM(0), LPARAM(lparam)) }.is_ok()
}

/// Il centro dell'area della finestra, in coordinate della finestra.
pub fn centro_finestra(hwnd: isize) -> Option<(i32, i32)> {
    let mut r = RECT::default();
    // SAFETY: handle della nostra finestra e puntatore valido.
    unsafe { GetClientRect(HWND(hwnd as *mut _), &mut r) }.ok()?;
    Some(((r.left + r.right) / 2, (r.top + r.bottom) / 2))
}

/// C'è stato un errore fatale (all'avvio o col ciclo degli eventi fermato da un errore): `main` esce con codice 1
/// (lo stampa `presentmon.ps1`). Dopo un panic, nella release (`panic = "abort"`), il processo si interrompe con il
/// codice di Windows e non arriva all'1; in debug un panic nel thread principale esce con 101.
pub static ERRORE_FATALE: AtomicBool = AtomicBool::new(false);

/// Un errore che ferma il programma (all'avvio, il ciclo degli eventi fermato o un panic): nella release non c'è
/// la console, quindi va in `%APPDATA%\aimpro\errore.txt` e in una finestra di messaggio, sopra lo schermo intero
/// e col cursore libero (nel giro è bloccato al centro e non si potrebbe premere OK). Col banco niente finestra
/// (nessuno la chiuderebbe): basta il file.
pub fn errore_fatale(testo: &str, banco: bool) {
    ERRORE_FATALE.store(true, Ordering::SeqCst);
    // Anche col banco: con panic = "abort" il Drop del banco, che lo libera, non gira.
    libera_cursore();
    eprintln!("aimpro: {testo}");
    let file = config::percorso_predefinito().with_file_name("errore.txt");
    if let Some(cartella) = file.parent() {
        let _ = std::fs::create_dir_all(cartella);
    }
    let _ = std::fs::write(&file, format!("{testo}\n"));
    if !banco {
        let messaggio = format!(
            "aimpro si è fermato per un errore:\n\n{testo}\n\nIl testo è anche in {}",
            file.display()
        );
        // SAFETY: stringhe valide per tutta la chiamata, nessuna finestra madre.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(messaggio),
                &HSTRING::from("aimpro"),
                MB_OK | MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND,
            )
        };
    }
}

/// L'handle della finestra come intero (per passarlo fra i thread).
pub fn hwnd_di(finestra: &winit::window::Window) -> isize {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match finestra.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}
