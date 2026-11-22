//! I pannelli con egui: menu con le impostazioni, pausa, riepilogo, pannello della latenza.
//! egui gira solo fuori dal giro (e, nel giro, solo per il pannello della latenza quando è aperto, 4 volte al
//! secondo): nel giro il frame non passa da egui.

use egui::{Align2, Color32, CornerRadius, Frame, Margin, RichText, Stroke, Ui};

use crate::config::{Config, LIMITI_DPI, LIMITI_FOV, LIMITI_FPS, LIMITI_SENSIBILITA};
use crate::giro::Riepilogo;
use crate::sensibilita::{Gioco, cm_per_360, converti, gradi_per_count};

pub const ACCENTO: Color32 = Color32::from_rgb(255, 176, 33);
const TENUE: Color32 = Color32::from_rgb(161, 163, 169);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Azione {
    Nessuna,
    Inizia,
    Riprendi,
    EsciDalGiro,
    TornaAlMenu,
    /// Le impostazioni sono cambiate: si salvano e si applicano.
    Impostazioni,
    Chiudi,
}

/// Informazioni da mostrare nel menu.
pub struct InfoMenu<'a> {
    pub scheda: &'a str,
    pub presentazione: &'a str,
    pub avviso: Option<&'a str>,
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

fn scheda(ui: &mut Ui, contenuto: impl FnOnce(&mut Ui) -> Azione) -> Azione {
    let mut azione = Azione::Nessuna;
    egui::Area::new(egui::Id::unique("scheda")).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(
        ui.ctx(),
        |ui| {
            Frame::new()
                .fill(Color32::from_rgba_unmultiplied(22, 23, 26, 240))
                .stroke(Stroke::new(1.0, Color32::from_white_alpha(26)))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(28))
                .show(ui, |ui| {
                    ui.set_width(440.0);
                    azione = contenuto(ui);
                });
        },
    );
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
    scheda(ui, |ui| {
        let mut azione = Azione::Nessuna;
        ui.label(RichText::new("aimpro").size(28.0).strong());
        ui.label(RichText::new("Scatto: 30 secondi, una sfera alla volta").color(TENUE));
        ui.add_space(4.0);
        ui.label("Colpisci la sfera: appena la prendi ne compare un'altra in un altro punto. Ogni clic a vuoto è un errore.");
        ui.separator();

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

            ui.label("Limite fps");
            ui.horizontal(|ui| {
                let mut attivo = c.limite_fps != 0;
                if ui.add_enabled(!c.vsync, egui::Checkbox::without_text(&mut attivo)).changed() {
                    c.limite_fps = if attivo { 240 } else { 0 };
                    azione = Azione::Impostazioni;
                }
                if attivo
                    && ui
                        .add_enabled(
                            !c.vsync,
                            egui::DragValue::new(&mut c.limite_fps)
                                .range(LIMITI_FPS.0..=LIMITI_FPS.1)
                                .speed(1.0)
                                .suffix(" fps"),
                        )
                        .changed()
                {
                    azione = Azione::Impostazioni;
                }
            });
            ui.end_row();

            ui.label("Schermo intero");
            if ui.checkbox(&mut c.schermo_intero_esclusivo, "esclusivo (vale dal prossimo avvio)").changed() {
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
            ui.label(RichText::new(a).color(Color32::from_rgb(255, 210, 122)).size(13.0));
        }
        ui.add_space(6.0);
        if pulsante(ui, "Inizia", true) {
            azione = Azione::Inizia;
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
                "{} · {}",
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
    scheda(ui, |ui| {
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

pub fn riepilogo(ui: &mut Ui, r: &Riepilogo) -> Azione {
    scheda(ui, |ui| {
        let mut azione = Azione::Nessuna;
        ui.label(RichText::new("Fine del giro").size(22.0).strong());
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
        });
        ui.add_space(8.0);
        if pulsante(ui, "Rigioca", true) {
            azione = Azione::Inizia;
        }
        if pulsante(ui, "Impostazioni", false) {
            azione = Azione::TornaAlMenu;
        }
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
