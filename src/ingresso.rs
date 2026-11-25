//! Il mouse da Raw Input di Windows, su un thread suo.
//!
//! - Il thread ha una finestra solo-messaggi (`HWND_MESSAGE`) registrata per il mouse (pagina 0x01, uso 0x02)
//!   con `RIDEV_INPUTSINK`; resta fermo in `GetMessageW` e si sveglia a ogni evento del mouse. Un thread a parte
//!   vuol dire che gli eventi non aspettano il giro dei messaggi della finestra del gioco.
//! - I count arrivano grezzi (`lLastX`, `lLastY` con `MOUSE_MOVE_RELATIVE`): niente accelerazione né
//!   "precisione del puntatore" di Windows. Ogni evento prende l'istante QPC in cui il thread lo riceve.
//! - I count si sommano in contatori atomici; il ciclo dei frame li legge all'ultimo momento, subito prima di
//!   calcolare la visuale. Ogni clic si mette in coda con l'istante e con i contatori di quel momento, così si
//!   valuta con la visuale che c'era al clic, anche se il frame arriva dopo. Nella stessa coda va anche il tasto che
//!   torna su: il tracking conta il tempo col tasto premuto.
//! - Fuori dal giro la registrazione si toglie (`RIDEV_REMOVE`): nei menu il thread non si sveglia.
//!
//! Il tratto prima del thread (mouse, USB, driver, Windows) non si vede da qui: lo misura PresentMon.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG, PostMessageW,
    RI_MOUSE_LEFT_BUTTON_DOWN, RI_MOUSE_LEFT_BUTTON_UP, RegisterClassExW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_INPUT, WNDCLASSEXW,
};
use windows::core::w;

use crate::tempo;

const CAPACITA_CLIC: usize = 256;
const WM_ATTIVA: u32 = WM_APP + 1;

/// Il tasto sinistro che va giù (un clic) o torna su, con i contatori dei count nel suo istante. Il tasto su serve al
/// tracking, che conta il tempo col tasto premuto.
#[derive(Clone, Copy, Debug, Default)]
pub struct Clic {
    pub qpc: i64,
    pub dx: i64,
    pub dy: i64,
    pub giu: bool,
}

struct CodaClic {
    voci: [Clic; CAPACITA_CLIC],
    n: usize,
}

/// Quello che il ciclo dei frame legge all'ultimo momento.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lettura {
    /// Count totali dall'avvio.
    pub dx: i64,
    pub dy: i64,
    /// Istante del primo e dell'ultimo evento di movimento non ancora letti (0 se nessuno).
    pub primo: i64,
    pub ultimo: i64,
}

pub struct Ingresso {
    attivo: AtomicBool,
    dx: AtomicI64,
    dy: AtomicI64,
    primo_in_attesa: AtomicI64,
    ultimo_in_attesa: AtomicI64,
    clic: Mutex<CodaClic>,
    /// Eventi del mouse ricevuti mentre attivo.
    pub eventi: AtomicU64,
    /// Eventi con coordinate assolute (desktop remoto, tavolette): ignorati.
    pub assoluti: AtomicU64,
    /// Eventi senza dispositivo (input simulato, per esempio dal banco di prova).
    pub simulati: AtomicU64,
    /// Clic persi perché la coda era piena.
    pub clic_persi: AtomicU32,
    finestra: AtomicI64,
}

/// Quello che legge la procedura della finestra (impostato dal thread prima di creare la finestra).
static CONDIVISO: OnceLock<Arc<Ingresso>> = OnceLock::new();
/// Il thread si avvia una volta sola per processo.
static AVVIATO: Mutex<Option<Arc<Ingresso>>> = Mutex::new(None);

impl Ingresso {
    /// Avvia il thread del Raw Input (una volta sola per processo).
    pub fn avvia() -> Arc<Ingresso> {
        let mut avviato = AVVIATO.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = avviato.as_ref() {
            return i.clone();
        }
        let i = {
            {
                let i = Arc::new(Ingresso {
                    attivo: AtomicBool::new(false),
                    dx: AtomicI64::new(0),
                    dy: AtomicI64::new(0),
                    primo_in_attesa: AtomicI64::new(0),
                    ultimo_in_attesa: AtomicI64::new(0),
                    clic: Mutex::new(CodaClic { voci: [Clic::default(); CAPACITA_CLIC], n: 0 }),
                    eventi: AtomicU64::new(0),
                    assoluti: AtomicU64::new(0),
                    simulati: AtomicU64::new(0),
                    clic_persi: AtomicU32::new(0),
                    finestra: AtomicI64::new(0),
                });
                let pronto = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
                let pronto2 = pronto.clone();
                let per_thread = i.clone();
                std::thread::Builder::new()
                    .name("aimpro-raw-input".into())
                    .spawn(move || ciclo_thread(per_thread, pronto2))
                    .expect("thread del Raw Input");
                // Aspetta che la finestra esista, così `imposta_attivo` ha dove mandare il messaggio.
                let (lock, cv) = &*pronto;
                let mut ok = lock.lock().unwrap_or_else(|e| e.into_inner());
                while !*ok {
                    ok = cv.wait(ok).unwrap_or_else(|e| e.into_inner());
                }
                i
            }
        };
        *avviato = Some(i.clone());
        i
    }

    /// Accende o spegne la lettura del mouse (giro in corso e finestra in primo piano).
    pub fn imposta_attivo(&self, attivo: bool) {
        if self.attivo.swap(attivo, Ordering::AcqRel) == attivo {
            return;
        }
        if !attivo {
            self.primo_in_attesa.store(0, Ordering::Release);
            if let Ok(mut c) = self.clic.lock() {
                c.n = 0;
            }
        }
        let hwnd = HWND(self.finestra.load(Ordering::Acquire) as *mut _);
        // SAFETY: la finestra vive quanto il processo; il messaggio la fa registrare o togliere dal suo thread.
        let _ = unsafe { PostMessageW(Some(hwnd), WM_ATTIVA, WPARAM(usize::from(attivo)), LPARAM(0)) };
    }

    /// I count totali e gli istanti degli eventi non ancora letti. Non alloca.
    #[inline]
    pub fn leggi(&self) -> Lettura {
        let primo = self.primo_in_attesa.swap(0, Ordering::AcqRel);
        let ultimo = self.ultimo_in_attesa.load(Ordering::Acquire);
        Lettura { dx: self.dx.load(Ordering::Acquire), dy: self.dy.load(Ordering::Acquire), primo, ultimo }
    }

    /// Sposta i clic in coda in `dest` e dice quanti sono. Non alloca. Va chiamata prima di `leggi`, così
    /// ogni clic ha contatori non oltre quelli letti.
    #[inline]
    pub fn prendi_clic(&self, dest: &mut [Clic; CAPACITA_CLIC]) -> usize {
        let Ok(mut c) = self.clic.lock() else { return 0 };
        let n = c.n;
        dest[..n].copy_from_slice(&c.voci[..n]);
        c.n = 0;
        n
    }

    fn evento(&self, ora: i64, dx: i32, dy: i32, sinistro_giu: bool, sinistro_su: bool) {
        if !self.attivo.load(Ordering::Relaxed) {
            return;
        }
        self.eventi.fetch_add(1, Ordering::Relaxed);
        if dx != 0 || dy != 0 {
            self.dx.fetch_add(i64::from(dx), Ordering::AcqRel);
            self.dy.fetch_add(i64::from(dy), Ordering::AcqRel);
            let _ = self.primo_in_attesa.compare_exchange(0, ora, Ordering::AcqRel, Ordering::Relaxed);
            self.ultimo_in_attesa.store(ora, Ordering::Release);
        }
        // Giù e su nello stesso evento (succede col mouse simulato): prima giù, poi su.
        for (vero, giu) in [(sinistro_giu, true), (sinistro_su, false)] {
            if vero && let Ok(mut c) = self.clic.lock() {
                if c.n < CAPACITA_CLIC {
                    let n = c.n;
                    c.voci[n] = Clic {
                        qpc: ora,
                        dx: self.dx.load(Ordering::Acquire),
                        dy: self.dy.load(Ordering::Acquire),
                        giu,
                    };
                    c.n += 1;
                } else {
                    self.clic_persi.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

fn registra(hwnd: HWND, attivo: bool) {
    let dispositivo = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x02,
        dwFlags: if attivo { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
        hwndTarget: if attivo { hwnd } else { HWND::default() },
    };
    // SAFETY: struttura valida e dimensione giusta.
    let _ = unsafe { RegisterRawInputDevices(&[dispositivo], std::mem::size_of::<RAWINPUTDEVICE>() as u32) };
}

extern "system" fn procedura(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_INPUT => {
            let ora = tempo::qpc();
            let mut dati = RAWINPUT::default();
            let mut dimensione = std::mem::size_of::<RAWINPUT>() as u32;
            // SAFETY: lparam è l'handle HRAWINPUT del messaggio; il buffer è grande quanto un RAWINPUT.
            let letti = unsafe {
                GetRawInputData(
                    HRAWINPUT(lparam.0 as *mut _),
                    RID_INPUT,
                    Some((&mut dati as *mut RAWINPUT).cast()),
                    &mut dimensione,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                )
            };
            if letti != u32::MAX
                && letti > 0
                && dati.header.dwType == RIM_TYPEMOUSE.0
                && let Some(i) = CONDIVISO.get()
            {
                // SAFETY: dwType dice che l'unione contiene il mouse.
                let mouse = unsafe { dati.data.mouse };
                // SAFETY: lettura del campo dei pulsanti dell'unione, sempre inizializzato.
                let pulsanti = u32::from(unsafe { mouse.Anonymous.Anonymous.usButtonFlags });
                if dati.header.hDevice == HANDLE::default() {
                    i.simulati.fetch_add(1, Ordering::Relaxed);
                }
                let (dx, dy) = if mouse.usFlags.0 & MOUSE_MOVE_ABSOLUTE.0 != 0 {
                    i.assoluti.fetch_add(1, Ordering::Relaxed);
                    (0, 0)
                } else {
                    (mouse.lLastX, mouse.lLastY)
                };
                i.evento(
                    ora,
                    dx,
                    dy,
                    pulsanti & RI_MOUSE_LEFT_BUTTON_DOWN != 0,
                    pulsanti & RI_MOUSE_LEFT_BUTTON_UP != 0,
                );
            }
            // SAFETY: per WM_INPUT va chiamata DefWindowProc, che libera i dati dell'evento.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_ATTIVA => {
            registra(hwnd, wparam.0 != 0);
            LRESULT(0)
        }
        // SAFETY: gestione predefinita degli altri messaggi.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn ciclo_thread(ingresso: Arc<Ingresso>, pronto: Arc<(Mutex<bool>, std::sync::Condvar)>) {
    let _ = CONDIVISO.set(ingresso.clone());
    // SAFETY: chiamate Win32 con parametri validi; la classe e la finestra vivono quanto il thread (il processo).
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
        let istanza = GetModuleHandleW(None).unwrap_or_default();
        let classe = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedura),
            hInstance: istanza.into(),
            lpszClassName: w!("aimpro-raw-input"),
            ..Default::default()
        };
        RegisterClassExW(&classe);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("aimpro-raw-input"),
            w!("aimpro raw input"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(istanza.into()),
            None,
        )
        .expect("finestra del Raw Input");
        ingresso.finestra.store(hwnd.0 as i64, Ordering::Release);
        {
            let (lock, cv) = &*pronto;
            *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
            cv.notify_all();
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
}
