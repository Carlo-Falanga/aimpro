//! Conta le allocazioni di memoria di tutto il processo (il nostro codice, wgpu, winit, egui), così il banco di
//! prova e il pannello dicono quante ne fa un frame. Costa un'addizione atomica per allocazione.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Contatore;

static ALLOCAZIONI: AtomicU64 = AtomicU64::new(0);

// SAFETY: passa tutto all'allocatore di sistema, aggiungendo solo un contatore.
unsafe impl GlobalAlloc for Contatore {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCAZIONI.fetch_add(1, Ordering::Relaxed);
        // SAFETY: stesso contratto di GlobalAlloc::alloc.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCAZIONI.fetch_add(1, Ordering::Relaxed);
        // SAFETY: stesso contratto di GlobalAlloc::alloc_zeroed.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCAZIONI.fetch_add(1, Ordering::Relaxed);
        // SAFETY: stesso contratto di GlobalAlloc::realloc.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: stesso contratto di GlobalAlloc::dealloc.
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// Allocazioni (comprese le riallocazioni) dall'avvio del processo.
pub fn totale() -> u64 {
    ALLOCAZIONI.load(Ordering::Relaxed)
}
