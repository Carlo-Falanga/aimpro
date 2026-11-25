//! La schermata Statistiche: andamento nel tempo, giorni e settimane, punti deboli, elenco dei giri e dettaglio di un
//! giro. Disegnata con egui fuori dal giro; i conti stanno in `statistiche.rs` e si rifanno solo quando cambiano
//! scenario, periodo o giri (o passa un minuto, per i periodi che finiscono adesso), non a ogni frame.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, vec2};

use crate::interfaccia::{ACCENTO, AVVISO, Azione, TENUE, pulsante, scheda_larga};
use crate::registro::Bersaglio;
use crate::statistiche::{
    self, COLPI_MINIMI, Direzione, FASCE, FINESTRA_MEDIA, Giorno, Gruppi, Gruppo, Media, Periodo,
    PuntiDeboli, Sessione, Settimane, Sintesi, Statistiche, media_mobile, nome_scenario, per_giorno,
    punti_deboli, sessione, settimane, testo_punteggio,
};
use crate::storico::{GiroSalvato, SCATTO};

const LARGHEZZA: f32 = 880.0;
/// Altezza massima del contenuto che scorre, sotto i controlli in alto.
const ALTEZZA_CONTENUTO: f32 = 560.0;
const CHIARO: Color32 = Color32::from_rgb(216, 217, 220);
/// Punti al massimo in un grafico: oltre, i giri vicini si mediano (più di uno per pixel non si distingue).
pub const PUNTI_GRAFICO: usize = 300;
/// Giorni mostrati nella tabella giorno per giorno.
pub const GIORNI_MOSTRATI: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheda {
    Andamento,
    Giorni,
    PuntiDeboli,
    Giri,
}

impl Scheda {
    const TUTTE: [Scheda; 4] = [Scheda::Andamento, Scheda::Giorni, Scheda::PuntiDeboli, Scheda::Giri];

    fn nome(self) -> &'static str {
        match self {
            Scheda::Andamento => "Andamento",
            Scheda::Giorni => "Per giorno",
            Scheda::PuntiDeboli => "Punti deboli",
            Scheda::Giri => "Giri",
        }
    }
}

/// Un giro aperto dall'elenco: la sua sintesi e, se il file si legge, il giro intero con i suoi gruppi.
pub struct Dettaglio {
    sintesi: Sintesi,
    giro: Option<(GiroSalvato, Gruppi)>,
}

/// Una serie di un grafico, già ridotta: i valori e la loro media mobile.
struct Serie {
    valori: Vec<Option<f64>>,
    media: Vec<Option<f64>>,
}

impl Serie {
    fn nuova(valori: Vec<Option<f64>>) -> Self {
        let media = media_mobile(&valori, FINESTRA_MEDIA);
        Serie { valori: riduci(&valori, PUNTI_GRAFICO), media: riduci(&media, PUNTI_GRAFICO) }
    }
}

/// I conti della schermata per uno scenario e un periodo.
struct Calcolo {
    chiave: (String, Periodo, u64, i64),
    scenari: Vec<String>,
    /// Ci sono giri dello scenario (in tutto lo storico)?
    ha_giri: bool,
    /// I giri del periodo, indici in `Statistiche`, dal più vecchio.
    indici: Vec<usize>,
    record: Option<f64>,
    sessione: Sessione,
    punteggi: Serie,
    precisioni: Serie,
    tempi: Serie,
    giorni: Vec<Giorno>,
    giorni_totali: usize,
    settimane: Option<Settimane>,
    deboli: PuntiDeboli,
}

impl Calcolo {
    fn nuovo(s: &Statistiche, chiave: (String, Periodo, u64, i64), adesso_ms: i64, avvio_ms: i64) -> Self {
        let (scenario, periodo) = (&chiave.0, chiave.1);
        let tutti = s.giri(scenario, Periodo::Tutto, adesso_ms);
        let indici = s.indici(scenario, periodo, adesso_ms);
        let giri: Vec<&Sintesi> = indici.iter().map(|&i| s.giro(i)).collect();
        let mut giorni = per_giorno(&giri);
        let giorni_totali = giorni.len();
        giorni.truncate(GIORNI_MOSTRATI);
        Calcolo {
            scenari: s.scenari(),
            ha_giri: !tutti.is_empty(),
            record: statistiche::record(&tutti),
            sessione: sessione(&tutti, avvio_ms),
            punteggi: Serie::nuova(giri.iter().map(|g| Some(g.punteggio)).collect()),
            precisioni: Serie::nuova(giri.iter().map(|g| g.precisione.map(|p| p * 100.0)).collect()),
            tempi: Serie::nuova(giri.iter().map(|g| g.tempo_medio_ms).collect()),
            giorni,
            giorni_totali,
            settimane: settimane(&tutti, adesso_ms),
            deboli: punti_deboli(&giri),
            indici,
            chiave,
        }
    }
}

/// Cosa si sta guardando nella schermata: resta finché aimpro è aperto.
pub struct Vista {
    pub scheda: Scheda,
    pub scenario: String,
    pub periodo: Periodo,
    pub dettaglio: Option<Dettaglio>,
    calcolo: Option<Calcolo>,
}

impl Default for Vista {
    fn default() -> Self {
        Vista {
            scheda: Scheda::Andamento,
            scenario: SCATTO.to_string(),
            periodo: Periodo::Tutto,
            dettaglio: None,
            calcolo: None,
        }
    }
}

impl Vista {
    /// I conti per lo scenario e il periodo scelti, rifatti solo se qualcosa è cambiato.
    fn aggiorna(&mut self, s: &Statistiche, adesso_ms: i64, avvio_ms: i64) {
        let chiave = (self.scenario.clone(), self.periodo, s.versione, adesso_ms.div_euclid(60_000));
        if self.calcolo.as_ref().is_some_and(|c| c.chiave == chiave) {
            return;
        }
        let c = Calcolo::nuovo(s, chiave, adesso_ms, avvio_ms);
        // Uno scenario senza giri (lo scatto in uno storico che ne ha solo altri) lascia il posto al primo che ne ha.
        if !c.ha_giri
            && let Some(primo) = c.scenari.first().filter(|p| **p != self.scenario)
        {
            self.scenario = primo.clone();
            return self.aggiorna(s, adesso_ms, avvio_ms);
        }
        self.calcolo = Some(c);
    }
}

/// Riduce una serie a `massimo` punti al più, con la media dei valori presenti di ogni tratto.
pub fn riduci(valori: &[Option<f64>], massimo: usize) -> Vec<Option<f64>> {
    let n = valori.len();
    if n <= massimo.max(1) {
        return valori.to_vec();
    }
    (0..massimo)
        .map(|k| {
            let tratto = &valori[k * n / massimo..(k + 1) * n / massimo];
            let presenti: Vec<f64> = tratto.iter().flatten().copied().collect();
            (!presenti.is_empty()).then(|| presenti.iter().sum::<f64>() / presenti.len() as f64)
        })
        .collect()
}

fn percentuale(p: Option<f64>) -> String {
    p.map_or("-".to_string(), |p| format!("{:.1}%", p * 100.0))
}

fn millisecondi(t: Option<f64>) -> String {
    t.map_or("-".to_string(), |t| format!("{t:.0} ms"))
}

fn decimale(v: Option<f64>) -> String {
    v.map_or("-".to_string(), |v| format!("{v:.2}"))
}

fn piccolo(t: impl Into<String>) -> RichText {
    RichText::new(t).size(13.0).color(TENUE)
}

fn titoletto(ui: &mut Ui, t: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(t).size(16.0).strong());
}

/// Il valore in cima a un grafico: un po' sopra il massimo, arrotondato a un numero tondo.
pub fn cima(massimo: f64) -> f64 {
    let m = (massimo * 1.1).max(1e-9);
    let mezzo = 10f64.powf(m.log10().floor()) / 2.0;
    (m / mezzo).ceil() * mezzo
}

/// Lo sfondo, le righe e i valori dell'asse di un grafico; dice il rettangolo dei dati e la funzione dell'altezza.
fn assi(
    ui: &mut Ui,
    altezza: f32,
    alto: f64,
    testo: impl Fn(f64) -> String,
) -> (egui::Response, egui::Painter, Rect, impl Fn(f64) -> f32) {
    let (area, risposta) = ui.allocate_exact_size(vec2(ui.available_width(), altezza), Sense::hover());
    let p = ui.painter_at(area);
    p.rect_filled(area, CornerRadius::same(8), Color32::from_white_alpha(6));
    let dentro = Rect::from_min_max(area.min + vec2(56.0, 12.0), area.max - vec2(14.0, 12.0));
    let y = move |v: f64| dentro.bottom() - (v / alto) as f32 * dentro.height();
    for v in [0.0, alto / 2.0, alto] {
        p.line_segment(
            [Pos2::new(dentro.left(), y(v)), Pos2::new(dentro.right(), y(v))],
            Stroke::new(1.0, Color32::from_white_alpha(14)),
        );
        p.text(
            Pos2::new(dentro.left() - 8.0, y(v)),
            Align2::RIGHT_CENTER,
            testo(v),
            FontId::proportional(12.0),
            TENUE,
        );
    }
    (risposta, p, dentro, y)
}

/// Un grafico a linee: un punto per giro (in arancione), la media mobile (in chiaro) e, se c'è, una linea
/// tratteggiata (il record). `tetto` è il valore più alto possibile (100 per una percentuale).
fn grafico(
    ui: &mut Ui,
    serie: &Serie,
    linea: Option<(f64, String)>,
    tetto: Option<f64>,
    testo: impl Fn(f64) -> String,
) {
    let massimo =
        serie.valori.iter().flatten().copied().chain(linea.as_ref().map(|l| l.0)).fold(0.0, f64::max);
    let alto = tetto.map_or(cima(massimo), |t| cima(massimo).min(t));
    let (_, p, dentro, y) = assi(ui, 130.0, alto, testo);
    if let Some((v, nome)) = linea {
        let yr = y(v);
        p.extend(Shape::dashed_line(
            &[Pos2::new(dentro.left(), yr), Pos2::new(dentro.right(), yr)],
            Stroke::new(1.5, CHIARO),
            6.0,
            4.0,
        ));
        p.text(
            Pos2::new(dentro.right(), yr - 4.0),
            Align2::RIGHT_BOTTOM,
            nome,
            FontId::proportional(12.0),
            CHIARO,
        );
    }
    // Con pochi giri i punti si allargano su tutto il grafico, il più recente sempre a destra.
    let n = serie.valori.len();
    let passo = dentro.width() / n.saturating_sub(1).max(1) as f32;
    let x = |i: usize| if n == 1 { dentro.right() } else { dentro.left() + passo * i as f32 };
    let linee = |v: &[Option<f64>]| -> Vec<Pos2> {
        v.iter().enumerate().filter_map(|(i, v)| v.map(|v| Pos2::new(x(i), y(v)))).collect()
    };
    let punti = linee(&serie.valori);
    if punti.len() > 1 {
        p.add(Shape::line(punti.clone(), Stroke::new(1.5, ACCENTO)));
    }
    if n <= 60 {
        for q in &punti {
            p.circle_filled(*q, 3.0, ACCENTO);
        }
    }
    let m = linee(&serie.media);
    if m.len() > 1 {
        p.add(Shape::line(m, Stroke::new(2.0, CHIARO)));
    }
}

/// Una tabella semplice: intestazione in grigio e righe; `evidenzia` è la riga da colorare.
fn tabella(ui: &mut Ui, id: &str, intestazione: &[&str], righe: &[Vec<String>], evidenzia: Option<usize>) {
    egui::Grid::new(id).num_columns(intestazione.len()).spacing([28.0, 6.0]).show(ui, |ui| {
        for t in intestazione {
            ui.label(piccolo(*t));
        }
        ui.end_row();
        for (i, r) in righe.iter().enumerate() {
            for c in r {
                let t = RichText::new(c).size(14.0);
                ui.label(if evidenzia == Some(i) { t.color(ACCENTO).strong() } else { t });
            }
            ui.end_row();
        }
    });
}

fn righe_gruppi(gruppi: impl Iterator<Item = (&'static str, Gruppo)>) -> Vec<Vec<String>> {
    gruppi
        .map(|(nome, g)| {
            vec![
                nome.to_string(),
                g.colpi.to_string(),
                millisecondi(g.tempo_medio()),
                decimale(g.overshoot_per_bersaglio()),
            ]
        })
        .collect()
}

const INTESTAZIONE_GRUPPI: [&str; 4] = ["", "Colpi", "Tempo medio", "Overshoot per bersaglio"];

fn tabelle_gruppi(ui: &mut Ui, id: &str, g: &Gruppi, dir: Option<Direzione>, fascia: Option<usize>) {
    if g.ha_direzioni() {
        titoletto(ui, "Per direzione");
        let righe = righe_gruppi(Direzione::TUTTE.iter().map(|d| (d.nome(), g.direzione(*d))));
        let evidenzia = dir.and_then(|d| Direzione::TUTTE.iter().position(|x| *x == d));
        tabella(ui, &format!("{id} direzioni"), &INTESTAZIONE_GRUPPI, &righe, evidenzia);
    } else {
        ui.label(piccolo(
            "Per direzione: questi giri sono stati salvati prima che aimpro registrasse la direzione dei bersagli.",
        ));
    }
    titoletto(ui, "Per distanza dal bersaglio precedente");
    let righe = righe_gruppi(FASCE.iter().enumerate().map(|(f, (_, nome))| (*nome, g.distanza(f))));
    tabella(ui, &format!("{id} distanze"), &INTESTAZIONE_GRUPPI, &righe, fascia);
}

fn andamento(ui: &mut Ui, c: &Calcolo, scenario: &str) {
    egui::Grid::new("riassunto").num_columns(2).spacing([40.0, 8.0]).show(ui, |ui| {
        ui.label("Record");
        ui.label(RichText::new(c.record.map_or("-".to_string(), |r| testo_punteggio(scenario, r))).strong());
        ui.end_row();
        ui.label("Da quando hai aperto aimpro");
        let s = &c.sessione;
        let testo = match (s.media, s.migliore) {
            (Some(m), Some(b)) => format!(
                "{} {}, media {m:.1}, migliore {}",
                s.giri,
                if s.giri == 1 { "giro" } else { "giri" },
                testo_punteggio(scenario, b)
            ),
            _ => "nessun giro".to_string(),
        };
        ui.label(RichText::new(testo).strong());
        ui.end_row();
        ui.label("Giri nel periodo");
        ui.label(RichText::new(c.indici.len().to_string()).strong());
        ui.end_row();
    });
    let ridotti = if c.indici.len() > PUNTI_GRAFICO {
        format!(" Con {} giri ogni punto è la media di giri vicini.", c.indici.len())
    } else {
        String::new()
    };
    ui.label(piccolo(format!(
        "Un punto per giro, dal più vecchio a sinistra; la linea chiara è la media degli ultimi {FINESTRA_MEDIA} giri.{ridotti}"
    )));
    titoletto(ui, "Punteggio");
    let record = c.record.map(|r| (r, format!("record {}", testo_punteggio(scenario, r))));
    grafico(ui, &c.punteggi, record, None, |v| testo_punteggio(scenario, v));
    titoletto(ui, "Precisione");
    grafico(ui, &c.precisioni, None, Some(100.0), |v| format!("{v:.0}%"));
    titoletto(ui, "Tempo medio per bersaglio");
    grafico(ui, &c.tempi, None, None, |v| format!("{v:.0} ms"));
}

fn giorni(ui: &mut Ui, c: &Calcolo, scenario: &str) {
    titoletto(ui, "Questa settimana e quella prima");
    match &c.settimane {
        Some(s) => {
            let g = |n: usize| if n == 1 { "giro" } else { "giri" };
            ui.label(format!(
                "Ultimi 7 giorni: media {:.1} in {} {}. I 7 giorni prima: media {:.1} in {} {}.",
                s.media_questa,
                s.giri_questa,
                g(s.giri_questa),
                s.media_prima,
                s.giri_prima,
                g(s.giri_prima),
            ));
            if let Some(m) = s.miglioramento() {
                let (testo, colore) = if m >= 0.0 {
                    (format!("Miglioramento: +{m:.1}%"), ACCENTO)
                } else {
                    (format!("Peggioramento: {m:.1}%"), AVVISO)
                };
                ui.label(RichText::new(testo).size(16.0).strong().color(colore));
            }
        }
        None => {
            ui.label(piccolo("Servono giri sia negli ultimi 7 giorni sia nei 7 giorni prima."));
        }
    }
    titoletto(ui, "Giorno per giorno");
    let righe: Vec<Vec<String>> = c
        .giorni
        .iter()
        .map(|d| {
            vec![
                d.giorno.clone(),
                d.giri.to_string(),
                format!("{:.1}", d.media),
                testo_punteggio(scenario, d.migliore),
            ]
        })
        .collect();
    tabella(ui, "giorni", &["Giorno", "Giri", "Media", "Migliore"], &righe, None);
    if c.giorni_totali > c.giorni.len() {
        ui.label(piccolo(format!("Gli ultimi {} giorni con almeno un giro.", c.giorni.len())));
    }
}

fn righe_medie(a: &Media, b: &Media, scenario: &str) -> [Vec<String>; 4] {
    [
        vec![
            "Punteggio medio".to_string(),
            testo_punteggio(scenario, a.punteggio),
            testo_punteggio(scenario, b.punteggio),
        ],
        vec!["Precisione".to_string(), percentuale(a.precisione), percentuale(b.precisione)],
        vec!["Tempo medio".to_string(), millisecondi(a.tempo_medio_ms), millisecondi(b.tempo_medio_ms)],
        vec![
            "Overshoot per bersaglio".to_string(),
            decimale(a.overshoot_per_bersaglio),
            decimale(b.overshoot_per_bersaglio),
        ],
    ]
}

fn deboli(ui: &mut Ui, p: &PuntiDeboli, scenario: &str) {
    let t = p.gruppi.tutti();
    if t.colpi == 0 {
        ui.label(piccolo("Nel periodo non ci sono bersagli registrati uno per uno."));
    } else {
        titoletto(ui, "In breve");
        if let Some((d, ritardo)) = p.direzione_peggiore {
            ui.label(format!(
                "La direzione più lenta è {}: {ritardo:.0} ms in più della media, a parità di distanza dal bersaglio \
                 precedente.",
                d.nome()
            ));
        } else if p.gruppi.ha_direzioni() {
            ui.label(piccolo(format!(
                "Per confrontare le direzioni servono almeno {COLPI_MINIMI} colpi in due direzioni diverse."
            )));
        }
        if let Some(f) = p.fascia_peggiore {
            ui.label(format!(
                "Rispetto ai tuoi giri migliori perdi di più sui bersagli {}: {:.0} ms contro {:.0} ms (+{:.0}%).",
                SUI_BERSAGLI[f.fascia],
                f.altri_ms,
                f.migliori_ms,
                f.scarto()
            ));
        }
        ui.label(format!(
            "Overshoot: {} per bersaglio; il {} dei bersagli ne ha almeno uno.",
            decimale(t.overshoot_per_bersaglio()),
            percentuale(t.quota_con_overshoot()).replace(".0%", "%")
        ));
        tabelle_gruppi(
            ui,
            "deboli",
            &p.gruppi,
            p.direzione_peggiore.map(|d| d.0),
            p.fascia_peggiore.map(|f| f.fascia),
        );
    }
    titoletto(ui, "I giri migliori contro gli altri");
    match &p.migliori {
        Some((a, b)) => {
            let righe = righe_medie(a, b, scenario);
            let (ta, tb) = (format!("Migliori ({} giri)", a.giri), format!("Altri ({} giri)", b.giri));
            tabella(ui, "migliori", &["", &ta, &tb], &righe, None);
            ui.label(piccolo("I migliori sono il quinto dei giri del periodo con il punteggio più alto."));
        }
        None => {
            ui.label(piccolo("Servono almeno due giri nel periodo."));
        }
    }
}

/// Le fasce di distanza come si dicono in una frase ("sui bersagli ...").
const SUI_BERSAGLI: [&str; 4] = ["vicini, fino a 20°", "a 20-35°", "a 35-50°", "oltre 50°"];

/// Colonne dell'elenco dei giri: titolo e larghezza.
const COLONNE: [(&str, f32); 7] = [
    ("Data", 170.0),
    ("Punteggio", 90.0),
    ("Errori", 70.0),
    ("Precisione", 100.0),
    ("Tempo medio", 110.0),
    ("Overshoot", 90.0),
    ("Sensibilità", 150.0),
];

/// Una riga dell'elenco, che si clicca tutta; vera se cliccata.
fn riga(ui: &mut Ui, celle: [String; 7], intestazione: bool) -> bool {
    let larghezza: f32 = COLONNE.iter().map(|c| c.1).sum();
    let senso = if intestazione { Sense::hover() } else { Sense::click() };
    let (rect, r) = ui.allocate_exact_size(vec2(larghezza, 24.0), senso);
    let p = ui.painter_at(rect);
    if r.hovered() && !intestazione {
        p.rect_filled(rect, CornerRadius::same(4), Color32::from_white_alpha(14));
    }
    let mut x = rect.left();
    for ((_, w), testo) in COLONNE.iter().zip(celle) {
        p.text(
            Pos2::new(x + w / 2.0, rect.center().y),
            Align2::CENTER_CENTER,
            testo,
            FontId::proportional(13.0),
            if intestazione { TENUE } else { CHIARO },
        );
        x += w;
    }
    r.clicked()
}

/// L'elenco dei giri del periodo, il più recente in alto; dice l'indice del giro cliccato.
fn elenco(ui: &mut Ui, s: &Statistiche, indici: &[usize], scenario: &str) -> Option<usize> {
    ui.label(piccolo("Clic su un giro per vederne il dettaglio."));
    riga(ui, COLONNE.map(|(t, _)| t.to_string()), true);
    let mut aperto = None;
    egui::ScrollArea::vertical()
        .id_salt("elenco giri")
        .max_height(ALTEZZA_CONTENUTO - 70.0)
        .auto_shrink([false, true])
        .show_rows(ui, 24.0, indici.len(), |ui, righe| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in righe {
                let indice = indici[indici.len() - 1 - i];
                let g = s.giro(indice);
                let celle = [
                    g.data.clone(),
                    testo_punteggio(scenario, g.punteggio),
                    g.errori.to_string(),
                    percentuale(g.precisione),
                    millisecondi(g.tempo_medio_ms),
                    g.overshoot.to_string(),
                    g.sensibilita.clone(),
                ];
                if riga(ui, celle, false) {
                    aperto = Some(indice);
                }
            }
        });
    aperto
}

/// Le barre dei tempi di ogni bersaglio, in chiaro quelli con almeno un overshoot; col mouse sopra una barra, i suoi
/// numeri.
fn barre(ui: &mut Ui, bersagli: &[Bersaglio]) {
    let massimo = bersagli.iter().map(|b| b.tempo_ms).fold(0.0, f64::max);
    let (risposta, p, dentro, y) = assi(ui, 150.0, cima(massimo), |v| format!("{v:.0} ms"));
    let passo = dentro.width() / bersagli.len().max(1) as f32;
    let spazio = if passo > 4.0 { 1.0 } else { 0.0 };
    let sopra = risposta
        .hover_pos()
        .filter(|q| dentro.x_range().contains(q.x))
        .map(|q| (((q.x - dentro.left()) / passo) as usize).min(bersagli.len().saturating_sub(1)));
    for (i, b) in bersagli.iter().enumerate() {
        let x = dentro.left() + passo * i as f32;
        let r = Rect::from_min_max(
            Pos2::new(x + spazio, y(b.tempo_ms)),
            Pos2::new(x + passo - spazio, dentro.bottom()),
        );
        let colore = if sopra == Some(i) {
            Color32::WHITE
        } else if b.overshoot > 0 {
            CHIARO
        } else {
            ACCENTO
        };
        p.rect_filled(r, CornerRadius::ZERO, colore);
    }
    if let Some(i) = sopra {
        let b = &bersagli[i];
        let direzione = statistiche::direzione(b).map_or(String::new(), |d| format!(", {}", d.nome()));
        risposta.on_hover_text_at_pointer(format!(
            "Bersaglio {}: {:.0} ms, {} overshoot, a {:.0}°{direzione}",
            i + 1,
            b.tempo_ms,
            b.overshoot,
            b.distanza_gradi
        ));
    }
}

fn dettaglio(ui: &mut Ui, d: &Dettaglio, scenario: &str) {
    let s = &d.sintesi;
    ui.label(RichText::new(format!("Giro del {}", s.data)).size(18.0).strong());
    egui::Grid::new("dettaglio").num_columns(4).spacing([40.0, 8.0]).show(ui, |ui| {
        ui.label("Punteggio");
        ui.label(RichText::new(testo_punteggio(scenario, s.punteggio)).strong());
        ui.label("Precisione");
        ui.label(RichText::new(percentuale(s.precisione)).strong());
        ui.end_row();
        ui.label("Colpi ed errori");
        ui.label(RichText::new(format!("{} e {}", s.colpi, s.errori)).strong());
        ui.label("Tempo medio");
        ui.label(RichText::new(millisecondi(s.tempo_medio_ms)).strong());
        ui.end_row();
        ui.label("Overshoot");
        ui.label(RichText::new(s.overshoot.to_string()).strong());
        ui.label("Sensibilità");
        ui.label(RichText::new(&s.sensibilita).strong());
        ui.end_row();
    });
    let Some((g, gruppi)) = &d.giro else {
        ui.add_space(6.0);
        ui.label(piccolo("Il file di questo giro non si legge più: restano i numeri qui sopra."));
        return;
    };
    titoletto(ui, "Tempo di ogni bersaglio");
    if g.bersagli.is_empty() {
        ui.label(piccolo("Nessun bersaglio colpito in questo giro."));
        return;
    }
    ui.label(piccolo(format!(
        "{} bersagli in ordine; in chiaro quelli con almeno un overshoot (il mirino è entrato nella sfera ed è uscito \
         senza colpirla). Col mouse sopra una barra, i suoi numeri.",
        g.bersagli.len()
    )));
    barre(ui, &g.bersagli);
    tabelle_gruppi(ui, "dettaglio", gruppi, None, None);
}

fn due_pulsanti(ui: &mut Ui, a: &str, b: &str) -> (bool, bool) {
    ui.horizontal(|ui| {
        let w = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
        let mut premuto = |t: &str| {
            ui.add_sized([w, 38.0], egui::Button::new(RichText::new(t).size(16.0).strong())).clicked()
        };
        (premuto(a), premuto(b))
    })
    .inner
}

/// La schermata Statistiche. `saltati` sono i file dello storico che non si leggono.
pub fn mostra(
    ui: &mut Ui,
    v: &mut Vista,
    s: &Statistiche,
    saltati: u32,
    adesso_ms: i64,
    avvio_ms: i64,
) -> Azione {
    v.aggiorna(s, adesso_ms, avvio_ms);
    scheda_larga(ui, "statistiche", LARGHEZZA, |ui| {
        let mut azione = Azione::Nessuna;
        let Some(c) = v.calcolo.as_ref() else { return azione };
        let scenario = v.scenario.clone();
        let mut aperto = None;
        ui.label(RichText::new("Statistiche").size(26.0).strong());
        if !c.ha_giri {
            ui.label(piccolo(format!("{}, 30 secondi", nome_scenario(&scenario))));
            ui.add_space(6.0);
            ui.label("Nessun giro salvato. Gioca un giro: ogni giro finito compare qui, con il record e l'andamento.");
        } else if let Some(d) = &v.dettaglio {
            ui.label(piccolo(format!("{}, 30 secondi", nome_scenario(&scenario))));
            egui::ScrollArea::vertical()
                .id_salt("dettaglio giro")
                .max_height(ALTEZZA_CONTENUTO + 40.0)
                .auto_shrink([false, false])
                .show(ui, |ui| dettaglio(ui, d, &scenario));
        } else {
            let (mut scenario_scelto, mut periodo, mut scheda) = (scenario.clone(), v.periodo, v.scheda);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label("Scenario");
                egui::ComboBox::from_id_salt("scenario statistiche")
                    .selected_text(format!("{}, 30 secondi", nome_scenario(&scenario)))
                    .show_ui(ui, |ui| {
                        for sc in &c.scenari {
                            ui.selectable_value(&mut scenario_scelto, sc.clone(), nome_scenario(sc));
                        }
                    });
                ui.add_space(24.0);
                ui.label("Periodo");
                for p in Periodo::TUTTI {
                    ui.selectable_value(&mut periodo, p, p.nome());
                }
            });
            ui.horizontal(|ui| {
                for sc in Scheda::TUTTE {
                    ui.selectable_value(&mut scheda, sc, RichText::new(sc.nome()).size(16.0));
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt(("contenuto statistiche", v.scheda as u8))
                .max_height(ALTEZZA_CONTENUTO)
                // Altezza fissa: il riquadro non cambia misura (e le schede non si spostano) da una scheda all'altra.
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if c.indici.is_empty() {
                        ui.label(format!("Nessun giro negli ultimi {}.", v.periodo.nome()));
                        return;
                    }
                    match v.scheda {
                        Scheda::Andamento => andamento(ui, c, &scenario),
                        Scheda::Giorni => giorni(ui, c, &scenario),
                        Scheda::PuntiDeboli => deboli(ui, &c.deboli, &scenario),
                        Scheda::Giri => aperto = elenco(ui, s, &c.indici, &scenario),
                    }
                });
            if (scenario_scelto.as_str(), periodo, scheda) != (v.scenario.as_str(), v.periodo, v.scheda) {
                // I conti nuovi al prossimo frame, che egui chiede da sé dopo un clic.
                (v.scenario, v.periodo, v.scheda) = (scenario_scelto, periodo, scheda);
            }
        }
        if let Some(i) = aperto {
            let sintesi = s.giro(i).clone();
            let giro = sintesi.percorso.as_deref().and_then(statistiche::leggi_giro).map(|g| {
                let gruppi = Gruppi::da_bersagli(&g.bersagli);
                (g, gruppi)
            });
            v.dettaglio = Some(Dettaglio { sintesi, giro });
        }
        if saltati > 0 {
            ui.label(piccolo(format!("{saltati} file dello storico non si leggono e sono stati saltati.")));
        }
        ui.add_space(6.0);
        if v.dettaglio.is_some() {
            let (elenco, menu) = due_pulsanti(ui, "Torna all'elenco", "Torna al menu");
            if elenco {
                v.dettaglio = None;
            }
            if menu {
                azione = Azione::TornaAlMenu;
            }
        } else if pulsante(ui, "Torna al menu", false) {
            azione = Azione::TornaAlMenu;
        }
        azione
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn la_cima_del_grafico_e_tonda_e_sopra_il_massimo() {
        for (m, attesa) in [(44.0, 50.0), (193.0, 250.0), (100.0, 150.0), (0.9, 1.0), (812.0, 900.0)] {
            let c = cima(m);
            assert!(c > m, "{m} -> {c}");
            assert!((c - attesa).abs() < 1e-9, "{m} -> {c} invece di {attesa}");
        }
        assert!(cima(0.0) > 0.0);
    }

    #[test]
    fn una_serie_lunga_si_riduce_con_la_media_di_ogni_tratto() {
        let v: Vec<Option<f64>> = (0..10).map(|i| Some(f64::from(i))).collect();
        assert_eq!(riduci(&v, 20), v, "corta: resta com'è");
        assert_eq!(riduci(&v, 5), [Some(0.5), Some(2.5), Some(4.5), Some(6.5), Some(8.5)]);
        let con_buchi = [None, None, Some(4.0), None];
        assert_eq!(riduci(&con_buchi, 2), [None, Some(4.0)]);
        assert_eq!(riduci(&vec![Some(1.0); 1000], PUNTI_GRAFICO).len(), PUNTI_GRAFICO);
    }
}
