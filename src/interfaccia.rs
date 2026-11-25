//! I pannelli con egui: menu con le impostazioni, pausa, riepilogo, statistiche, pannello della latenza.
//! egui gira solo fuori dal giro (e, nel giro, solo per il pannello della latenza quando è aperto, 4 volte al
//! secondo): nel giro il frame non passa da egui.

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, Pos2, RichText, Sense, Shape, Stroke, Ui};

use crate::aggiornamento::Stato as StatoAggiornamento;
use crate::config::{Config, LIMITI_DPI, LIMITI_FOV, LIMITI_FPS, LIMITI_MOLTIPLICATORE_Y};
use crate::giro::Riepilogo;
use crate::scenari::{LIMITI_PARAMETRO, Parametri, Scenario, TipoMovimento};
use crate::sensibilita::{
    Gioco, LIMITI_SECONDI_VARIAZIONE, LIMITI_VARIAZIONE, Ogni, Variazione, cm_per_360, converti,
    gradi_per_count,
};
use crate::storico::{Confronto, GiroSalvato, Storico};

pub const ACCENTO: Color32 = Color32::from_rgb(255, 176, 33);
pub const TENUE: Color32 = Color32::from_rgb(161, 163, 169);
pub const AVVISO: Color32 = Color32::from_rgb(255, 210, 122);

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
    /// "Senti": il suono del colpo, col volume scelto.
    ProvaSuono,
    Chiudi,
}

/// Le parti del menu delle impostazioni.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sezione {
    #[default]
    Gioco,
    Mirino,
    Aspetto,
    Suoni,
}

impl Sezione {
    pub const TUTTE: [Sezione; 4] = [Sezione::Gioco, Sezione::Mirino, Sezione::Aspetto, Sezione::Suoni];

    pub fn nome(self) -> &'static str {
        match self {
            Sezione::Gioco => "Sensibilità",
            Sezione::Mirino => "Mirino",
            Sezione::Aspetto => "Bersagli e stanza",
            Sezione::Suoni => "Suoni",
        }
    }
}

/// La sensibilità a caso: acceso, ±N%, a ogni giro o ogni N secondi. Vero se è cambiata.
fn riga_variazione(ui: &mut Ui, v: &mut Variazione) -> bool {
    let prima = *v;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.checkbox(&mut v.attiva, "entro ±");
            ui.add_enabled(
                v.attiva,
                egui::DragValue::new(&mut v.percento)
                    .range(LIMITI_VARIAZIONE.0..=LIMITI_VARIAZIONE.1)
                    .speed(0.2)
                    .max_decimals(1)
                    .suffix("%"),
            );
        });
        if v.attiva {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("variazione ogni")
                    .selected_text(if v.ogni == Ogni::Giro { "a ogni giro" } else { "ogni" })
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut v.ogni, Ogni::Giro, "a ogni giro");
                        ui.selectable_value(&mut v.ogni, Ogni::Secondi, "ogni");
                    });
                if v.ogni == Ogni::Secondi {
                    ui.add(
                        egui::DragValue::new(&mut v.secondi)
                            .range(LIMITI_SECONDI_VARIAZIONE.0..=LIMITI_SECONDI_VARIAZIONE.1)
                            .speed(0.1)
                            .max_decimals(1)
                            .suffix(" s"),
                    );
                }
            });
        }
    });
    *v != prima
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
    /// L'uscita audio non si è aperta (o si è persa): i suoni sono muti.
    pub audio_guasto: bool,
    /// Gli scenari da scegliere e gli avvisi dei file di scenario saltati.
    pub scenari: &'a [Scenario],
    pub avvisi_scenari: &'a [String],
}

/// Un punteggio come si legge: i colpi, o la percentuale del tracking.
pub fn valore(punteggio: f64, tracking: bool) -> String {
    if tracking { format!("{punteggio:.1}%") } else { format!("{punteggio:.0} colpi") }
}

/// La riga sotto la scelta dello scenario: cosa si fa e come si conta.
pub fn spiegazione(s: &Scenario) -> String {
    let secondi = s.regole.durata_ms / 1000.0;
    let conto = if s.regole.tracking() {
        "conta il tempo con il mirino sulla sfera e il tasto sinistro premuto"
    } else {
        "conta i colpi; ogni clic a vuoto è un errore"
    };
    format!("{} {secondi:.0} secondi, {conto}.", s.descrizione)
}

/// Un campo in percentuale (50-200%, a passi di 5) per un parametro; vero se è cambiato.
fn campo_parametro(ui: &mut Ui, valore: &mut f64, attivo: bool) -> bool {
    let mut p = (*valore * 100.0).round();
    let r = ui.add_enabled(
        attivo,
        egui::DragValue::new(&mut p)
            .range(LIMITI_PARAMETRO.0 * 100.0..=LIMITI_PARAMETRO.1 * 100.0)
            .speed(1.0)
            .max_decimals(0)
            .suffix("%"),
    );
    let nuovo = ((p / 5.0).round() * 5.0 / 100.0).clamp(LIMITI_PARAMETRO.0, LIMITI_PARAMETRO.1);
    if r.changed() && (nuovo - *valore).abs() > 1e-9 {
        *valore = nuovo;
        return true;
    }
    false
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
pub fn aggiornamento(ui: &mut Ui, build: Option<u32>, stato: &StatoAggiornamento) -> Azione {
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

pub fn scheda(ui: &mut Ui, nome: &str, contenuto: impl FnOnce(&mut Ui) -> Azione) -> Azione {
    scheda_larga(ui, nome, 440.0, contenuto)
}

/// Un riquadro centrato. `nome` gli dà un id suo: egui centra l'area con la misura che aveva nel frame prima, e con
/// un id in comune il primo frame di un riquadro prenderebbe la misura di un altro (la pausa usciva più in alto,
/// con l'altezza del riepilogo).
pub fn scheda_larga(
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

pub fn pulsante(ui: &mut Ui, testo: &str, principale: bool) -> bool {
    let t = RichText::new(testo).size(16.0).strong();
    let b = if principale {
        egui::Button::new(t.color(Color32::from_rgb(23, 19, 10))).fill(ACCENTO)
    } else {
        egui::Button::new(t)
    };
    ui.add_sized([ui.available_width(), 38.0], b).clicked()
}

/// Mentre si scarica o si riavvia un aggiornamento non si gioca: la rete non lavora durante il giro.
pub fn si_gioca(info: &InfoMenu) -> bool {
    !matches!(info.aggiornamento, StatoAggiornamento::Scarico { .. } | StatoAggiornamento::Pronto(_))
}

/// Le righe della sensibilità in una griglia a due colonne: gioco, sensibilità, verticale, sensibilità a caso e FOV
/// (se `complete`) e DPI. Vero se qualcosa è cambiato.
pub fn righe_sensibilita(ui: &mut Ui, c: &mut Config, complete: bool) -> bool {
    let mut cambiato = false;
    ui.label("Sensibilità di");
    let prima = c.gioco;
    egui::ComboBox::from_id_salt("gioco").selected_text(c.gioco.nome()).height(400.0).show_ui(ui, |ui| {
        for g in Gioco::TUTTI {
            ui.selectable_value(&mut c.gioco, g, g.nome());
        }
    });
    if c.gioco != prima {
        // Stessi centimetri per giro nel gioco nuovo; FOV al valore di base del gioco.
        // Dentro i limiti: un valore estremo convertito si ferma al limite invece di tornare al predefinito.
        let (min, max) = c.gioco.limiti_sensibilita();
        c.sensibilita = ((converti(prima, c.sensibilita, c.gioco, c.dpi) * 1_000_000.0).round()
            / 1_000_000.0)
            .clamp(min, max);
        c.fov = c.gioco.fov_predefinito();
        cambiato = true;
    }
    ui.end_row();

    ui.label(c.gioco.nome_sensibilita());
    let (min, max) = c.gioco.limiti_sensibilita();
    let (passo, decimali) = if c.gioco == Gioco::Cm360 { (0.1, 2) } else { (0.005, 4) };
    cambiato |= ui
        .add(egui::DragValue::new(&mut c.sensibilita).range(min..=max).speed(passo).max_decimals(decimali))
        .changed();
    ui.end_row();

    if complete {
        ui.label("Verticale");
        cambiato |= ui
            .add(
                egui::DragValue::new(&mut c.moltiplicatore_y)
                    .range(LIMITI_MOLTIPLICATORE_Y.0..=LIMITI_MOLTIPLICATORE_Y.1)
                    .speed(0.01)
                    .max_decimals(2)
                    .prefix("× "),
            )
            .on_hover_text("La sensibilità in verticale rispetto a quella orizzontale: 1 = uguale.")
            .changed();
        ui.end_row();

        ui.label("Sensibilità a caso");
        cambiato |= riga_variazione(ui, &mut c.variazione);
        ui.end_row();

        ui.label(c.gioco.descrizione_fov());
        cambiato |= ui
            .add(
                egui::DragValue::new(&mut c.fov)
                    .range(LIMITI_FOV.0..=LIMITI_FOV.1)
                    .speed(0.1)
                    .max_decimals(2),
            )
            .changed();
        ui.end_row();
    }

    ui.label("DPI del mouse");
    cambiato |=
        ui.add(egui::DragValue::new(&mut c.dpi).range(LIMITI_DPI.0..=LIMITI_DPI.1).speed(10.0)).changed();
    ui.end_row();
    cambiato
}

/// La riga sotto la sensibilità: gradi per count e centimetri per giro.
pub fn riga_cm(ui: &mut Ui, c: &Config) {
    ui.label(
        RichText::new(format!(
            "Ogni count del mouse gira la visuale di {:.4} gradi: a {:.0} DPI un giro completo è {:.1} cm.",
            gradi_per_count(c.gioco, c.sensibilita, c.dpi),
            c.dpi,
            cm_per_360(c.gioco, c.sensibilita, c.dpi)
        ))
        .color(TENUE)
        .size(13.0),
    );
}

/// Il contenuto della pagina Impostazioni della home: sensibilità e gioco, mirino, bersagli e stanza, suoni.
pub fn impostazioni(ui: &mut Ui, c: &mut Config, info: &InfoMenu) -> Azione {
    let mut azione = Azione::Nessuna;
    titolo_pagina(ui, "Impostazioni", "Valgono per tutti gli scenari e si salvano da sole.");
    let id_sezione = egui::Id::unique("sezione delle impostazioni");
    let mut sezione = ui.data(|d| d.get_temp::<Sezione>(id_sezione)).unwrap_or_default();
    ui.horizontal(|ui| {
        for s in Sezione::TUTTE {
            ui.selectable_value(&mut sezione, s, RichText::new(s.nome()).size(15.0));
        }
    });
    ui.data_mut(|d| d.insert_temp(id_sezione, sezione));
    ui.add_space(6.0);
    match sezione {
        Sezione::Gioco => {
            if impostazioni_gioco(ui, c, info) {
                azione = Azione::Impostazioni;
            }
        }
        Sezione::Mirino => {
            if crate::impostazioni::mirino(ui, c) {
                azione = Azione::Impostazioni;
            }
        }
        Sezione::Aspetto => {
            if crate::impostazioni::aspetto(ui, c) {
                azione = Azione::Impostazioni;
            }
        }
        Sezione::Suoni => match crate::impostazioni::suoni(ui, c, info.audio_guasto) {
            Some(true) => azione = Azione::ProvaSuono,
            Some(false) => azione = Azione::Impostazioni,
            None => {}
        },
    }
    if let Some(a) = info.avviso {
        ui.label(RichText::new(a).color(AVVISO).size(13.0));
    }
    ui.add_space(10.0);
    ui.label(RichText::new("Tasti").size(15.0).strong());
    for riga in [
        "Esc mette in pausa il giro (e lo riprende).",
        "R ricomincia subito il giro, anche dal riepilogo.",
        "Spazio, o un clic, salta il conto alla rovescia.",
        "F3 apre il pannello della latenza.",
    ] {
        ui.label(RichText::new(riga).color(TENUE).size(13.0));
    }
    ui.add_space(6.0);
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
}

/// La sezione Sensibilità delle impostazioni: sensibilità, vsync, limite, schermo intero, nel giro, aggiornamenti.
fn impostazioni_gioco(ui: &mut Ui, c: &mut Config, info: &InfoMenu) -> bool {
    let mut cambiato = false;
    egui::Grid::new("impostazioni").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        cambiato |= righe_sensibilita(ui, c, true);

        ui.label("Vsync");
        cambiato |= ui.checkbox(&mut c.vsync, "acceso (più latenza, niente tearing)").changed();
        ui.end_row();

        if c.vsync {
            // Col vsync il limite non vale: la riga resta, grigia, e dice perché.
            // Disegnate come disattivate, cioè più scure delle altre righe.
            ui.add_enabled(false, egui::Label::new("Limite fps"));
            // Alta quanto la tendina: accendendo il vsync la pagina non si sposta.
            let alta = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
            ui.allocate_ui_with_layout(
                egui::vec2(0.0, alta),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| ui.add_enabled(false, egui::Label::new("col vsync decide lo schermo")),
            );
        } else {
            ui.label("Limite fps");
            cambiato |= riga_limite(ui, c, info.hz_schermo);
        }
        ui.end_row();

        ui.label("Schermo intero");
        cambiato |=
            ui.checkbox(&mut c.schermo_intero_esclusivo, "esclusivo (vale dal prossimo avvio)").changed();
        ui.end_row();

        ui.label("Nel giro");
        cambiato |= ui
            .checkbox(&mut c.punteggio_dal_vivo, "punteggio e precisione dal vivo, in alto a sinistra")
            .changed();
        ui.end_row();

        ui.label("Aggiornamenti");
        cambiato |= ui.checkbox(&mut c.controlla_aggiornamenti, "controlla all'avvio").changed();
        ui.end_row();
    });
    ui.add_space(2.0);
    riga_cm(ui, c);
    cambiato
}

/// Dimensione e velocità dei bersagli (uguali per tutti gli scenari), nella pagina dello scenario. Vero se cambiano.
pub fn parametri_scenario(ui: &mut Ui, c: &mut Config, s: &Scenario) -> bool {
    let mut cambiato = false;
    let si_muove = s.regole.movimento != TipoMovimento::Fermo;
    egui::Grid::new("parametri").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        ui.label("Dimensione bersagli");
        cambiato |= campo_parametro(ui, &mut c.dimensione, true);
        ui.end_row();
        ui.label("Velocità bersagli");
        ui.horizontal(|ui| {
            cambiato |= campo_parametro(ui, &mut c.velocita, si_muove);
            if !si_muove {
                ui.label(RichText::new("le sfere di questo scenario stanno ferme").color(TENUE).size(12.0));
            }
        });
        ui.end_row();
    });
    if s.regole.parametri_che_valgono(c.parametri()).standard() {
        ui.label(RichText::new("Parametri standard: i giri fanno record.").color(TENUE).size(13.0));
    } else {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Parametri cambiati: i giri si salvano ma non fanno record.")
                    .color(AVVISO)
                    .size(13.0),
            );
            if ui.small_button("Torna allo standard").clicked() {
                (c.dimensione, c.velocita) = (Parametri::STANDARD.dimensione, Parametri::STANDARD.velocita);
                cambiato = true;
            }
        });
    }
    cambiato
}

/// Il titolo di una pagina della home, con una riga sotto.
pub fn titolo_pagina(ui: &mut Ui, titolo: &str, riga: &str) {
    ui.label(RichText::new(titolo).size(26.0).strong());
    if !riga.is_empty() {
        ui.label(RichText::new(riga).color(TENUE));
    }
    ui.add_space(8.0);
}

/// Un pulsante che risponde solo quando `attesa` è finita (Riprendi, Rigioca): prima è spento, e il menu si ridisegna
/// da solo quando si accende.
fn pulsante_dopo(ui: &mut Ui, testo: &str, attesa: std::time::Duration) -> bool {
    if !attesa.is_zero() {
        ui.ctx().request_repaint_after(attesa);
    }
    ui.add_enabled_ui(attesa.is_zero(), |ui| pulsante(ui, testo, true)).inner && attesa.is_zero()
}

pub fn pausa(ui: &mut Ui, secondi: u32, attesa: std::time::Duration) -> Azione {
    scheda(ui, "pausa", |ui| {
        let mut azione = Azione::Nessuna;
        ui.label(RichText::new("In pausa").size(22.0).strong());
        ui.label(RichText::new(format!("Restano {secondi} secondi. Il tempo è fermo.")).color(TENUE));
        ui.add_space(6.0);
        if pulsante_dopo(ui, "Riprendi", attesa) {
            azione = Azione::Riprendi;
        }
        if pulsante(ui, "Esci dal giro", false) {
            azione = Azione::EsciDalGiro;
        }
        azione
    })
}

/// Il confronto col record come si legge nel riepilogo: il record di prima, la differenza, e se è un record nuovo.
pub fn testo_confronto(c: &Confronto, tracking: bool) -> (String, String, bool) {
    let record = match c.record_prima {
        Some(r) => valore(r, tracking),
        None => "nessuno prima di questo giro".to_string(),
    };
    let differenza = match c.differenza() {
        // Pari quando la differenza, arrotondata come si mostra, è zero.
        Some(d) if valore(d.abs(), tracking) == valore(0.0, tracking) => "pari al record".to_string(),
        Some(d) if d > 0.0 => format!("+{}", valore(d, tracking)),
        Some(d) => format!("-{}", valore(-d, tracking)),
        None => "primo giro".to_string(),
    };
    (record, differenza, c.nuovo_record())
}

/// Quello che il riepilogo mostra oltre ai numeri del giro.
pub struct InfoRiepilogo<'a> {
    pub scenario: &'a Scenario,
    pub parametri: Parametri,
    pub confronto: Option<&'a Confronto>,
    pub avviso: Option<&'a str>,
    /// La sensibilità del giro, se era a caso.
    pub sensibilita_usata: Option<&'a str>,
    /// Quanto manca perché Rigioca risponda.
    pub attesa: std::time::Duration,
}

/// "120%" per un parametro.
fn percento(v: f64) -> String {
    format!("{:.0}%", v * 100.0)
}

pub fn riepilogo(ui: &mut Ui, r: &Riepilogo, info: &InfoRiepilogo) -> Azione {
    scheda(ui, "riepilogo", |ui| {
        let mut azione = Azione::Nessuna;
        let tracking = r.tracking.is_some();
        ui.label(RichText::new("Fine del giro").size(22.0).strong());
        ui.label(RichText::new(&info.scenario.nome).color(TENUE));
        if let Some(c) = info.confronto.filter(|c| c.nuovo_record()) {
            ui.label(
                RichText::new(format!("Nuovo record: {}", valore(c.punteggio, tracking)))
                    .size(17.0)
                    .strong()
                    .color(ACCENTO),
            );
        }
        ui.add_space(4.0);
        let precisione = r.precisione.map_or("nessun clic".to_string(), |p| format!("{:.1}%", p * 100.0));
        let tempo = r.tempo_medio_ms.map_or("nessun colpo".to_string(), |t| format!("{t:.0} ms"));
        let righe: Vec<(&str, String)> = match r.tracking {
            Some(t) => vec![
                ("Sul bersaglio", format!("{:.1}%", t * 100.0)),
                (
                    "Tempo sul bersaglio",
                    format!("{:.1} s di {:.0}", t * info.scenario.regole.durata_ms / 1000.0, {
                        info.scenario.regole.durata_ms / 1000.0
                    }),
                ),
            ],
            None => vec![
                ("Colpi", r.colpi.to_string()),
                ("Errori", r.errori.to_string()),
                ("Precisione", precisione),
                (
                    if info.scenario.regole.sfere > 1 {
                        "Tempo medio fra i colpi"
                    } else {
                        "Tempo medio per bersaglio"
                    },
                    tempo,
                ),
            ],
        };
        egui::Grid::new("riepilogo").num_columns(2).spacing([40.0, 10.0]).show(ui, |ui| {
            for (nome, valore) in righe {
                ui.label(nome);
                ui.label(RichText::new(valore).strong().size(16.0));
                ui.end_row();
            }
            if let Some(c) = info.confronto {
                let (record, differenza, _) = testo_confronto(c, tracking);
                ui.label(format!("Record di {}", info.scenario.nome));
                ui.label(RichText::new(record).strong().size(16.0));
                ui.end_row();
                ui.label("Rispetto al record");
                ui.label(RichText::new(differenza).strong().size(16.0));
                ui.end_row();
            }
            if let Some(s) = info.sensibilita_usata {
                ui.label("Sensibilità usata");
                ui.label(RichText::new(s).strong().size(16.0));
                ui.end_row();
            }
        });
        if !info.parametri.standard() {
            ui.label(
                RichText::new(format!(
                    "Parametri cambiati (dimensione {}, velocità {}): il giro è salvato ma non conta per il record.",
                    percento(info.parametri.dimensione),
                    percento(info.parametri.velocita)
                ))
                .color(AVVISO)
                .size(13.0),
            );
        }
        if let Some(a) = info.avviso {
            ui.label(RichText::new(a).color(AVVISO).size(13.0));
        }
        ui.add_space(8.0);
        if pulsante_dopo(ui, "Rigioca", info.attesa) {
            azione = Azione::Inizia;
        }
        if pulsante(ui, "Statistiche", false) {
            azione = Azione::Statistiche;
        }
        if pulsante(ui, "Torna alla home", false) {
            azione = Azione::TornaAlMenu;
        }
        ui.label(RichText::new("R rigioca subito, anche adesso.").color(TENUE).size(12.0));
        azione
    })
}

/// Giri mostrati nel grafico.
pub const GIRI_NEL_GRAFICO: usize = 30;

/// Il valore in cima al grafico: un po' sopra il più alto fra giri e record, arrotondato a 5 (al più 100 nel tracking).
pub fn cima_grafico(punti: &[f64], record: f64, tracking: bool) -> f64 {
    let massimo = punti.iter().copied().fold(record, f64::max).max(0.0).floor() as u32;
    let cima = f64::from((massimo + massimo / 10 + 1).div_ceil(5).max(1) * 5);
    if tracking { cima.min(100.0) } else { cima }
}

fn grafico(ui: &mut Ui, giri: &[&GiroSalvato], record: f64, tracking: bool) {
    let colpi: Vec<f64> = giri.iter().map(|g| g.punteggio()).collect();
    let cima = cima_grafico(&colpi, record, tracking) as f32;
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
        format!("record {}", if tracking { format!("{record:.1}%") } else { format!("{record:.0}") }),
        font,
        chiaro,
    );
    // Con pochi giri i punti si allargano su tutto il grafico, il più recente sempre a destra.
    let passo = dentro.width() / colpi.len().saturating_sub(1).max(1) as f32;
    let ultimo = colpi.len().saturating_sub(1);
    let punti: Vec<Pos2> = colpi
        .iter()
        .enumerate()
        .map(|(i, &c)| Pos2::new(dentro.right() - passo * (ultimo - i) as f32, y(c as f32).max(dentro.top())))
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

/// Colonne dell'elenco dei giri: titolo e larghezza. Nel tracking la seconda è "Sul bersaglio".
const COLONNE: [(&str, f32); 8] = [
    ("Data", 140.0),
    ("Colpi", 80.0),
    ("Errori", 55.0),
    ("Precisione", 85.0),
    ("Tempo medio", 95.0),
    ("Overshoot", 80.0),
    ("Sensibilità", 105.0),
    ("Parametri", 90.0),
];

fn riga(ui: &mut Ui, celle: [String; 8], intestazione: bool) {
    ui.horizontal(|ui| {
        for ((_, w), testo) in COLONNE.iter().zip(celle) {
            let t = RichText::new(testo).size(13.0);
            let t = if intestazione { t.color(TENUE) } else { t };
            ui.add_sized([*w, 18.0], egui::Label::new(t).truncate());
        }
    });
}

/// Gli scenari che la schermata Statistiche può mostrare: quelli caricati, più gli id che stanno solo nello storico
/// (uno scenario dell'utente tolto dalla cartella), con il loro id come nome. Id, nome, tracking.
pub fn scenari_delle_statistiche(scenari: &[Scenario], s: &Storico) -> Vec<(String, String, bool)> {
    let mut v: Vec<(String, String, bool)> =
        scenari.iter().map(|x| (x.id.clone(), x.nome.clone(), x.regole.tracking())).collect();
    for g in s.giri() {
        if !v.iter().any(|(id, _, _)| *id == g.scenario) {
            v.push((g.scenario.clone(), g.scenario.clone(), g.tracking.is_some()));
        }
    }
    v
}

/// La schermata Statistiche di uno scenario: grafico degli ultimi giri con il record, la sessione e l'elenco.
pub fn statistiche(
    ui: &mut Ui,
    s: &Storico,
    scenari: &[Scenario],
    scelto: &mut String,
    esportato: Option<&str>,
) -> Azione {
    // Il contenuto della pagina Statistiche della home.
    let mut contenuto = |ui: &mut Ui| {
        let mut azione = Azione::Nessuna;
        ui.label(RichText::new("Statistiche").size(26.0).strong());
        let elenco = scenari_delle_statistiche(scenari, s);
        let (nome, tracking) = elenco
            .iter()
            .find(|(id, _, _)| id == scelto)
            .map_or((scelto.clone(), false), |(_, n, t)| (n.clone(), *t));
        ui.horizontal(|ui| {
            ui.label("Scenario");
            egui::ComboBox::from_id_salt("scenario statistiche")
                .selected_text(&nome)
                .width(220.0)
                .height(400.0)
                .show_ui(ui, |ui| {
                    for (id, n, _) in &elenco {
                        let giri = s.giri_di(id).count();
                        ui.selectable_value(scelto, id.clone(), format!("{n} ({giri})"));
                    }
                });
        });
        let tutti: Vec<&GiroSalvato> = s.giri_di(scelto).collect();
        let giri: Vec<&GiroSalvato> = tutti.iter().copied().filter(|g| g.standard()).collect();
        let cambiati = tutti.len() - giri.len();
        let unita = if tracking { "Tempo sul bersaglio" } else { "Colpi" };
        if tutti.is_empty() {
            ui.add_space(6.0);
            ui.label(format!(
                "Nessun giro salvato di {nome}. Ogni giro finito compare qui, con il record e l'andamento."
            ));
        } else {
            let record = s.record(scelto).unwrap_or(0.0);
            let sessione = s.sessione(scelto);
            ui.add_space(4.0);
            let inizio = giri.len().saturating_sub(GIRI_NEL_GRAFICO);
            let n = giri.len() - inizio;
            ui.label(
                RichText::new(match n {
                    0 => "Nessun giro coi parametri standard: il grafico e il record contano solo quelli"
                        .to_string(),
                    1 => format!("{unita} dell'ultimo giro, con la linea del record"),
                    _ => format!("{unita} degli ultimi {n} giri, con la linea del record"),
                })
                .size(13.0)
                .color(TENUE),
            );
            grafico(ui, &giri[inizio..], record, tracking);
            ui.add_space(2.0);
            egui::Grid::new("sessione").num_columns(2).spacing([40.0, 8.0]).show(ui, |ui| {
                ui.label("Record");
                ui.label(
                    RichText::new(if giri.is_empty() { "nessuno".into() } else { valore(record, tracking) })
                        .strong(),
                );
                ui.end_row();
                ui.label("Da quando hai aperto aimpro");
                let testo = match (sessione.media, sessione.migliore) {
                    (Some(m), Some(b)) => format!(
                        "{} {}, media {}, migliore {}",
                        sessione.giri,
                        if sessione.giri == 1 { "giro" } else { "giri" },
                        if tracking { format!("{m:.1}%") } else { format!("{m:.1} colpi") },
                        valore(b, tracking)
                    ),
                    _ => "nessun giro".to_string(),
                };
                ui.label(RichText::new(testo).strong());
                ui.end_row();
                ui.label("Giri salvati");
                ui.label(
                    RichText::new(if cambiati > 0 {
                        format!(
                            "{}, di cui {cambiati} coi parametri cambiati (fuori da record e grafico)",
                            tutti.len()
                        )
                    } else {
                        tutti.len().to_string()
                    })
                    .strong(),
                );
                ui.end_row();
            });
            ui.separator();
            let mut titoli = COLONNE.map(|(t, _)| t.to_string());
            if tracking {
                titoli[1] = "Sul bersaglio".to_string();
            }
            riga(ui, titoli, true);
            egui::ScrollArea::vertical().max_height(200.0).auto_shrink([false, true]).show_rows(
                ui,
                18.0,
                tutti.len(),
                |ui, righe| {
                    for i in righe {
                        // Il più recente in alto.
                        let g = tutti[tutti.len() - 1 - i];
                        let col_tracking = g.tracking.is_some();
                        riga(
                            ui,
                            [
                                g.data.clone(),
                                if col_tracking { percentuale(g.tracking) } else { g.colpi.to_string() },
                                if col_tracking { "-".into() } else { g.errori.to_string() },
                                percentuale(g.precisione),
                                millisecondi(g.tempo_medio_ms),
                                if col_tracking { "-".into() } else { g.overshoot.to_string() },
                                format!("{} {}", g.impostazioni.gioco.nome(), g.impostazioni.sensibilita),
                                if g.standard() {
                                    "standard".into()
                                } else {
                                    format!(
                                        "{} / {}",
                                        percento(g.parametri.dimensione),
                                        percento(g.parametri.velocita)
                                    )
                                },
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
        let esporta = egui::Button::new(RichText::new("Esporta CSV").size(16.0).strong());
        if ui.add_enabled_ui(!s.giri().is_empty(), |ui| ui.add_sized([240.0, 38.0], esporta)).inner.clicked()
        {
            azione = Azione::EsportaCsv;
        }
        azione
    };
    contenuto(ui)
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

    fn c(punteggio: f64, record_prima: Option<f64>) -> Confronto {
        Confronto { punteggio, record_prima, standard: true }
    }

    #[test]
    fn il_confronto_dice_record_differenza_e_record_nuovo() {
        let s = |a: &str, b: &str, n| (a.to_string(), b.to_string(), n);
        assert_eq!(testo_confronto(&c(34.0, Some(30.0)), false), s("30 colpi", "+4 colpi", true));
        assert_eq!(testo_confronto(&c(25.0, Some(30.0)), false), s("30 colpi", "-5 colpi", false));
        assert_eq!(testo_confronto(&c(30.0, Some(30.0)), false), s("30 colpi", "pari al record", false));
        assert_eq!(
            testo_confronto(&c(12.0, None), false),
            s("nessuno prima di questo giro", "primo giro", true)
        );
    }

    #[test]
    fn il_confronto_del_tracking_e_in_percentuale() {
        let s = |a: &str, b: &str, n| (a.to_string(), b.to_string(), n);
        assert_eq!(testo_confronto(&c(73.44, Some(70.0)), true), s("70.0%", "+3.4%", true));
        assert_eq!(testo_confronto(&c(65.0, Some(70.0)), true), s("70.0%", "-5.0%", false));
        assert_eq!(testo_confronto(&c(70.01, Some(70.0)), true).1, "pari al record");
    }

    #[test]
    fn coi_parametri_cambiati_niente_record_nuovo() {
        let x = Confronto { punteggio: 99.0, record_prima: Some(30.0), standard: false };
        assert!(!testo_confronto(&x, false).2);
    }

    #[test]
    fn la_spiegazione_dice_durata_e_conto() {
        let s = crate::scenari::di_aimpro();
        assert!(spiegazione(&s[0]).contains("30 secondi, conta i colpi"));
        assert!(spiegazione(s.iter().find(|x| x.id == "filo").unwrap()).contains("tasto sinistro premuto"));
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
        assert_eq!(cima_grafico(&[], 0.0, false), 5.0);
        assert_eq!(cima_grafico(&[10.0, 20.0], 18.0, false), 25.0);
        assert_eq!(cima_grafico(&[10.0, 20.0], 40.0, false), 45.0);
        assert!(cima_grafico(&[44.0], 0.0, false) > 44.0);
        assert_eq!(cima_grafico(&[97.5], 90.0, true), 100.0, "il tracking arriva al più a 100");
    }
}
