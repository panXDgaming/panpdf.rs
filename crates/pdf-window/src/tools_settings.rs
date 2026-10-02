use std::collections::BTreeMap;

use eframe::egui;

use pdf_app::tools::{Manner, PAD_ASPECT, PICTURE_PRESETS, manner, sign_strokes, terms_in};
use pdf_app::wording::{Lang, Message, Tools};
use pdf_convert::{Choice, Setting, SettingKind, Value, Values};

use crate::tools_page::Wanted;

const LABEL_WIDTH: f32 = 132.0;
const FIELD_WIDTH: f32 = 280.0;
const STACK_BELOW: f32 = 520.0;

pub(crate) struct Draw<'a> {
    pub(crate) values: &'a mut Values,
    pub(crate) drafts: &'a mut BTreeMap<Setting, String>,
    pub(crate) pad: &'a mut Vec<Vec<(f32, f32)>>,
    pub(crate) repeat: &'a mut String,
    pub(crate) revealed: &'a mut bool,
    pub(crate) picture_name: Option<String>,
    pub(crate) lang: Lang,
    pub(crate) this_page: Option<usize>,
    pub(crate) wanted: &'a mut Option<Wanted>,
}

impl Draw<'_> {
    fn say(&self, sentence: &Tools) -> String {
        sentence.say(self.lang)
    }

    fn set(&mut self, setting: Setting, value: Value) {
        self.values.set(setting, value);
    }
}

fn weak(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().weak_text_color()
}

fn small(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(weak(ui))).wrap());
}

fn help_under(ui: &mut egui::Ui, d: &Draw<'_>, setting: Setting) {
    if let Some(help) = Tools::Help(setting).said(d.lang) {
        small(ui, &help);
    }
}

fn labelled(
    ui: &mut egui::Ui,
    d: &mut Draw<'_>,
    setting: Setting,
    add: impl FnOnce(&mut egui::Ui, &mut Draw<'_>),
) {
    let label = d.say(&Tools::Label(setting));
    if ui.available_width() < STACK_BELOW {
        ui.add_space(2.0);
        ui.label(egui::RichText::new(label).size(13.0).color(weak(ui)));
        add(ui, d);
        help_under(ui, d, setting);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, 26.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(LABEL_WIDTH, 26.0));
                ui.label(egui::RichText::new(label).size(13.0).color(weak(ui)));
            },
        );
        ui.vertical(|ui| {
            add(ui, d);
            help_under(ui, d, setting);
        });
    });
}

pub(crate) fn show(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    ui.add_space(4.0);
    match manner(setting) {
        Manner::Cards => cards(ui, d, setting),
        Manner::Segments => segments(ui, d, setting),
        Manner::Menu => menu(ui, d, setting),
        Manner::Ticks => ticks(ui, d, setting),
        Manner::Switch => switch(ui, d, setting),
        Manner::Presets => presets(ui, d, setting),
        Manner::Slider => slider(ui, d, setting),
        Manner::Amount => amount(ui, d, setting),
        Manner::Words => words(ui, d, setting),
        Manner::Secret => secret(ui, d, setting),
        Manner::Pages | Manner::SignPages => pages(ui, d, setting),
        Manner::Picture => picture(ui, d, setting),
        Manner::Pad => pad(ui, d, setting),
        Manner::Typeface => typeface(ui, d, setting),
        Manner::Hidden => {}
    }
}

fn current(d: &Draw<'_>, setting: Setting) -> Option<Choice> {
    d.values.choice(setting)
}

fn cards(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    ui.label(
        egui::RichText::new(d.say(&Tools::Label(setting)))
            .size(13.0)
            .color(weak(ui)),
    );
    ui.add_space(2.0);
    let chosen = current(d, setting);
    let mut picked = None;
    for &choice in setting.choices() {
        let title = d.say(&Tools::Choice(choice));
        let about = Tools::Explains(choice).said(d.lang);
        if card(ui, &title, about.as_deref(), chosen == Some(choice)) {
            picked = Some(choice);
        }
        ui.add_space(4.0);
    }
    if let Some(choice) = picked {
        d.set(setting, Value::Choice(choice));
    }
}

fn card(ui: &mut egui::Ui, title: &str, about: Option<&str>, on: bool) -> bool {
    let height = if about.is_some() { 50.0 } else { 34.0 };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let accent = visuals.selection.stroke.color;
        let (fill, stroke) = if on {
            (accent.gamma_multiply(0.12), egui::Stroke::new(1.5, accent))
        } else if response.hovered() {
            (
                visuals.widgets.hovered.weak_bg_fill,
                egui::Stroke::new(1.0, accent),
            )
        } else {
            (visuals.panel_fill, visuals.widgets.noninteractive.bg_stroke)
        };
        let text = visuals.text_color();
        let faint = visuals.weak_text_color();
        let painter = ui.painter();
        painter.rect(rect, 8.0, fill, stroke, egui::StrokeKind::Inside);
        let centre = egui::pos2(rect.left() + 28.0, rect.center().y);
        painter.circle_stroke(
            centre,
            7.0,
            egui::Stroke::new(1.3, if on { accent } else { faint }),
        );
        if on {
            painter.circle_filled(centre, 3.6, accent);
        }
        let x = rect.left() + 46.0;
        match about {
            Some(about) => {
                painter.text(
                    egui::pos2(x, rect.top() + 16.0),
                    egui::Align2::LEFT_CENTER,
                    title,
                    egui::FontId::proportional(14.0),
                    text,
                );
                painter.text(
                    egui::pos2(x, rect.top() + 34.0),
                    egui::Align2::LEFT_CENTER,
                    about,
                    egui::FontId::proportional(12.0),
                    faint,
                );
            }
            None => {
                painter.text(
                    egui::pos2(x, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    title,
                    egui::FontId::proportional(14.0),
                    text,
                );
            }
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

fn segments(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    labelled(ui, d, setting, |ui, d| {
        let chosen = current(d, setting);
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            for &choice in setting.choices() {
                let name = d.say(&Tools::Choice(choice));
                let button = egui::Button::selectable(chosen == Some(choice), name)
                    .min_size(egui::vec2(0.0, 26.0));
                if ui.add(button).clicked() {
                    picked = Some(choice);
                }
            }
        });
        if let Some(choice) = picked {
            d.values.set(setting, Value::Choice(choice));
        }
    });
}

fn menu(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    labelled(ui, d, setting, |ui, d| {
        let chosen = current(d, setting);
        let shown = chosen.map_or_else(String::new, |choice| d.say(&Tools::Choice(choice)));
        let mut picked = None;
        egui::ComboBox::from_id_salt(("tool-setting", setting))
            .selected_text(shown)
            .width(190.0)
            .show_ui(ui, |ui| {
                for &choice in setting.choices() {
                    let name = d.say(&Tools::Choice(choice));
                    if ui.selectable_label(chosen == Some(choice), name).clicked() {
                        picked = Some(choice);
                    }
                }
            });
        if let Some(choice) = picked {
            d.values.set(setting, Value::Choice(choice));
        }
    });
}

fn ticks(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    labelled(ui, d, setting, |ui, d| {
        let mut chosen = d.values.choices(setting);
        let before = chosen.clone();
        ui.horizontal_wrapped(|ui| {
            for &choice in setting.choices() {
                let mut on = chosen.contains(&choice);
                if ui
                    .checkbox(&mut on, d.say(&Tools::Choice(choice)))
                    .changed()
                {
                    chosen.retain(|had| *had != choice);
                    if on {
                        chosen.push(choice);
                    }
                }
                ui.add_space(8.0);
            }
        });
        if chosen != before {
            if chosen.is_empty() {
                d.values.unset(setting);
            } else {
                d.values.set(setting, Value::Choices(chosen));
            }
        }
    });
}

fn switch(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    let mut on = d.values.flag(setting);
    if ui
        .checkbox(&mut on, d.say(&Tools::Label(setting)))
        .changed()
    {
        d.set(setting, Value::Flag(on));
    }
    if let Some(help) = Tools::Help(setting).said(d.lang) {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            small(ui, &help);
        });
    }
}

fn presets(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    labelled(ui, d, setting, |ui, d| {
        let now = d.values.number(setting);
        let mut picked = None;
        ui.horizontal(|ui| {
            for (dpi, name) in PICTURE_PRESETS.iter().zip([
                Tools::PresetScreen,
                Tools::PresetNormal,
                Tools::PresetPrint,
            ]) {
                let on = now.is_some_and(|number| (number - dpi).abs() < 0.5);
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a resolution of 72, 150 or 300"
                )]
                let hover = Tools::Dpi(*dpi as u32).say(d.lang);
                let button =
                    egui::Button::selectable(on, name.say(d.lang)).min_size(egui::vec2(0.0, 26.0));
                if ui.add(button).on_hover_text(hover).clicked() {
                    picked = Some(*dpi);
                }
            }
        });
        if let Some(dpi) = picked {
            d.values.set(setting, Value::Number(dpi));
        }
    });
}

fn bounds(setting: Setting) -> (f64, f64, f64) {
    match setting.kind() {
        SettingKind::Number {
            least,
            most,
            default,
            ..
        } => (least, most, default),
        _ => (0.0, 1.0, 0.0),
    }
}

fn slider(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    labelled(ui, d, setting, |ui, d| {
        let (least, most, default) = bounds(setting);
        let mut number = d.values.number(setting).unwrap_or(default);
        if ui
            .add(egui::Slider::new(&mut number, least..=most).step_by(1.0))
            .changed()
        {
            d.values.set(setting, Value::Number(number));
        }
    });
}

fn amount(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    labelled(ui, d, setting, |ui, d| {
        let (least, most, default) = bounds(setting);
        let mut number = d.values.number(setting).unwrap_or(default);
        let unit = Tools::Unit(setting).said(d.lang).unwrap_or_default();
        let drag = egui::DragValue::new(&mut number)
            .range(least..=most)
            .speed(1.0)
            .max_decimals(0)
            .suffix(unit);
        if ui.add(drag).changed() {
            d.values.set(setting, Value::Number(number));
        }
    });
}

fn draft_of(d: &mut Draw<'_>, setting: Setting) -> String {
    d.drafts.get(&setting).cloned().unwrap_or_default()
}

fn words(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    labelled(ui, d, setting, |ui, d| {
        let mut text = draft_of(d, setting);
        let hint = Tools::Hint(setting).said(d.lang).unwrap_or_default();
        let field = if setting == Setting::Search {
            egui::TextEdit::multiline(&mut text)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
        } else {
            egui::TextEdit::singleline(&mut text).desired_width(FIELD_WIDTH)
        };
        if ui.add(field.hint_text(hint)).changed() {
            keep_words(d, setting, text);
        }
    });
}

fn keep_words(d: &mut Draw<'_>, setting: Setting, text: String) {
    match setting {
        Setting::Search => {
            let terms = terms_in(&text);
            if terms.is_empty() {
                d.values.unset(setting);
            } else {
                d.values.set(setting, Value::Terms(terms));
            }
        }
        _ => {
            if text.trim().is_empty() {
                d.values.unset(setting);
            } else {
                d.values.set(setting, Value::Text(text.trim().to_owned()));
            }
        }
    }
    d.drafts.insert(setting, text);
}

fn secret(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    let repeated = setting == Setting::NewPassword;
    labelled(ui, d, setting, |ui, d| {
        let mut text = draft_of(d, setting);
        let hint = Tools::Hint(setting).said(d.lang).unwrap_or_default();
        ui.horizontal(|ui| {
            let field = egui::TextEdit::singleline(&mut text)
                .password(!*d.revealed)
                .desired_width(FIELD_WIDTH)
                .hint_text(hint);
            if ui.add(field).changed() {
                if text.is_empty() {
                    d.values.unset(setting);
                } else {
                    d.values.set(setting, Value::Secret(text.clone()));
                }
                d.drafts.insert(setting, text.clone());
            }
            if !repeated {
                reveal_box(ui, d);
            }
        });
    });
    if repeated {
        ui.add_space(4.0);
        let label = d.say(&Tools::RepeatPassword);
        let hint = d.say(&Tools::RepeatHint);
        let mut again = std::mem::take(d.repeat);
        let wide = ui.available_width() >= STACK_BELOW;
        let mut field = |ui: &mut egui::Ui, d: &mut Draw<'_>| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut again)
                        .password(!*d.revealed)
                        .desired_width(FIELD_WIDTH)
                        .hint_text(hint.clone()),
                );
                reveal_box(ui, d);
            });
        };
        if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(LABEL_WIDTH, 26.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(egui::vec2(LABEL_WIDTH, 26.0));
                        ui.label(egui::RichText::new(&label).size(13.0).color(weak(ui)));
                    },
                );
                field(ui, d);
            });
        } else {
            ui.label(egui::RichText::new(&label).size(13.0).color(weak(ui)));
            field(ui, d);
        }
        *d.repeat = again;
    }
}

fn reveal_box(ui: &mut egui::Ui, d: &mut Draw<'_>) {
    ui.checkbox(d.revealed, Message::ShowThePassword.say(d.lang));
}

fn pages(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    let sign = manner(setting) == Manner::SignPages;
    labelled(ui, d, setting, |ui, d| {
        let mut text = draft_of(d, setting);
        let fallback = match setting.kind() {
            SettingKind::Pages { default } => default,
            _ => "",
        };
        let effective = if text.trim().is_empty() {
            fallback.to_owned()
        } else {
            text.trim().to_owned()
        };
        let mut picks: Vec<(Tools, String)> = if sign {
            vec![
                (Tools::PagesLast, "last".to_owned()),
                (Tools::PagesFirst, "1".to_owned()),
                (Tools::PagesEvery, "all".to_owned()),
            ]
        } else {
            vec![(Tools::PagesAll, String::new())]
        };
        if !sign && let Some(page) = d.this_page {
            picks.push((Tools::PagesThis, (page + 1).to_string()));
        }
        let mut changed = None;
        ui.horizontal_wrapped(|ui| {
            for (name, value) in &picks {
                let on = effective.eq_ignore_ascii_case(value);
                let button =
                    egui::Button::selectable(on, name.say(d.lang)).min_size(egui::vec2(0.0, 26.0));
                if ui.add(button).clicked() {
                    changed = Some(value.clone());
                }
            }
            let hint = Tools::Hint(setting).said(d.lang).unwrap_or_default();
            let typed_here = !text.trim().is_empty()
                && !picks
                    .iter()
                    .any(|(_, value)| text.trim().eq_ignore_ascii_case(value));
            let mut shown = if typed_here {
                text.clone()
            } else {
                String::new()
            };
            let field = egui::TextEdit::singleline(&mut shown)
                .desired_width(120.0)
                .hint_text(hint);
            if ui.add(field).changed() {
                changed = Some(shown);
            }
        });
        if let Some(now) = changed {
            text = now;
            if text.trim().is_empty() {
                d.values.unset(setting);
            } else {
                d.values.set(setting, Value::Pages(text.trim().to_owned()));
            }
            d.drafts.insert(setting, text);
        }
    });
}

fn picture(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    labelled(ui, d, setting, |ui, d| {
        ui.horizontal(|ui| {
            let name = d.picture_name.clone();
            let words = if name.is_some() {
                Tools::ChangeFile
            } else {
                Tools::ChoosePicture
            };
            if ui.button(d.say(&words)).clicked() {
                *d.wanted = Some(Wanted::SignaturePicture);
            }
            if let Some(name) = name {
                ui.label(egui::RichText::new(name).size(12.0).color(weak(ui)));
            }
        });
    });
}

fn typeface(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    labelled(ui, d, setting, |ui, d| {
        let chosen = d.values.text(setting).map(std::borrow::Cow::into_owned);
        let standard = d.say(&Tools::TypefaceStandard);
        let shown = chosen.clone().unwrap_or_else(|| standard.clone());
        let mut picked: Option<Option<String>> = None;
        egui::ComboBox::from_id_salt(("tool-setting", setting))
            .selected_text(shown)
            .width(220.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(chosen.is_none(), standard.clone())
                    .clicked()
                {
                    picked = Some(None);
                }
                for family in pdf_cli::font_families() {
                    if ui
                        .selectable_label(chosen.as_deref() == Some(family.as_str()), family)
                        .clicked()
                    {
                        picked = Some(Some(family.clone()));
                    }
                }
            });
        match picked {
            Some(Some(family)) => d.values.set(setting, Value::Text(family)),
            Some(None) => {
                d.values.unset(setting);
            }
            None => {}
        }
    });
}

fn pad(ui: &mut egui::Ui, d: &mut Draw<'_>, setting: Setting) {
    ui.label(
        egui::RichText::new(d.say(&Tools::Label(setting)))
            .size(13.0)
            .color(weak(ui)),
    );
    ui.add_space(2.0);
    let width = ui.available_width().min(460.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, width * PAD_ASPECT),
        egui::Sense::click_and_drag(),
    );
    if response.drag_started() {
        d.pad.push(Vec::new());
    }
    if response.dragged()
        && let (Some(at), Some(stroke)) = (response.interact_pointer_pos(), d.pad.last_mut())
    {
        let point = (at.x - rect.left(), at.y - rect.top());
        if stroke.last() != Some(&point) {
            stroke.push(point);
        }
    }
    if response.drag_stopped() {
        keep_drawing(d, setting);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    if ui.is_rect_visible(rect) {
        paint_pad(ui, d, rect);
    }
    ui.horizontal(|ui| {
        let clear = egui::Button::new(d.say(&Tools::ClearDrawing));
        if ui.add_enabled(!d.pad.is_empty(), clear).clicked() {
            d.pad.clear();
            d.values.unset(setting);
        }
    });
}

fn keep_drawing(d: &mut Draw<'_>, setting: Setting) {
    d.pad.retain(|stroke| !stroke.is_empty());
    let strokes = sign_strokes(d.pad);
    if strokes.is_empty() {
        d.values.unset(setting);
    } else {
        d.values.set(setting, Value::Strokes(strokes));
    }
}

fn paint_pad(ui: &egui::Ui, d: &Draw<'_>, rect: egui::Rect) {
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect(
        rect,
        8.0,
        egui::Color32::from_rgb(0xfb, 0xfb, 0xf8),
        visuals.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let base = rect.top() + rect.height() * 0.74;
    painter.line_segment(
        [
            egui::pos2(rect.left() + 18.0, base),
            egui::pos2(rect.right() - 18.0, base),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_rgb(0xd5, 0xd9, 0xe0)),
    );
    if d.pad.is_empty() {
        painter.text(
            egui::pos2(rect.center().x, base - 12.0),
            egui::Align2::CENTER_CENTER,
            d.say(&Tools::SignHere),
            egui::FontId::proportional(12.0),
            egui::Color32::from_rgb(0x9a, 0xa1, 0xad),
        );
    }
    let ink = egui::Stroke::new(2.2, egui::Color32::from_rgb(0x0d, 0x1f, 0x73));
    for stroke in d.pad.iter() {
        let points: Vec<egui::Pos2> = stroke
            .iter()
            .map(|(x, y)| egui::pos2(rect.left() + x, rect.top() + y))
            .collect();
        match points.as_slice() {
            [] => {}
            [one] => {
                painter.circle_filled(*one, 1.2, ink.color);
            }
            _ => {
                painter.add(egui::Shape::line(points, ink));
            }
        }
    }
}
