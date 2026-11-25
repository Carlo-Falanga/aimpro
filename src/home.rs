//! La home: la barra laterale (Gioca, Libreria, Playlist, Statistiche, Impostazioni) e le sue pagine, più la pagina
//! del primo avvio. Tutto con egui, solo fuori dal giro.
//!
//! Ogni pagina è una funzione che disegna il suo contenuto nell'area a destra della barra: Statistiche e Impostazioni
//! chiamano le funzioni di `interfaccia.rs`, Playlist una sua funzione qui, così chi le rifà cambia solo quella.

use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Ui};

use crate::config::Config;
use crate::interfaccia::{self, ACCENTO, AVVISO, Azione, InfoMenu, TENUE, pulsante, valore};
use crate::scenari::{Categoria, Difficolta, Scenario};
use crate::storico::{GiroSalvato, Storico};

/// Le pagine della home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pagina {
    Gioca,
    Libreria,
    /// La pagina di uno scenario, col suo id.
    Scenario(String),
    Playlist,
    Statistiche,
    Impostazioni,
}

impl Pagina {
    /// Le voci della barra laterale.
    pub const BARRA: [(Pagina, &'static str); 5] = [
        (Pagina::Gioca, "Gioca"),
        (Pagina::Libreria, "Libreria"),
        (Pagina::Playlist, "Playlist"),
        (Pagina::Statistiche, "Statistiche"),
        (Pagina::Impostazioni, "Impostazioni"),
    ];

    /// La voce della barra accesa per questa pagina (lo scenario sta nella libreria).
    pub fn voce(&self) -> Pagina {
        match self {
            Pagina::Scenario(_) => Pagina::Libreria,
            p => p.clone(),
        }
    }
}

/// Ricerca e filtri della libreria.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filtri {
    pub ricerca: String,
    pub categoria: Option<Categoria>,
    pub difficolta: Option<Difficolta>,
    pub solo_preferiti: bool,
}

/// Gli scenari che passano ricerca e filtri, nell'ordine della libreria. La ricerca guarda nome e descrizione, senza
/// badare a maiuscole e accenti.
pub fn filtra<'a>(scenari: &'a [Scenario], f: &Filtri, c: &Config) -> Vec<&'a Scenario> {
    let cerca = semplice(f.ricerca.trim());
    scenari
        .iter()
        .filter(|s| f.categoria.is_none_or(|x| s.categoria == x))
        .filter(|s| f.difficolta.is_none_or(|x| s.difficolta == x))
        .filter(|s| !f.solo_preferiti || c.preferito(&s.id))
        .filter(|s| {
            cerca.is_empty()
                || semplice(&s.nome).contains(&cerca)
                || semplice(&s.descrizione).contains(&cerca)
        })
        .collect()
}

/// Minuscole e senza accenti, per la ricerca.
fn semplice(t: &str) -> String {
    t.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'á' => 'a',
            'è' | 'é' => 'e',
            'ì' | 'í' => 'i',
            'ò' | 'ó' => 'o',
            'ù' | 'ú' => 'u',
            c => c,
        })
        .collect()
}

/// Lo stato della home che resta fra un frame e l'altro.
#[derive(Clone, Debug)]
pub struct Home {
    pub pagina: Pagina,
    pub filtri: Filtri,
}

impl Default for Home {
    fn default() -> Self {
        Home { pagina: Pagina::Gioca, filtri: Filtri::default() }
    }
}

/// Quello che le pagine mostrano oltre alle impostazioni.
pub struct InfoHome<'a> {
    pub menu: InfoMenu<'a>,
    /// `None` finché il thread dello storico non ha finito (la pagina Gioca lo dice e aspetta).
    pub storico: Option<&'a Storico>,
    pub esportato: Option<&'a str>,
}

const SFONDO_BARRA: Color32 = Color32::from_rgb(17, 18, 20);
const SFONDO_PAGINA: Color32 = Color32::from_rgb(22, 23, 26);
const RIQUADRO: Color32 = Color32::from_rgba_premultiplied(6, 6, 6, 6);

/// Un riquadro chiaro su fondo scuro, come i gruppi di una pagina di impostazioni.
fn riquadro<R>(ui: &mut Ui, contenuto: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(RIQUADRO)
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(18)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contenuto(ui)
        })
        .inner
}

fn sezione(ui: &mut Ui, titolo: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(titolo).size(17.0).strong());
    ui.add_space(2.0);
}

/// La home intera: la barra a sinistra e la pagina a destra.
pub fn home(
    ui: &mut Ui,
    h: &mut Home,
    c: &mut Config,
    scelta_statistiche: &mut String,
    info: &InfoHome,
) -> Azione {
    let mut azione = Azione::Nessuna;
    egui::Panel::left("barra")
        .exact_size(220.0)
        .resizable(false)
        .frame(Frame::new().fill(SFONDO_BARRA).inner_margin(Margin::symmetric(16, 24)))
        .show(ui, |ui| {
            ui.label(RichText::new("aimpro").size(26.0).strong());
            ui.add_space(18.0);
            let accesa = h.pagina.voce();
            for (p, nome) in Pagina::BARRA {
                let scelta = accesa == p;
                let testo = RichText::new(nome).size(16.0);
                let testo = if scelta { testo.strong().color(ACCENTO) } else { testo };
                if ui
                    .add_sized([ui.available_width(), 36.0], egui::Button::selectable(scelta, testo))
                    .clicked()
                {
                    if p == Pagina::Statistiche {
                        *scelta_statistiche = c.scenario.clone();
                    }
                    h.pagina = p;
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                if pulsante(ui, "Esci da aimpro", false) {
                    azione = Azione::Chiudi;
                }
            });
        });
    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(SFONDO_PAGINA).inner_margin(Margin::symmetric(36, 28)))
        .show(ui, |ui| {
            let pagina = h.pagina.clone();
            let a = if pagina == Pagina::Statistiche {
                // Le statistiche hanno il loro elenco che scorre: niente seconda barra di scorrimento.
                match info.storico {
                    Some(s) => {
                        interfaccia::statistiche(ui, s, info.menu.scenari, scelta_statistiche, info.esportato)
                    }
                    None => attesa_storico(ui),
                }
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(900.0));
                        match &pagina {
                            Pagina::Gioca => gioca(ui, h, c, info),
                            Pagina::Libreria => libreria(ui, h, c, info),
                            Pagina::Scenario(id) => scenario(ui, h, c, id, scelta_statistiche, info),
                            Pagina::Playlist => playlist(ui),
                            Pagina::Impostazioni => interfaccia::impostazioni(ui, c, &info.menu),
                            Pagina::Statistiche => Azione::Nessuna,
                        }
                    })
                    .inner
            };
            if a != Azione::Nessuna {
                azione = a;
            }
        });
    azione
}

fn attesa_storico(ui: &mut Ui) -> Azione {
    ui.label(RichText::new("Carico lo storico dei giri…").color(TENUE));
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    Azione::Nessuna
}

fn trova<'a>(scenari: &'a [Scenario], id: &str) -> Option<&'a Scenario> {
    scenari.iter().find(|s| s.id == id)
}

fn record_di(info: &InfoHome, s: &Scenario) -> String {
    info.storico.and_then(|st| st.record(&s.id)).map_or("-".to_string(), |r| valore(r, s.regole.tracking()))
}

/// Un elenco di scenari in una griglia: il nome (apre la sua pagina), la categoria (se `con_categoria`), la
/// difficoltà, il record, preferito e Gioca.
#[allow(clippy::too_many_arguments)]
fn elenco(
    ui: &mut Ui,
    id: &str,
    scenari: &[&Scenario],
    con_categoria: bool,
    h: &mut Home,
    c: &mut Config,
    info: &InfoHome,
    azione: &mut Azione,
) {
    let tenue = |t: &str| RichText::new(t).color(TENUE).size(13.0);
    egui::Grid::new(id).num_columns(6).spacing([18.0, 6.0]).min_col_width(130.0).min_row_height(26.0).show(
        ui,
        |ui| {
            ui.label(tenue("Scenario"));
            if con_categoria {
                ui.label(tenue("Categoria"));
            }
            ui.label(tenue("Difficoltà"));
            ui.label(tenue("Record"));
            ui.label("");
            ui.label("");
            ui.end_row();
            for s in scenari {
                let nome = egui::Button::new(RichText::new(&s.nome).size(15.0).strong()).frame(false);
                if ui.add(nome).on_hover_text(&s.descrizione).clicked() {
                    h.pagina = Pagina::Scenario(s.id.clone());
                }
                if con_categoria {
                    ui.label(tenue(s.categoria.nome()));
                }
                ui.label(tenue(s.difficolta.nome()));
                ui.label(RichText::new(record_di(info, s)).size(13.0));
                ui.label(
                    RichText::new(if c.preferito(&s.id) { "preferito" } else { "" }).color(TENUE).size(12.0),
                );
                if ui.add_enabled(interfaccia::si_gioca(&info.menu), egui::Button::new("Gioca")).clicked() {
                    c.scenario = s.id.clone();
                    *azione = Azione::Inizia;
                }
                ui.end_row();
            }
        },
    );
}

/// La pagina iniziale: lo scenario scelto, gli ultimi giocati, i preferiti e i record recenti.
fn gioca(ui: &mut Ui, h: &mut Home, c: &mut Config, info: &InfoHome) -> Azione {
    let mut azione = interfaccia::aggiornamento(ui, info.menu.build, info.menu.aggiornamento);
    interfaccia::titolo_pagina(ui, "Gioca", "");
    let scelto = trova(info.menu.scenari, &c.scenario).or(info.menu.scenari.first());
    if let Some(s) = scelto {
        let s = s.clone();
        riquadro(ui, |ui| {
            ui.label(RichText::new(&s.nome).size(22.0).strong());
            ui.label(
                RichText::new(format!(
                    "{} · {} · record {}",
                    s.categoria.nome(),
                    s.difficolta.nome(),
                    record_di(info, &s)
                ))
                .color(TENUE),
            );
            ui.label(interfaccia::spiegazione(&s));
            if !c.parametri().standard() {
                ui.label(
                    RichText::new("Parametri cambiati: i giri si salvano ma non fanno record.")
                        .color(AVVISO)
                        .size(13.0),
                );
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.set_max_width(460.0);
                let w = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                let gioca = egui::Button::new(
                    RichText::new("Gioca").size(16.0).strong().color(Color32::from_rgb(23, 19, 10)),
                )
                .fill(ACCENTO);
                if ui
                    .add_enabled(interfaccia::si_gioca(&info.menu), gioca.min_size(egui::vec2(w, 38.0)))
                    .clicked()
                {
                    c.scenario = s.id.clone();
                    azione = Azione::Inizia;
                }
                let apri = egui::Button::new(RichText::new("Apri lo scenario").size(16.0))
                    .min_size(egui::vec2(w, 38.0));
                if ui.add(apri).clicked() {
                    h.pagina = Pagina::Scenario(s.id.clone());
                }
            });
        });
    }
    for a in info.menu.avvisi_scenari.iter().take(3) {
        ui.label(RichText::new(a).color(AVVISO).size(12.0));
    }
    if info.menu.avvisi_scenari.len() > 3 {
        ui.label(
            RichText::new(format!("e altri {} file di scenario saltati", info.menu.avvisi_scenari.len() - 3))
                .color(AVVISO)
                .size(12.0),
        );
    }

    let Some(storico) = info.storico else {
        attesa_storico(ui);
        return azione;
    };

    sezione(ui, "Ultimi giocati");
    riquadro(ui, |ui| {
        let ultimi: Vec<&Scenario> =
            storico.ultimi_giocati(5).into_iter().filter_map(|id| trova(info.menu.scenari, id)).collect();
        if ultimi.is_empty() {
            ui.label(RichText::new("Nessun giro ancora: scegli uno scenario e gioca.").color(TENUE));
        } else {
            elenco(ui, "ultimi", &ultimi, true, h, c, info, &mut azione);
        }
    });

    sezione(ui, "Preferiti");
    riquadro(ui, |ui| {
        let preferiti: Vec<&Scenario> =
            c.preferiti.iter().filter_map(|id| trova(info.menu.scenari, id)).collect();
        if preferiti.is_empty() {
            ui.label(
                RichText::new(
                    "Nessun preferito: si aggiungono dalla pagina di uno scenario, nella libreria.",
                )
                .color(TENUE),
            );
        } else {
            elenco(ui, "preferiti", &preferiti, true, h, c, info, &mut azione);
        }
    });

    sezione(ui, "Record recenti");
    riquadro(ui, |ui| {
        let record = storico.record_recenti(5);
        if record.is_empty() {
            ui.label(RichText::new("Ancora nessun record.").color(TENUE));
        }
        egui::Grid::new("record recenti").num_columns(3).spacing([28.0, 8.0]).show(ui, |ui| {
            for g in record {
                let nome =
                    trova(info.menu.scenari, &g.scenario).map_or(g.scenario.as_str(), |s| s.nome.as_str());
                ui.label(RichText::new(&g.data).color(TENUE).size(13.0));
                ui.label(RichText::new(nome).strong());
                ui.label(valore(g.punteggio(), g.tracking.is_some()));
                ui.end_row();
            }
        });
    });
    azione
}

/// La libreria: ricerca, filtri e gli scenari divisi per categoria.
fn libreria(ui: &mut Ui, h: &mut Home, c: &mut Config, info: &InfoHome) -> Azione {
    let mut azione = Azione::Nessuna;
    interfaccia::titolo_pagina(
        ui,
        "Libreria",
        "Gli scenari di aimpro e quelli che metti in %APPDATA%\\aimpro\\scenari. Ognuno dura 30 secondi.",
    );
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut h.filtri.ricerca)
                .hint_text("Cerca per nome")
                .desired_width(220.0),
        );
        let tutte = "Tutte le categorie";
        egui::ComboBox::from_id_salt("filtro categoria")
            .selected_text(h.filtri.categoria.map_or(tutte, Categoria::nome))
            .width(230.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut h.filtri.categoria, None, tutte);
                for k in Categoria::TUTTE {
                    ui.selectable_value(&mut h.filtri.categoria, Some(k), k.nome());
                }
            });
        let ogni = "Ogni difficoltà";
        egui::ComboBox::from_id_salt("filtro difficolta")
            .selected_text(h.filtri.difficolta.map_or(ogni, Difficolta::nome))
            .width(130.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut h.filtri.difficolta, None, ogni);
                for d in Difficolta::TUTTE {
                    ui.selectable_value(&mut h.filtri.difficolta, Some(d), d.nome());
                }
            });
        ui.checkbox(&mut h.filtri.solo_preferiti, "solo preferiti");
    });
    let trovati = filtra(info.menu.scenari, &h.filtri, c);
    ui.label(
        RichText::new(match trovati.len() {
            0 => "Nessuno scenario con questi filtri.".to_string(),
            1 => "1 scenario".to_string(),
            n => format!("{n} scenari"),
        })
        .color(TENUE)
        .size(13.0),
    );
    for k in Categoria::TUTTE {
        let di_questa: Vec<&Scenario> = trovati.iter().copied().filter(|s| s.categoria == k).collect();
        if di_questa.is_empty() {
            continue;
        }
        sezione(ui, k.nome());
        riquadro(ui, |ui| {
            elenco(ui, k.nome(), &di_questa, false, h, c, info, &mut azione);
        });
    }
    azione
}

/// La pagina di uno scenario: descrizione, record, ultimi punteggi, preferito, parametri e Gioca.
fn scenario(
    ui: &mut Ui,
    h: &mut Home,
    c: &mut Config,
    id: &str,
    scelta_statistiche: &mut String,
    info: &InfoHome,
) -> Azione {
    let mut azione = Azione::Nessuna;
    if ui.add(egui::Button::new(RichText::new("Torna alla libreria").color(TENUE)).frame(false)).clicked() {
        h.pagina = Pagina::Libreria;
        return azione;
    }
    let Some(s) = trova(info.menu.scenari, id) else {
        ui.label("Questo scenario non c'è più.");
        return azione;
    };
    let tracking = s.regole.tracking();
    interfaccia::titolo_pagina(
        ui,
        &s.nome,
        &format!(
            "{} · difficoltà {} · {:.0} secondi",
            s.categoria.nome(),
            s.difficolta.nome(),
            s.regole.durata_ms / 1000.0
        ),
    );
    ui.label(interfaccia::spiegazione(s));
    if let Some(f) = &s.file {
        ui.label(RichText::new(format!("Dal file {}", f.display())).color(TENUE).size(12.0));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let gioca = egui::Button::new(
            RichText::new("Gioca").size(16.0).strong().color(Color32::from_rgb(23, 19, 10)),
        )
        .fill(ACCENTO)
        .min_size(egui::vec2(180.0, 38.0));
        if ui.add_enabled(interfaccia::si_gioca(&info.menu), gioca).clicked() {
            c.scenario = s.id.clone();
            azione = Azione::Inizia;
        }
        let preferito = c.preferito(&s.id);
        let testo = if preferito { "Togli dai preferiti" } else { "Aggiungi ai preferiti" };
        if ui
            .add(egui::Button::new(RichText::new(testo).size(16.0)).min_size(egui::vec2(200.0, 38.0)))
            .clicked()
        {
            c.cambia_preferito(&s.id);
            azione = Azione::Impostazioni;
        }
        if ui
            .add(egui::Button::new(RichText::new("Statistiche").size(16.0)).min_size(egui::vec2(140.0, 38.0)))
            .clicked()
        {
            *scelta_statistiche = s.id.clone();
            h.pagina = Pagina::Statistiche;
        }
    });

    sezione(ui, "Parametri");
    riquadro(ui, |ui| {
        if interfaccia::parametri_scenario(ui, c, s) {
            azione = Azione::Impostazioni;
        }
    });

    sezione(ui, "Record e ultimi punteggi");
    riquadro(ui, |ui| {
        let Some(storico) = info.storico else {
            attesa_storico(ui);
            return;
        };
        let record = storico.record(&s.id);
        ui.label(
            RichText::new(format!(
                "Record: {}",
                record.map_or("nessuno ancora (contano i giri coi parametri standard)".to_string(), |r| {
                    valore(r, tracking)
                })
            ))
            .strong(),
        );
        let ultimi: Vec<&GiroSalvato> = storico.giri_di(&s.id).rev().take(8).collect();
        if ultimi.is_empty() {
            ui.label(RichText::new("Nessun giro di questo scenario.").color(TENUE));
            return;
        }
        egui::Grid::new("ultimi punteggi").num_columns(3).spacing([28.0, 8.0]).show(ui, |ui| {
            for g in ultimi {
                ui.label(RichText::new(&g.data).color(TENUE).size(13.0));
                ui.label(RichText::new(valore(g.punteggio(), tracking)).strong());
                ui.label(
                    RichText::new(if g.standard() {
                        "parametri standard".to_string()
                    } else {
                        format!(
                            "dimensione {:.0}%, velocità {:.0}%: fuori dal record",
                            g.parametri.dimensione * 100.0,
                            g.parametri.velocita * 100.0
                        )
                    })
                    .color(TENUE)
                    .size(13.0),
                );
                ui.end_row();
            }
        });
    });
    azione
}

/// La pagina Playlist: le sequenze di scenari arrivano con la loro task.
pub fn playlist(ui: &mut Ui) -> Azione {
    interfaccia::titolo_pagina(ui, "Playlist", "");
    riquadro(ui, |ui| {
        ui.label("Le playlist, cioè più scenari da giocare uno dopo l'altro, non ci sono ancora.");
        ui.label(
            RichText::new("Intanto i preferiti tengono a portata di mano gli scenari che giochi di più.")
                .color(TENUE),
        );
    });
    Azione::Nessuna
}

/// Il primo avvio: una pagina sola con gioco, sensibilità e DPI, poi la home.
pub fn benvenuto(ui: &mut Ui, c: &mut Config) -> Azione {
    interfaccia::scheda(ui, "benvenuto", |ui| {
        let mut azione = Azione::Nessuna;
        ui.label(RichText::new("Benvenuto in aimpro").size(26.0).strong());
        ui.label(
            RichText::new("Scegli il gioco da cui prendi la sensibilità: la visuale girerà come lì.")
                .color(TENUE),
        );
        ui.add_space(6.0);
        egui::Grid::new("benvenuto").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            if interfaccia::righe_sensibilita(ui, c, false) {
                azione = Azione::Impostazioni;
            }
        });
        ui.add_space(2.0);
        interfaccia::riga_cm(ui, c);
        ui.label(RichText::new("Tutto si cambia anche dopo, in Impostazioni.").color(TENUE).size(13.0));
        ui.add_space(8.0);
        if pulsante(ui, "Continua", true) {
            c.primo_avvio_fatto = true;
            azione = Azione::Impostazioni;
        }
        azione
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::scenari::di_aimpro;

    #[test]
    fn la_ricerca_trova_per_nome_senza_badare_a_maiuscole_e_accenti() {
        let s = di_aimpro();
        let c = Config::default();
        let f = |r: &str| Filtri { ricerca: r.into(), ..Filtri::default() };
        let nomi = |v: Vec<&Scenario>| v.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        assert_eq!(nomi(filtra(&s, &f("GUIZ"), &c)), ["guizzo"]);
        assert_eq!(filtra(&s, &f(""), &c).len(), 26);
        assert!(filtra(&s, &f("nessuno così"), &c).is_empty());
        // "velocita" trova anche "velocità" nella descrizione.
        assert!(semplice("Velocità").contains("velocita"));
    }

    #[test]
    fn i_filtri_per_categoria_difficolta_e_preferiti() {
        let s = di_aimpro();
        let mut c = Config::default();
        let tracking = Filtri { categoria: Some(Categoria::Tracking), ..Filtri::default() };
        let t = filtra(&s, &tracking, &c);
        assert!(t.len() >= 4 && t.iter().all(|x| x.regole.tracking()));
        let difficili = Filtri { difficolta: Some(Difficolta::Difficile), ..tracking.clone() };
        let d = filtra(&s, &difficili, &c);
        assert!(
            !d.is_empty() && d.iter().all(|x| x.difficolta == Difficolta::Difficile && x.regole.tracking())
        );
        let pref = Filtri { solo_preferiti: true, ..Filtri::default() };
        assert!(filtra(&s, &pref, &c).is_empty());
        c.cambia_preferito("terna");
        c.cambia_preferito("filo");
        let p: Vec<&str> = filtra(&s, &pref, &c).iter().map(|x| x.id.as_str()).collect();
        assert_eq!(p, ["filo", "terna"], "nell'ordine della libreria");
        c.cambia_preferito("terna");
        assert_eq!(c.preferiti, ["filo"]);
    }

    #[test]
    fn la_barra_ha_le_cinque_voci_e_lo_scenario_accende_la_libreria() {
        let nomi: Vec<&str> = Pagina::BARRA.iter().map(|(_, n)| *n).collect();
        assert_eq!(nomi, ["Gioca", "Libreria", "Playlist", "Statistiche", "Impostazioni"]);
        assert_eq!(Pagina::Scenario("filo".into()).voce(), Pagina::Libreria);
        assert_eq!(Pagina::Statistiche.voce(), Pagina::Statistiche);
    }
}
