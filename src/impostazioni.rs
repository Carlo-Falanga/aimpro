//! Le parti del menu per mirino, bersagli e stanza, suoni. Solo fuori dal giro, come tutto egui.

use egui::{Color32, CornerRadius, RichText, Sense, Stroke, Ui};

use crate::aspetto::{ColoriBersaglio, Tema};
use crate::config::{Config, LIMITI_VOLUME};
use crate::mirino::{self, Forma, LIMITI_BORDO, LIMITI_LUNGHEZZA, LIMITI_SPAZIO, LIMITI_SPESSORE, Mirino};

const TENUE: Color32 = Color32::from_rgb(161, 163, 169);

fn colore32(c: [f32; 4]) -> Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(b(c[0]), b(c[1]), b(c[2]), b(c[3]))
}

fn colore3(c: [f32; 3]) -> Color32 {
    colore32([c[0], c[1], c[2], 1.0])
}

/// L'anteprima del mirino sulla parete della stanza del tema scelto, con gli stessi pixel del giro
/// (`mirino::pezzi`).
fn anteprima(ui: &mut Ui, m: &Mirino, tema: Tema) {
    let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.0), Sense::hover());
    let p = ui.painter_at(area);
    let stanza = tema.colori();
    p.rect_filled(area, CornerRadius::same(8), colore3(stanza.pareti));
    // Le linee della griglia, come sulla parete del giro.
    let linee = colore3(stanza.linee).gamma_multiply(0.6);
    let passo = 24.0;
    let mut x = area.left() + (area.width() % passo) / 2.0;
    while x < area.right() {
        p.vline(x, area.y_range(), Stroke::new(1.0, linee));
        x += passo;
    }
    let mut y = area.top() + (area.height() % passo) / 2.0;
    while y < area.bottom() {
        p.hline(area.x_range(), y, Stroke::new(1.0, linee));
        y += passo;
    }
    // In pixel veri, come nel giro, poi riportati ai punti di egui.
    let ppp = ui.ctx().pixels_per_point();
    let centro = area.center() * ppp;
    let mut pezzi = [mirino::Pezzo::default(); mirino::MAX_PEZZI];
    let n = mirino::pezzi(m, centro.x, centro.y, ppp, &mut pezzi);
    for q in &pezzi[..n] {
        let c = colore32(q.colore);
        match q.anello {
            Some((interno, esterno)) => {
                let cx = (q.x + q.w / 2.0) / ppp;
                let cy = (q.y + q.h / 2.0) / ppp;
                p.circle_stroke(
                    egui::pos2(cx, cy),
                    (interno + esterno) / 2.0 / ppp,
                    Stroke::new((esterno - interno) / ppp, c),
                );
            }
            None => {
                let r = egui::Rect::from_min_size(
                    egui::pos2(q.x / ppp, q.y / ppp),
                    egui::vec2(q.w / ppp, q.h / ppp),
                );
                p.rect_filled(r, CornerRadius::ZERO, c);
            }
        }
    }
}

fn valore(ui: &mut Ui, v: &mut f32, (min, max): (f32, f32), attivo: bool) -> bool {
    ui.add_enabled(attivo, egui::DragValue::new(v).range(min..=max).speed(0.1).max_decimals(1).suffix(" px"))
        .changed()
}

/// L'editor del mirino. Vero se è cambiato.
pub fn mirino(ui: &mut Ui, c: &mut Config) -> bool {
    let prima = c.mirino;
    anteprima(ui, &c.mirino, c.tema);
    let m = &mut c.mirino;
    egui::Grid::new("mirino").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        ui.label("Forma");
        ui.horizontal(|ui| {
            for f in Forma::TUTTE {
                ui.selectable_value(&mut m.forma, f, f.nome());
            }
        });
        ui.end_row();
        ui.label(if m.forma == Forma::Cerchio { "Raggio" } else { "Lunghezza" });
        valore(ui, &mut m.lunghezza, LIMITI_LUNGHEZZA, m.forma != Forma::Punto);
        ui.end_row();
        ui.label("Spessore");
        valore(ui, &mut m.spessore, LIMITI_SPESSORE, true);
        ui.end_row();
        ui.label("Spazio dal centro");
        valore(ui, &mut m.spazio, LIMITI_SPAZIO, m.forma == Forma::Croce);
        ui.end_row();
        ui.label("Bordo");
        valore(ui, &mut m.bordo, LIMITI_BORDO, true);
        ui.end_row();
        ui.label("Colore");
        ui.horizontal(|ui| {
            egui::color_picker::color_edit_button_srgb(ui, &mut m.colore);
            if ui.button("Come prima").clicked() {
                *m = Mirino::default();
            }
        });
        ui.end_row();
    });
    ui.label(
        RichText::new("Nel giro il mirino è uguale all'anteprima, pixel per pixel.").color(TENUE).size(13.0),
    );
    c.mirino != prima
}

/// I colori dei bersagli e il tema della stanza. Vero se sono cambiati.
pub fn aspetto(ui: &mut Ui, c: &mut Config) -> bool {
    let (colori, tema) = (c.bersagli, c.tema);
    egui::Grid::new("aspetto").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        ui.label("Bersaglio");
        egui::color_picker::color_edit_button_srgb(ui, &mut c.bersagli.normale);
        ui.end_row();
        ui.label("Col mirino sopra");
        egui::color_picker::color_edit_button_srgb(ui, &mut c.bersagli.guardato);
        ui.end_row();
        ui.label("Colpito");
        egui::color_picker::color_edit_button_srgb(ui, &mut c.bersagli.colpito);
        ui.end_row();
        ui.label("");
        if ui.button("Colori di base").clicked() {
            c.bersagli = ColoriBersaglio::default();
        }
        ui.end_row();
        ui.label("Stanza");
        ui.horizontal(|ui| {
            for t in Tema::TUTTI {
                ui.selectable_value(&mut c.tema, t, t.nome());
            }
        });
        ui.end_row();
    });
    // La stanza del tema con le tre sfere: come si vedranno nel giro.
    let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), Sense::hover());
    let p = ui.painter_at(area);
    let stanza = c.tema.colori();
    p.rect_filled(area, CornerRadius::same(8), colore3(stanza.pareti));
    let f = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    for (i, (col, nome)) in [
        (f(c.bersagli.normale), "bersaglio"),
        (f(c.bersagli.guardato), "mirino sopra"),
        (f(c.bersagli.colpito), "colpito"),
    ]
    .into_iter()
    .enumerate()
    {
        let x = area.left() + area.width() * (i as f32 * 2.0 + 1.0) / 6.0;
        p.circle_filled(egui::pos2(x, area.top() + 36.0), 20.0, col);
        p.text(
            egui::pos2(x, area.bottom() - 14.0),
            egui::Align2::CENTER_CENTER,
            nome,
            egui::FontId::proportional(12.0),
            colore3(stanza.linee)
                .lerp_to_gamma(Color32::from_gray(if stanza.pareti[0] > 0.4 { 20 } else { 230 }), 0.7),
        );
    }
    c.bersagli != colori || c.tema != tema
}

/// Interruttore e volume. `Some(true)`: "Senti"; `Some(false)`: cambiati; `None`: niente.
pub fn suoni(ui: &mut Ui, c: &mut Config, guasto: bool) -> Option<bool> {
    let mut esito = None;
    egui::Grid::new("suoni").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        ui.label("Suoni");
        if ui.checkbox(&mut c.suoni, "colpo, errore e comparsa").changed() {
            esito = Some(false);
        }
        ui.end_row();
        ui.label("Volume");
        ui.horizontal(|ui| {
            let r = ui.add_enabled(
                c.suoni,
                egui::Slider::new(&mut c.volume, LIMITI_VOLUME.0..=LIMITI_VOLUME.1).step_by(1.0).suffix("%"),
            );
            if r.changed() {
                esito = Some(false);
            }
            // Si sente a mouse rilasciato, col volume nuovo.
            if ui.add_enabled(c.suoni && c.volume > 0.0, egui::Button::new("Senti")).clicked()
                || r.drag_stopped()
            {
                esito = Some(true);
            }
        });
        ui.end_row();
    });
    ui.label(
        RichText::new(
            "Suoni sintetizzati da aimpro. Escono da un thread a parte: il frame del giro non li aspetta.",
        )
        .color(TENUE)
        .size(13.0),
    );
    if guasto && c.suoni {
        ui.label(
            RichText::new("L'uscita audio non risponde: per ora i suoni sono muti.")
                .color(Color32::from_rgb(255, 210, 122))
                .size(13.0),
        );
    }
    esito
}
