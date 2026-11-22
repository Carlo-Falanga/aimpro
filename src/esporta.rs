//! "Esporta CSV": lo storico in un file per Excel, una riga per giro, nella cartella `aimpro` dentro i Documenti
//! di Windows (quella vera, anche se è stata spostata, letta con `SHGetKnownFolderPath`).
//!
//! Scelte per Excel in italiano: separatore punto e virgola, decimali con la virgola, date `GG/MM/AAAA hh:mm:ss`,
//! UTF-8 con il BOM (senza, Excel legge male le lettere accentate), righe chiuse da CRLF.

use std::path::{Path, PathBuf};

use crate::storico::{GiroSalvato, Ora};

const INTESTAZIONE: [&str; 16] = [
    "data",
    "scenario",
    "durata (s)",
    "colpi",
    "errori",
    "precisione (%)",
    "tempo medio (ms)",
    "overshoot",
    "gioco",
    "sensibilità",
    "cm/360",
    "FOV",
    "DPI",
    "vsync",
    "limite fps",
    "versione",
];

/// Un numero con al più `cifre` decimali, senza zeri in coda, con la virgola.
fn decimale(v: f64, cifre: usize) -> String {
    let mut t = format!("{v:.cifre$}");
    if t.contains('.') {
        t = t.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if t == "-0" {
        t = "0".to_string();
    }
    t.replace('.', ",")
}

fn campo(t: &str) -> String {
    if t.contains([';', '"', '\n', '\r']) { format!("\"{}\"", t.replace('"', "\"\"")) } else { t.to_string() }
}

/// `2026-11-22 14:03:05` diventa `22/11/2026 14:03:05`, che Excel in italiano legge come data.
fn data_per_excel(d: &str) -> String {
    match (d.get(0..4), d.get(5..7), d.get(8..10), d.get(10..)) {
        (Some(a), Some(m), Some(g), Some(resto)) if d.len() >= 10 => format!("{g}/{m}/{a}{resto}"),
        _ => d.to_string(),
    }
}

pub fn csv(giri: &[GiroSalvato]) -> String {
    let mut s = String::from("\u{feff}");
    s.push_str(&INTESTAZIONE.join(";"));
    s.push_str("\r\n");
    for g in giri {
        let i = &g.impostazioni;
        let riga = [
            campo(&data_per_excel(&g.data)),
            campo(&g.scenario),
            decimale(g.durata_s, 3),
            g.colpi.to_string(),
            g.errori.to_string(),
            g.precisione.map_or(String::new(), |p| decimale(p * 100.0, 1)),
            g.tempo_medio_ms.map_or(String::new(), |t| decimale(t, 1)),
            g.overshoot.to_string(),
            i.gioco.nome().to_string(),
            decimale(i.sensibilita, 6),
            decimale(i.cm_360, 2),
            decimale(i.fov, 2),
            decimale(i.dpi, 0),
            if i.vsync { "sì" } else { "no" }.to_string(),
            if i.limite_fps == 0 { "nessuno".to_string() } else { i.limite_fps.to_string() },
            campo(&i.versione),
        ];
        s.push_str(&riga.join(";"));
        s.push_str("\r\n");
    }
    s
}

/// La cartella Documenti dell'utente; se Windows non la dà, `%USERPROFILE%\Documents`.
pub fn documenti() -> PathBuf {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: GUID costante; il testo restituito si copia e si libera con CoTaskMemFree, come chiede la funzione.
    let letta = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None).ok().and_then(|p| {
            let t = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            t
        })
    };
    letta.map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("USERPROFILE").map_or_else(|| PathBuf::from("."), PathBuf::from).join("Documents")
    })
}

/// Scrive `aimpro-giri-<AAAAMMGG-hhmmss>.csv` nella cartella e ne dice il percorso.
pub fn esporta(giri: &[GiroSalvato], cartella: &Path, ora: Ora) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(cartella)?;
    let base = format!("aimpro-giri-{}", ora.compatta());
    let mut percorso = cartella.join(format!("{base}.csv"));
    let mut n = 2;
    while percorso.exists() {
        percorso = cartella.join(format!("{base}-{n}.csv"));
        n += 1;
    }
    std::fs::write(&percorso, csv(giri))?;
    Ok(percorso)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::storico::test::{cartella_di_prova, giro_di_prova, ora};

    #[test]
    fn una_riga_per_giro_per_excel_in_italiano() {
        let mut a = giro_di_prova(42, 1);
        a.precisione = Some(42.0 / 45.0);
        a.tempo_medio_ms = Some(702.26);
        let mut b = giro_di_prova(0, 2);
        b.precisione = None;
        b.tempo_medio_ms = None;
        b.impostazioni.vsync = true;
        b.impostazioni.limite_fps = 240;
        let t = csv(&[a, b]);
        assert!(t.starts_with('\u{feff}'), "BOM per Excel");
        let righe: Vec<&str> = t.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert_eq!(righe.len(), 4, "intestazione, due giri e la riga vuota finale: {righe:?}");
        assert_eq!(righe[3], "");
        assert_eq!(righe[0].split(';').count(), 16);
        let c: Vec<&str> = righe[1].split(';').collect();
        assert_eq!(c.len(), 16);
        assert_eq!(c[0], "22/11/2026 14:03:05");
        assert_eq!(&c[1..6], ["scatto", "30", "42", "3", "93,3"]);
        assert_eq!(c[6], "702,3");
        assert_eq!(c[7], "4");
        assert_eq!(c[8], "CS2");
        assert_eq!(c[13], "no");
        assert_eq!(c[14], "nessuno");
        assert!(c[10].contains(','), "cm/360 con la virgola: {}", c[10]);
        let c: Vec<&str> = righe[2].split(';').collect();
        assert_eq!((c[5], c[6], c[13], c[14]), ("", "", "sì", "240"));
    }

    #[test]
    fn decimali_con_la_virgola_senza_zeri_in_coda() {
        assert_eq!(decimale(1.5, 3), "1,5");
        assert_eq!(decimale(30.0, 3), "30");
        assert_eq!(decimale(0.000_04, 2), "0");
        assert_eq!(decimale(51.958_3, 2), "51,96");
    }

    #[test]
    fn i_campi_col_punto_e_virgola_vanno_tra_virgolette() {
        assert_eq!(campo("a;b"), "\"a;b\"");
        assert_eq!(campo("di \"x\""), "\"di \"\"x\"\"\"");
        assert_eq!(campo("scatto"), "scatto");
    }

    #[test]
    fn esporta_scrive_il_file_e_ne_dice_il_percorso() {
        let d = cartella_di_prova("csv").join("aimpro");
        let p = esporta(&[giro_di_prova(1, 1)], &d, ora(9)).unwrap();
        assert_eq!(p, d.join("aimpro-giri-20261122-140309.csv"));
        let q = esporta(&[giro_di_prova(1, 1)], &d, ora(9)).unwrap();
        assert_ne!(p, q, "un secondo file nello stesso secondo non sovrascrive il primo");
        assert!(std::fs::read_to_string(&p).unwrap().contains("scatto"));
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn la_cartella_documenti_e_un_percorso_assoluto() {
        // Windows la dà anche se la cartella non esiste ancora: `esporta` la crea.
        assert!(documenti().is_absolute(), "{:?}", documenti());
    }
}
