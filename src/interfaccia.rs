//! I pannelli con egui: menu con le impostazioni, pausa, riepilogo, statistiche, pannello della latenza.
//! egui gira solo fuori dal giro (e, nel giro, solo per il pannello della latenza quando è aperto, 4 volte al
//! secondo): nel giro il frame non passa da egui.

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, Pos2, RichText, Sense, Shape, Stroke, Ui};

use crate::aggiornamento::Stato as StatoAggiornamento;
use crate::config::{Config, LIMITI_DPI, LIMITI_FOV, LIMITI_FPS, LIMITI_SENSIBILITA};
use crate::giro::Riepilogo;
use crate::sensibilita::{Gioco, cm_per_360, converti, gradi_per_count};
use crate::storico::{Confronto, GiroSalvato, SCATTO, Storico};

pub const ACCENTO: Color32 = Color32::from_rgb(255, 176, 33);
const TENUE: Color32 = Color32::from_rgb(161, 163, 169);
const AVVISO: Color32 = Color32::from_rgb(255, 210, 122);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Azione {
    Nessuna,
    Inizia,
    Riprendi,
    EsciDalGiro,
    TornaAlMenu,
    Statistiche,
    EsportaCsv,
    /// "Aggiorna e riavvia" (o "Riprova" dopo un errore).
    Aggiorna,
    /// Le impostazioni sono cambiate: si salvano e si applicano.
    Impostazioni,
    Chiudi,
}

/// Informazioni da mostrare nel menu.
pub struct InfoMenu<'a> {
    pub scheda: &'a str,
    pub presentazione: &'a str,
    pub avviso: Option<&'a str>,
    /// La build di questo .exe (`None` per una build locale) e lo stato dell'aggiornamento.
    pub build: Option<u32>,
    pub aggiornamento: &'a StatoAggiornamento,
    /// Gli Hz dello schermo, arrotondati (0 se Windows non li dice): la voce del limite pari a questi dice "(schermo)".
    pub hz_schermo: u32,
}

/// Le voci fisse della tendina del limite di fps, fra "Senza limite" e "Personalizzato…".
pub const LIMITI_ELENCO: [u32; 6] = [60, 144, 165, 240, 360, 480];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum VoceLimite {
    Senza,
    Elenco(u32),
    Personalizzato,
}

/// La voce della tendina per un limite (0 = nessun limite). `personalizzato` è la scelta fatta in questa sessione:
/// chi ha scelto "Personalizzato…" continua a vedere il campo anche se scrive un numero dell'elenco.
fn voce_limite(limite: u32, personalizzato: bool) -> VoceLimite {
    if limite == 0 {
        VoceLimite::Senza
    } else if !personalizzato && LIMITI_ELENCO.contains(&limite) {
        VoceLimite::Elenco(limite)
    } else {
        VoceLimite::Personalizzato
    }
}

/// Il limite che dà una voce; `attuale` è quello di prima, che "Personalizzato…" tiene (240 se non c'era).
fn valore_voce(voce: VoceLimite, attuale: u32) -> u32 {
    match voce {
        VoceLimite::Senza => 0,
        VoceLimite::Elenco(v) => v,
        VoceLimite::Personalizzato if attuale == 0 => 240,
        VoceLimite::Personalizzato => attuale,
    }
}

fn nome_voce(voce: VoceLimite, hz_schermo: u32) -> String {
    match voce {
        VoceLimite::Senza => "Senza limite".to_string(),
        VoceLimite::Elenco(v) if v == hz_schermo => format!("{v} fps (schermo)"),
        VoceLimite::Elenco(v) => format!("{v} fps"),
        VoceLimite::Personalizzato => "Personalizzato…".to_string(),
    }
}

/// Gli Hz di winit (239,96, 59,94) arrotondati, per riconoscere la voce pari allo schermo.
pub fn hz_intero(hz: f64) -> u32 {
    if hz > 0.0 { hz.round() as u32 } else { 0 }
}

/// La tendina del limite di fps e, solo con "Personalizzato…", il campo del numero. Vero se il limite è cambiato.
fn riga_limite(ui: &mut Ui, c: &mut Config, hz_schermo: u32) -> bool {
    let id = egui::Id::unique("limite personalizzato");
    let mut personalizzato = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let prima = c.limite_fps;
    let voce = voce_limite(c.limite_fps, personalizzato);
    // Un valore fuori elenco (dal config.toml) resta "Personalizzato…" anche se col campo si arriva a uno dell'elenco:
    // il campo non deve sparire mentre lo si trascina.
    personalizzato |= voce == VoceLimite::Personalizzato;
    let mut scelta = voce;
    let tendina = |ui: &mut Ui, scelta: &mut VoceLimite| {
        egui::ComboBox::from_id_salt("limite fps")
            .selected_text(nome_voce(voce, hz_schermo))
            .width(170.0)
            .height(400.0)
            .show_ui(ui, |ui| {
                let voci = std::iter::once(VoceLimite::Senza)
                    .chain(LIMITI_ELENCO.map(VoceLimite::Elenco))
                    .chain(std::iter::once(VoceLimite::Personalizzato));
                for v in voci {
                    ui.selectable_value(scelta, v, nome_voce(v, hz_schermo));
                }
            });
    };
    if voce == VoceLimite::Personalizzato {
        ui.horizontal_top(|ui| {
            tendina(ui, &mut scelta);
            if scelta == voce {
                // Margini interni più stretti: con l'etichetta del FOV di Valorant la colonna parte più a destra e
                // "2000 fps" con i margini dei pulsanti allargava il riquadro del menu.
                ui.spacing_mut().button_padding.x = 8.0;
                ui.add(
                    egui::DragValue::new(&mut c.limite_fps)
                        .range(LIMITI_FPS.0..=LIMITI_FPS.1)
                        .speed(1.0)
                        .suffix(" fps"),
                );
            }
        });
    } else {
        // Senza il campo la tendina va da sola nella cella, allineata come quella della sensibilità.
        tendina(ui, &mut scelta);
    }
    if scelta != voce {
        // Il campo compare (o sparisce) dal frame dopo: la scelta chiede comunque un ridisegno.
        personalizzato = scelta == VoceLimite::Personalizzato;
        c.limite_fps = valore_voce(scelta, c.limite_fps);
    }
    ui.data_mut(|d| d.insert_temp(id, personalizzato));
    c.limite_fps != prima
}

fn maiuscola(t: &str) -> String {
    let mut c = t.chars();
    c.next().map_or(String::new(), |p| p.to_uppercase().chain(c).collect())
}

/// Il riquadro dell'aggiornamento in cima al menu; niente se non c'è niente da dire.
fn aggiornamento(ui: &mut Ui, build: Option<u32>, stato: &StatoAggiornamento) -> Azione {
    let tua = build.map_or("una build locale".to_string(), |b| format!("la build {b}"));
    let (titolo, riga, colore, pulsante_attivo, testo_pulsante) = match stato {
        StatoAggiornamento::Nessuno => return Azione::Nessuna,
        StatoAggiornamento::Disponibile(r) => (
            "Aggiornamento disponibile".to_string(),
            format!("È uscita la build {}: hai {tua}.", r.build),
            TENUE,
            true,
            "Aggiorna e riavvia",
        ),
        StatoAggiornamento::Scarico { release, scaricati, totali } => (
            format!("Scarico la build {}", release.build),
            match totali {
                Some(t) if *t > 0 => format!(
                    "{:.1} MB su {:.1} ({}%). aimpro si riavvia da solo alla fine.",
                    *scaricati as f64 / 1e6,
                    *t as f64 / 1e6,
                    scaricati * 100 / t
                ),
                _ => format!("{:.1} MB. aimpro si riavvia da solo alla fine.", *scaricati as f64 / 1e6),
            },
            TENUE,
            false,
            "Aggiorna e riavvia",
        ),
        StatoAggiornamento::Pronto(r) => (
            format!("Build {} pronta", r.build),
            "Riavvio aimpro.".to_string(),
            TENUE,
            false,
            "Aggiorna e riavvia",
        ),
        StatoAggiornamento::NonRiparte { release, motivo } => (
            format!("Build {} installata", release.build),
            format!(
                "Non è ripartita da sola ({}): chiudi aimpro e riaprilo.",
                motivo.trim_end().trim_end_matches('.')
            ),
            AVVISO,
            false,
            "Aggiorna e riavvia",
        ),
        StatoAggiornamento::Errore { release, motivo } => (
            "Aggiornamento non riuscito".to_string(),
            format!(
                "{}. Resta {tua}: puoi riprovare, o scaricare la build {} dalla pagina delle release.",
                maiuscola(motivo.trim_end().trim_end_matches('.')),
                release.build
            ),
            AVVISO,
            true,
            "Riprova",
        ),
    };
    let mut azione = Azione::Nessuna;
    ui.label(RichText::new(titolo).size(15.0).strong());
    ui.label(RichText::new(riga).color(colore).size(13.0));
    if ui.add_enabled_ui(pulsante_attivo, |ui| pulsante(ui, testo_pulsante, false)).inner {
        azione = Azione::Aggiorna;
    }
    ui.separator();
    azione
}

pub fn stile(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    let mut s = (*ctx.global_style()).clone();
    s.spacing.item_spacing = egui::vec2(10.0, 10.0);
    s.spacing.button_padding = egui::vec2(18.0, 8.0);
    s.visuals.selection.bg_fill = ACCENTO.gamma_multiply(0.6);
    // Niente animazioni: ogni dissolvenza sono decine di frame di fila nel menu, cioè consumo senza motivo.
    s.animation_time = 0.0;
    ctx.set_global_style(s);
}

fn scheda(ui: &mut Ui, nome: &str, contenuto: impl FnOnce(&mut Ui) -> Azione) -> Azione {
    scheda_larga(ui, nome, 440.0, contenuto)
}

/// Un riquadro centrato. `nome` gli dà un id suo: egui centra l'area con la misura che aveva nel frame prima, e con
/// un id in comune il primo frame di un riquadro prenderebbe la misura di un altro (la pausa usciva più in alto,
/// con l'altezza del riepilogo).
fn scheda_larga(
    ui: &mut Ui,
    nome: &str,
    larghezza: f32,
    contenuto: impl FnOnce(&mut Ui) -> Azione,
) -> Azione {
    let mut azione = Azione::Nessuna;
    let id = egui::Id::unique(("scheda", nome));
    let risposta = egui::Area::new(id).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(22, 23, 26, 240))
            .stroke(Stroke::new(1.0, Color32::from_white_alpha(26)))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(28))
            .show(ui, |ui| {
                ui.set_width(larghezza);
                azione = contenuto(ui);
            });
    });
    // Lo stesso riquadro può cambiare altezza (il riepilogo con o senza "Nuovo record", il menu con un avviso):
    // centrato con la misura vecchia uscirebbe spostato fino al prossimo evento. Se la misura è cambiata, egui rifà
    // subito il passaggio con quella nuova; non con un clic in mano, che il secondo passaggio non vedrebbe.
    let misura = risposta.response.rect.size();
    let chiave = id.with("misura");
    let prima: Option<egui::Vec2> = ui.ctx().data(|d| d.get_temp(chiave));
    ui.ctx().data_mut(|d| d.insert_temp(chiave, misura));
    if prima.is_some_and(|p| (p - misura).length() > 0.5) && azione == Azione::Nessuna {
        ui.ctx().request_discard("riquadro con un'altra misura");
        // Già al secondo passaggio egui non ne fa un terzo: allora un frame in più, che parte dalla misura nuova.
        if !ui.ctx().will_discard() {
            ui.ctx().request_repaint();
        }
    }
    azione
}

fn pulsante(ui: &mut Ui, testo: &str, principale: bool) -> bool {
    let t = RichText::new(testo).size(16.0).strong();
    let b = if principale {
        egui::Button::new(t.color(Color32::from_rgb(23, 19, 10))).fill(ACCENTO)
    } else {
        egui::Button::new(t)
    };
    ui.add_sized([ui.available_width(), 38.0], b).clicked()
}

pub fn menu(ui: &mut Ui, c: &mut Config, info: &InfoMenu) -> Azione {
    scheda(ui, "menu", |ui| {
        ui.label(RichText::new("aimpro").size(28.0).strong());
        ui.label(RichText::new("Scatto: 30 secondi, una sfera alla volta").color(TENUE));
        ui.add_space(4.0);
        ui.label("Colpisci la sfera: appena la prendi ne compare un'altra in un altro punto. Ogni clic a vuoto è un errore.");
        ui.separator();
        let mut azione = aggiornamento(ui, info.build, info.aggiornamento);
        // Mentre si scarica o si riavvia non si gioca: la rete non lavora durante il giro.
        let si_gioca =
            !matches!(info.aggiornamento, StatoAggiornamento::Scarico { .. } | StatoAggiornamento::Pronto(_));

        egui::Grid::new("impostazioni").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label("Sensibilità di");
            let prima = c.gioco;
            egui::ComboBox::from_id_salt("gioco").selected_text(c.gioco.nome()).show_ui(ui, |ui| {
                for g in Gioco::TUTTI {
                    ui.selectable_value(&mut c.gioco, g, g.nome());
                }
            });
            if c.gioco != prima {
                // Stessi centimetri per giro nel gioco nuovo; FOV al valore di base del gioco.
                // Dentro i limiti: un valore estremo convertito si ferma al limite invece di tornare al predefinito.
                c.sensibilita = ((converti(prima, c.sensibilita, c.gioco) * 1_000_000.0).round()
                    / 1_000_000.0)
                    .clamp(LIMITI_SENSIBILITA.0, LIMITI_SENSIBILITA.1);
                c.fov = c.gioco.fov_predefinito();
                azione = Azione::Impostazioni;
            }
            ui.end_row();

            ui.label("Sensibilità");
            if ui
                .add(
                    egui::DragValue::new(&mut c.sensibilita)
                        .range(LIMITI_SENSIBILITA.0..=LIMITI_SENSIBILITA.1)
                        .speed(0.005)
                        .max_decimals(4),
                )
                .changed()
            {
                azione = Azione::Impostazioni;
            }
            ui.end_row();

            ui.label(if c.gioco == Gioco::Cs2 {
                "FOV (su 4:3, 90 in CS2)"
            } else {
                "FOV (su 16:9, 103 in Valorant)"
            });
            if ui
                .add(
                    egui::DragValue::new(&mut c.fov)
                        .range(LIMITI_FOV.0..=LIMITI_FOV.1)
                        .speed(0.1)
                        .max_decimals(2),
                )
                .changed()
            {
                azione = Azione::Impostazioni;
            }
            ui.end_row();

            ui.label("DPI del mouse");
            if ui
                .add(egui::DragValue::new(&mut c.dpi).range(LIMITI_DPI.0..=LIMITI_DPI.1).speed(10.0))
                .changed()
            {
                azione = Azione::Impostazioni;
            }
            ui.end_row();

            ui.label("Vsync");
            if ui.checkbox(&mut c.vsync, "acceso (più latenza, niente tearing)").changed() {
                azione = Azione::Impostazioni;
            }
            ui.end_row();

            if c.vsync {
                // Col vsync il limite non vale: la riga resta, grigia, e dice perché.
                // Disegnate come disattivate, cioè più scure delle altre righe.
                ui.add_enabled(false, egui::Label::new("Limite fps"));
                // Alta quanto la tendina: accendendo il vsync il riquadro non si sposta.
                let alta =
                    ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
                ui.allocate_ui_with_layout(
                    egui::vec2(0.0, alta),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| ui.add_enabled(false, egui::Label::new("col vsync decide lo schermo")),
                );
            } else {
                ui.label("Limite fps");
                if riga_limite(ui, c, info.hz_schermo) {
                    azione = Azione::Impostazioni;
                }
            }
            ui.end_row();

            ui.label("Schermo intero");
            if ui.checkbox(&mut c.schermo_intero_esclusivo, "esclusivo (vale dal prossimo avvio)").changed() {
                azione = Azione::Impostazioni;
            }
            ui.end_row();

            ui.label("Aggiornamenti");
            if ui.checkbox(&mut c.controlla_aggiornamenti, "controlla all'avvio").changed() {
                azione = Azione::Impostazioni;
            }
            ui.end_row();
        });

        ui.add_space(2.0);
        ui.label(
            RichText::new(format!(
                "Ogni count del mouse gira la visuale di {:.4} gradi: a {:.0} DPI un giro completo è {:.1} cm.",
                gradi_per_count(c.gioco, c.sensibilita),
                c.dpi,
                cm_per_360(c.gioco, c.sensibilita, c.dpi)
            ))
            .color(TENUE)
            .size(13.0),
        );
        if let Some(a) = info.avviso {
            ui.label(RichText::new(a).color(AVVISO).size(13.0));
        }
        ui.add_space(6.0);
        if ui.add_enabled_ui(si_gioca, |ui| pulsante(ui, "Inizia", true)).inner {
            azione = Azione::Inizia;
        }
        if pulsante(ui, "Statistiche", false) {
            azione = Azione::Statistiche;
        }
        if pulsante(ui, "Esci da aimpro", false) {
            azione = Azione::Chiudi;
        }
        // Due righe: in una sola, nel riquadro da 440 px, il nome della scheda finiva spezzato a metà.
        ui.label(
            RichText::new("Esc mette in pausa, F3 apre il pannello della latenza.").color(TENUE).size(12.0),
        );
        ui.label(
            RichText::new(format!(
                "{} · {} · {}",
                info.build.map_or("build locale".to_string(), |b| format!("build {b}")),
                info.scheda,
                match info.presentazione {
                    "Immediate" | "Mailbox" => "vsync spento",
                    "Fifo" => "vsync acceso",
                    altro => altro,
                }
            ))
            .color(TENUE)
            .size(12.0),
        );
        azione
    })
}

pub fn pausa(ui: &mut Ui, secondi: u32) -> Azione {
    scheda(ui, "pausa", |ui| {
        let mut azione = Azione::Nessuna;
        ui.label(RichText::new("In pausa").size(22.0).strong());
        ui.label(RichText::new(format!("Restano {secondi} secondi. Il tempo è fermo.")).color(TENUE));
        ui.add_space(6.0);
        if pulsante(ui, "Riprendi", true) {
            azione = Azione::Riprendi;
        }
        if pulsante(ui, "Esci dal giro", false) {
            azione = Azione::EsciDalGiro;
        }
        azione
    })
}

/// Il confronto col record come si legge nel riepilogo: il record di prima, la differenza, e se è un record nuovo.
pub fn testo_confronto(c: &Confronto) -> (String, String, bool) {
    let record = match c.record_prima {
        Some(r) => format!("{r} colpi"),
        None => "nessuno prima di questo giro".to_string(),
    };
    let differenza = match c.differenza() {
        Some(0) => "pari al record".to_string(),
        Some(d) if d > 0 => format!("+{d} colpi"),
        Some(d) => format!("{d} colpi"),
        None => "primo giro".to_string(),
    };
    (record, differenza, c.nuovo_record())
}

pub fn riepilogo(ui: &mut Ui, r: &Riepilogo, confronto: Option<&Confronto>, avviso: Option<&str>) -> Azione {
    scheda(ui, "riepilogo", |ui| {
        let mut azione = Azione::Nessuna;
        ui.label(RichText::new("Fine del giro").size(22.0).strong());
        if let Some(c) = confronto.filter(|c| c.nuovo_record()) {
            ui.label(
                RichText::new(format!("Nuovo record: {} colpi", c.colpi)).size(17.0).strong().color(ACCENTO),
            );
        }
        ui.add_space(4.0);
        let precisione = r.precisione.map_or("nessun clic".to_string(), |p| format!("{:.1}%", p * 100.0));
        let tempo = r.tempo_medio_ms.map_or("nessun colpo".to_string(), |t| format!("{t:.0} ms"));
        egui::Grid::new("riepilogo").num_columns(2).spacing([40.0, 10.0]).show(ui, |ui| {
            for (nome, valore) in [
                ("Colpi", r.colpi.to_string()),
                ("Errori", r.errori.to_string()),
                ("Precisione", precisione),
                ("Tempo medio per bersaglio", tempo),
            ] {
                ui.label(nome);
                ui.label(RichText::new(valore).strong().size(16.0));
                ui.end_row();
            }
            if let Some(c) = confronto {
                let (record, differenza, _) = testo_confronto(c);
                ui.label("Record di scatto");
                ui.label(RichText::new(record).strong().size(16.0));
                ui.end_row();
                ui.label("Rispetto al record");
                ui.label(RichText::new(differenza).strong().size(16.0));
                ui.end_row();
            }
        });
        if let Some(a) = avviso {
            ui.label(RichText::new(a).color(AVVISO).size(13.0));
        }
        ui.add_space(8.0);
        if pulsante(ui, "Rigioca", true) {
            azione = Azione::Inizia;
        }
        if pulsante(ui, "Statistiche", false) {
            azione = Azione::Statistiche;
        }
        if pulsante(ui, "Impostazioni", false) {
            azione = Azione::TornaAlMenu;
        }
        azione
    })
}

/// Giri mostrati nel grafico.
pub const GIRI_NEL_GRAFICO: usize = 30;

/// Il valore in cima al grafico: un po' sopra il più alto fra giri e record, arrotondato a 5.
pub fn cima_grafico(colpi: &[u32], record: u32) -> u32 {
    let massimo = colpi.iter().copied().max().unwrap_or(0).max(record);
    (massimo + massimo / 10 + 1).div_ceil(5).max(1) * 5
}

fn grafico(ui: &mut Ui, giri: &[&GiroSalvato], record: u32) {
    let colpi: Vec<u32> = giri.iter().map(|g| g.colpi).collect();
    let cima = cima_grafico(&colpi, record) as f32;
    let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 170.0), Sense::hover());
    let p = ui.painter_at(area);
    p.rect_filled(area, CornerRadius::same(8), Color32::from_white_alpha(6));
    // Spazio a sinistra per i valori dell'asse.
    let dentro =
        egui::Rect::from_min_max(area.min + egui::vec2(36.0, 14.0), area.max - egui::vec2(14.0, 14.0));
    let y = |v: f32| dentro.bottom() - v / cima * dentro.height();
    let font = FontId::proportional(12.0);
    for v in [0.0, cima / 2.0, cima] {
        p.line_segment(
            [Pos2::new(dentro.left(), y(v)), Pos2::new(dentro.right(), y(v))],
            Stroke::new(1.0, Color32::from_white_alpha(14)),
        );
        p.text(
            Pos2::new(dentro.left() - 8.0, y(v)),
            Align2::RIGHT_CENTER,
            format!("{v:.0}"),
            font.clone(),
            TENUE,
        );
    }
    let chiaro = Color32::from_rgb(216, 217, 220);
    let yr = y(record as f32);
    p.extend(Shape::dashed_line(
        &[Pos2::new(dentro.left(), yr), Pos2::new(dentro.right(), yr)],
        Stroke::new(1.5, chiaro),
        6.0,
        4.0,
    ));
    p.text(
        Pos2::new(dentro.right(), yr - 4.0),
        Align2::RIGHT_BOTTOM,
        format!("record {record}"),
        font,
        chiaro,
    );
    // Con pochi giri i punti si allargano su tutto il grafico, il più recente sempre a destra.
    let passo = dentro.width() / colpi.len().saturating_sub(1).max(1) as f32;
    let ultimo = colpi.len().saturating_sub(1);
    let punti: Vec<Pos2> = colpi
        .iter()
        .enumerate()
        .map(|(i, &c)| Pos2::new(dentro.right() - passo * (ultimo - i) as f32, y(c as f32)))
        .collect();
    if punti.len() > 1 {
        p.add(Shape::line(punti.clone(), Stroke::new(2.0, ACCENTO)));
    }
    for q in punti {
        p.circle_filled(q, 3.5, ACCENTO);
    }
}

fn percentuale(p: Option<f64>) -> String {
    p.map_or("-".to_string(), |p| format!("{:.1}%", p * 100.0))
}

fn millisecondi(t: Option<f64>) -> String {
    t.map_or("-".to_string(), |t| format!("{t:.0} ms"))
}

/// Colonne dell'elenco dei giri: titolo e larghezza.
const COLONNE: [(&str, f32); 7] = [
    ("Data", 150.0),
    ("Colpi", 60.0),
    ("Errori", 60.0),
    ("Precisione", 90.0),
    ("Tempo medio", 100.0),
    ("Overshoot", 85.0),
    ("Sensibilità", 130.0),
];

fn riga(ui: &mut Ui, celle: [String; 7], intestazione: bool) {
    ui.horizontal(|ui| {
        for ((_, w), testo) in COLONNE.iter().zip(celle) {
            let t = RichText::new(testo).size(13.0);
            let t = if intestazione { t.color(TENUE) } else { t };
            ui.add_sized([*w, 18.0], egui::Label::new(t).truncate());
        }
    });
}

/// La schermata Statistiche dello scenario scatto: grafico degli ultimi giri con il record, la sessione e l'elenco.
pub fn statistiche(ui: &mut Ui, s: &Storico, esportato: Option<&str>) -> Azione {
    scheda_larga(ui, "statistiche", 760.0, |ui| {
        let mut azione = Azione::Nessuna;
        ui.label(RichText::new("Statistiche").size(26.0).strong());
        ui.label(RichText::new("Scatto, 30 secondi").color(TENUE));
        let giri: Vec<&GiroSalvato> = s.giri_di(SCATTO).collect();
        if giri.is_empty() {
            ui.add_space(6.0);
            ui.label("Nessun giro salvato. Gioca uno scatto: ogni giro finito compare qui, con il record e l'andamento.");
        } else {
            let record = s.record(SCATTO).unwrap_or(0);
            let sessione = s.sessione(SCATTO);
            ui.add_space(4.0);
            let inizio = giri.len().saturating_sub(GIRI_NEL_GRAFICO);
            let n = giri.len() - inizio;
            ui.label(
                RichText::new(if n == 1 {
                    "Colpi dell'ultimo giro, con la linea del record".to_string()
                } else {
                    format!("Colpi degli ultimi {n} giri, con la linea del record")
                })
                .size(13.0)
                .color(TENUE),
            );
            grafico(ui, &giri[inizio..], record);
            ui.add_space(2.0);
            egui::Grid::new("sessione").num_columns(2).spacing([40.0, 8.0]).show(ui, |ui| {
                ui.label("Record");
                ui.label(RichText::new(format!("{record} colpi")).strong());
                ui.end_row();
                ui.label("Da quando hai aperto aimpro");
                let testo = match (sessione.media_colpi, sessione.migliore) {
                    (Some(m), Some(b)) => format!(
                        "{} {}, media {m:.1} colpi, migliore {b}",
                        sessione.giri,
                        if sessione.giri == 1 { "giro" } else { "giri" }
                    ),
                    _ => "nessun giro".to_string(),
                };
                ui.label(RichText::new(testo).strong());
                ui.end_row();
                ui.label("Giri salvati");
                ui.label(RichText::new(giri.len().to_string()).strong());
                ui.end_row();
            });
            ui.separator();
            riga(ui, COLONNE.map(|(t, _)| t.to_string()), true);
            egui::ScrollArea::vertical().max_height(200.0).auto_shrink([false, true]).show_rows(
                ui,
                18.0,
                giri.len(),
                |ui, righe| {
                    for i in righe {
                        // Il più recente in alto.
                        let g = giri[giri.len() - 1 - i];
                        riga(
                            ui,
                            [
                                g.data.clone(),
                                g.colpi.to_string(),
                                g.errori.to_string(),
                                percentuale(g.precisione),
                                millisecondi(g.tempo_medio_ms),
                                g.overshoot.to_string(),
                                format!("{} {}", g.impostazioni.gioco.nome(), g.impostazioni.sensibilita),
                            ],
                            false,
                        );
                    }
                },
            );
        }
        if s.saltati > 0 {
            ui.label(
                RichText::new(format!(
                    "{} file dello storico non si leggono e sono stati saltati.",
                    s.saltati
                ))
                .color(TENUE)
                .size(12.0),
            );
        }
        if let Some(e) = esportato {
            ui.label(RichText::new(e).size(13.0));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            let esporta = egui::Button::new(RichText::new("Esporta CSV").size(16.0).strong());
            if ui.add_enabled_ui(!giri.is_empty(), |ui| ui.add_sized([w, 38.0], esporta)).inner.clicked() {
                azione = Azione::EsportaCsv;
            }
            let torna = egui::Button::new(RichText::new("Torna al menu").size(16.0).strong());
            if ui.add_sized([w, 38.0], torna).clicked() {
                azione = Azione::TornaAlMenu;
            }
        });
        azione
    })
}

pub fn pannello_latenza(ui: &mut Ui, testo: &str) {
    egui::Area::new(egui::Id::unique("latenza")).anchor(Align2::RIGHT_TOP, [-12.0, 12.0]).show(
        ui.ctx(),
        |ui| {
            Frame::new()
                .fill(Color32::from_rgba_unmultiplied(14, 15, 17, 225))
                .stroke(Stroke::new(1.0, Color32::from_white_alpha(26)))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    // Una riga per misura: niente a capo, il pannello si allarga quanto serve.
                    ui.add(
                        egui::Label::new(
                            RichText::new(testo)
                                .monospace()
                                .size(12.0)
                                .color(Color32::from_rgb(216, 217, 220)),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        },
    );
}

#[cfg(test)]
mod test {
    use super::*;

    fn c(colpi: u32, record_prima: Option<u32>) -> Confronto {
        Confronto { colpi, record_prima }
    }

    #[test]
    fn il_confronto_dice_record_differenza_e_record_nuovo() {
        let s = |a: &str, b: &str, n| (a.to_string(), b.to_string(), n);
        assert_eq!(testo_confronto(&c(34, Some(30))), s("30 colpi", "+4 colpi", true));
        assert_eq!(testo_confronto(&c(25, Some(30))), s("30 colpi", "-5 colpi", false));
        assert_eq!(testo_confronto(&c(30, Some(30))), s("30 colpi", "pari al record", false));
        assert_eq!(testo_confronto(&c(12, None)), s("nessuno prima di questo giro", "primo giro", true));
    }

    #[test]
    fn il_motivo_dell_errore_comincia_con_la_maiuscola() {
        assert_eq!(maiuscola("lo SHA-256 non torna"), "Lo SHA-256 non torna");
        assert_eq!(maiuscola(""), "");
    }

    #[test]
    fn il_limite_di_fps_ha_la_sua_voce_nella_tendina() {
        assert_eq!(voce_limite(0, false), VoceLimite::Senza);
        assert_eq!(voce_limite(0, true), VoceLimite::Senza);
        for v in LIMITI_ELENCO {
            assert_eq!(voce_limite(v, false), VoceLimite::Elenco(v));
            // Scelto "Personalizzato…" resta personalizzato anche se il numero scritto è in elenco.
            assert_eq!(voce_limite(v, true), VoceLimite::Personalizzato);
        }
        // Un valore fuori dall'elenco (anche da un config.toml scritto a mano) è personalizzato.
        assert_eq!(voce_limite(200, false), VoceLimite::Personalizzato);
        assert_eq!(voce_limite(30, false), VoceLimite::Personalizzato);
        assert_eq!(voce_limite(2000, false), VoceLimite::Personalizzato);
    }

    #[test]
    fn ogni_voce_della_tendina_da_il_suo_valore() {
        assert_eq!(valore_voce(VoceLimite::Senza, 240), 0);
        for v in LIMITI_ELENCO {
            assert_eq!(valore_voce(VoceLimite::Elenco(v), 0), v);
            // Andata e ritorno: la voce scelta è quella che si rivede.
            assert_eq!(voce_limite(valore_voce(VoceLimite::Elenco(v), 0), false), VoceLimite::Elenco(v));
        }
        // Personalizzato tiene il numero che c'era, o parte da 240 se non c'era limite.
        assert_eq!(valore_voce(VoceLimite::Personalizzato, 144), 144);
        assert_eq!(valore_voce(VoceLimite::Personalizzato, 0), 240);
        assert!((LIMITI_FPS.0..=LIMITI_FPS.1).contains(&valore_voce(VoceLimite::Personalizzato, 0)));
        assert!(LIMITI_ELENCO.iter().all(|v| (LIMITI_FPS.0..=LIMITI_FPS.1).contains(v)));
    }

    #[test]
    fn la_voce_pari_agli_hz_dello_schermo_lo_dice() {
        assert_eq!(nome_voce(VoceLimite::Senza, 240), "Senza limite");
        assert_eq!(nome_voce(VoceLimite::Elenco(240), 240), "240 fps (schermo)");
        assert_eq!(nome_voce(VoceLimite::Elenco(144), 240), "144 fps");
        assert_eq!(nome_voce(VoceLimite::Elenco(144), 144), "144 fps (schermo)");
        assert_eq!(nome_voce(VoceLimite::Elenco(60), 0), "60 fps");
        assert_eq!(nome_voce(VoceLimite::Personalizzato, 240), "Personalizzato…");
        assert_eq!(hz_intero(239.96), 240);
        assert_eq!(hz_intero(59.94), 60);
        assert_eq!(hz_intero(0.0), 0);
    }

    #[test]
    fn il_grafico_ha_spazio_sopra_il_massimo() {
        assert_eq!(cima_grafico(&[], 0), 5);
        assert_eq!(cima_grafico(&[10, 20], 18), 25);
        assert_eq!(cima_grafico(&[10, 20], 40), 45);
        assert!(cima_grafico(&[44], 0) > 44);
    }
}
