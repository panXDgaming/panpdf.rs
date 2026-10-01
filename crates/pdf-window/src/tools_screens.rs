use std::path::{Path, PathBuf};

use eframe::egui;

use pdf_app::tools::trouble_in;
use pdf_app::wording::{Lang, Tools};
use pdf_convert::run::{Failure, Outcome};
use pdf_convert::{Group, Setting, Tool};

use crate::tools_marks;
use crate::tools_page::{ACCENT, ACCENT_HOVER, Act};
use crate::tools_run::{Saved, Working};

const SHOWN_RESULTS: usize = 6;

pub(crate) struct Done {
    pub(crate) tool: Tool,
    pub(crate) outcome: Outcome,
    pub(crate) saved: Result<Saved, String>,
    pub(crate) tenths: u32,
    pub(crate) from_bytes: Option<u64>,
    pub(crate) original: PathBuf,
}

pub(crate) struct Failed {
    pub(crate) failure: Failure,
    pub(crate) typed: String,
    pub(crate) typed_newer: String,
    pub(crate) tried: bool,
}

pub(crate) fn weak(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().weak_text_color()
}

pub(crate) fn caption(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(weak(ui))).wrap());
}

pub(crate) fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(20.0);
    ui.label(
        egui::RichText::new(text)
            .size(12.0)
            .strong()
            .color(weak(ui)),
    );
    ui.add_space(6.0);
}

pub(crate) fn hairline(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}

pub(crate) fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool, width: f32) -> bool {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 40.0), sense);
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let (fill, ink) = if !enabled {
            (
                visuals.widgets.inactive.weak_bg_fill,
                visuals.weak_text_color(),
            )
        } else if response.is_pointer_button_down_on() || response.hovered() {
            (ACCENT_HOVER, egui::Color32::WHITE)
        } else {
            (ACCENT, egui::Color32::WHITE)
        };
        ui.painter().rect_filled(rect, 8.0, fill);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(15.0),
            ink,
        );
        if enabled && response.has_focus() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                9.0,
                egui::Stroke::new(1.5, ACCENT),
                egui::StrokeKind::Outside,
            );
        }
    }
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && response.clicked()
}

pub(crate) fn note_box(ui: &mut egui::Ui, text: &str) {
    let visuals = ui.visuals();
    egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(visuals.widgets.noninteractive.bg_stroke)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(weak(ui))).wrap());
        });
}

pub(crate) fn header(ui: &mut egui::Ui, tool: Tool, lang: Lang, back: bool) -> Option<Act> {
    let mut act = None;
    let button = egui::Button::new(format!("\u{2190}  {}", Tools::BackToTools.say(lang)));
    if ui.add_enabled(back, button).clicked() {
        act = Some(Act::Back);
    }
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        let size = tools_marks::mark_size(44.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        tools_marks::mark(ui.painter(), rect.min, 44.0, tool);
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.add_space(3.0);
            ui.label(
                egui::RichText::new(Tools::Name(tool).say(lang))
                    .size(22.0)
                    .strong(),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(Tools::Blurb(tool).say(lang))
                        .size(13.0)
                        .color(weak(ui)),
                )
                .wrap(),
            );
        });
    });
    ui.add_space(16.0);
    hairline(ui);
    act
}

pub(crate) fn progress_bar(ui: &mut egui::Ui, fraction: Option<f32>) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
    let visuals = ui.visuals();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 5.0, visuals.widgets.inactive.weak_bg_fill);
    if let Some(share) = fraction {
        let mut filled = rect;
        filled.set_right(rect.left() + rect.width() * share.clamp(0.02, 1.0));
        painter.rect_filled(filled, 5.0, ACCENT);
        return;
    }
    let time = ui.input(|input| input.time);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a position along a bar, between nothing and the whole of it"
    )]
    let along = ((time * 0.7).fract() * 1.4 - 0.2) as f32;
    let span = rect.width() * 0.3;
    let left = rect.left() + rect.width() * along;
    let pill = egui::Rect::from_min_max(
        egui::pos2(left, rect.top()),
        egui::pos2(left + span, rect.bottom()),
    );
    painter.rect_filled(pill, 5.0, ACCENT);
    ui.ctx().request_repaint();
}

pub(crate) fn running(ui: &mut egui::Ui, working: &Working, tool: Tool, lang: Lang) -> Option<Act> {
    let mut act = None;
    let title = if matches!(tool.group(), Group::FromPdf | Group::ToPdf) {
        Tools::Converting
    } else {
        Tools::Working
    };
    ui.add_space(24.0);
    ui.label(egui::RichText::new(title.say(lang)).size(20.0).strong());
    ui.add_space(2.0);
    caption(ui, &Tools::StaysHere.say(lang));
    ui.add_space(18.0);
    progress_bar(ui, working.how_far());
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(working.words(lang))
            .size(12.5)
            .monospace()
            .color(weak(ui)),
    );
    ui.add_space(18.0);
    let stop = egui::Button::new(Tools::Stop.say(lang)).min_size(egui::vec2(96.0, 30.0));
    if ui.add_enabled(!working.stopping, stop).clicked() {
        act = Some(Act::Stop);
    }
    act
}

pub(crate) fn done(ui: &mut egui::Ui, finished: &Done, lang: Lang) -> Option<Act> {
    let mut act = None;
    let many = finished.outcome.files.len() > 1;
    ui.add_space(22.0);
    let title = if many {
        Tools::FilesReady
    } else {
        Tools::Ready(finished.tool.output())
    };
    ui.label(egui::RichText::new(title.say(lang)).size(20.0).strong());
    ui.add_space(2.0);
    caption(
        ui,
        &Tools::DoneIn {
            files: finished.outcome.files.len(),
            tenths: finished.tenths,
        }
        .say(lang),
    );
    ui.add_space(14.0);
    results(ui, finished, lang);
    ui.add_space(10.0);
    match &finished.saved {
        Ok(saved) => {
            let folder = saved.folder.display().to_string();
            ui.add(
                egui::Label::new(
                    egui::RichText::new(Tools::SavedIn(folder).say(lang))
                        .size(12.0)
                        .color(weak(ui)),
                )
                .truncate(),
            );
        }
        Err(why) => {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(Tools::NotSaved(why.clone()).say(lang))
                        .size(12.0)
                        .color(ui.visuals().warn_fg_color),
                )
                .wrap(),
            );
        }
    }
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        act = result_buttons(ui, finished, lang);
    });
    notes(ui, &finished.outcome.notes, lang);
    act
}

fn opens_in_panpdf(saved: &Saved) -> Option<&Path> {
    saved
        .files
        .iter()
        .find(|path| {
            path.extension()
                .is_some_and(|ending| ending.eq_ignore_ascii_case("pdf"))
        })
        .map(PathBuf::as_path)
}

fn result_buttons(ui: &mut egui::Ui, finished: &Done, lang: Lang) -> Option<Act> {
    let mut act = None;
    let saved = finished.saved.as_ref().ok();
    if let Some(saved) = saved {
        if let Some(pdf) = opens_in_panpdf(saved) {
            if primary_button(ui, &Tools::OpenInPanPdf.say(lang), true, 168.0) {
                act = Some(Act::OpenInPanPdf(pdf.to_path_buf()));
            }
        } else if let [only] = saved.files.as_slice()
            && primary_button(ui, &Tools::OpenIt.say(lang), true, 120.0)
        {
            act = Some(Act::OpenOutside(only.clone()));
        }
        let first = saved.files.first().cloned().unwrap_or_default();
        let show = egui::Button::new(Tools::ShowInFolder.say(lang)).min_size(egui::vec2(0.0, 32.0));
        if ui.add(show).clicked() {
            act = Some(Act::ShowInFolder(first));
        }
    }
    if finished.outcome.files.len() == 1 {
        let save = egui::Button::new(Tools::SaveAs.say(lang)).min_size(egui::vec2(0.0, 32.0));
        if ui.add(save).clicked() {
            act = Some(Act::SaveCopy);
        }
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let again = egui::Button::new(Tools::StartOver.say(lang)).min_size(egui::vec2(0.0, 32.0));
        if ui.add(again).clicked() {
            act = Some(Act::StartOver);
        }
    });
    act
}

fn results(ui: &mut egui::Ui, finished: &Done, lang: Lang) {
    let visuals = ui.visuals();
    let stroke = visuals.widgets.noninteractive.bg_stroke;
    let fill = visuals.panel_fill;
    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let files = &finished.outcome.files;
            let shown = files.len().min(SHOWN_RESULTS);
            for (at, (name, bytes)) in files.iter().take(shown).enumerate() {
                if at > 0 {
                    ui.add_space(4.0);
                }
                result_row(ui, name, bytes.len() as u64, finished, lang);
            }
            if files.len() > shown {
                ui.add_space(4.0);
                caption(ui, &Tools::AndMore(files.len() - shown).say(lang));
            }
        });
}

fn result_row(ui: &mut egui::Ui, name: &str, bytes: u64, finished: &Done, lang: Lang) {
    ui.horizontal(|ui| {
        let size = tools_marks::mark_size(32.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        tools_marks::mark_for_file(ui.painter(), rect.min, 32.0, name);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.add_space(2.0);
            let shown = display_name(&finished.saved, name);
            ui.add(egui::Label::new(egui::RichText::new(shown).size(14.0)).truncate());
            let sizes = match finished.from_bytes {
                Some(from) if finished.outcome.files.len() == 1 => {
                    Tools::Shrank { from, to: bytes }
                }
                _ => Tools::Size(bytes),
            };
            ui.label(
                egui::RichText::new(sizes.say(lang))
                    .size(12.0)
                    .monospace()
                    .color(weak(ui)),
            );
        });
    });
}

fn display_name(saved: &Result<Saved, String>, made: &str) -> String {
    match saved {
        Ok(saved) if saved.files.len() == 1 => saved.files[0].file_name().map_or_else(
            || made.to_owned(),
            |name| name.to_string_lossy().into_owned(),
        ),
        _ => made.to_owned(),
    }
}

fn notes(ui: &mut egui::Ui, notes: &[String], lang: Lang) {
    if notes.is_empty() {
        return;
    }
    ui.add_space(14.0);
    egui::CollapsingHeader::new(Tools::WhatItDid.say(lang))
        .default_open(notes.len() <= 2)
        .show(ui, |ui| {
            for note in notes {
                ui.add(
                    egui::Label::new(egui::RichText::new(note).size(12.0).color(weak(ui))).wrap(),
                );
                ui.add_space(2.0);
            }
        });
}

pub(crate) fn failed(
    ui: &mut egui::Ui,
    failed: &mut Failed,
    (tool, file_password): (Tool, Option<Setting>),
    lang: Lang,
) -> Option<Act> {
    let mut act = None;
    let asks = failed.failure == Failure::NeedsPassword;
    ui.add_space(22.0);
    let (title, lede) = match &failed.failure {
        Failure::Cancelled => (Tools::Stopped, Tools::NothingMade),
        Failure::NeedsPassword => (Tools::NeedsPasswordTitle, Tools::NeedsPassword),
        Failure::BadInput(why) | Failure::Refused(why) => {
            (Tools::ThatDidNotWork, Tools::Trouble(trouble_in(why)))
        }
        Failure::Panicked(why) => (
            Tools::ThatDidNotWork,
            Tools::Trouble(trouble_in(&format!("panicked {why}"))),
        ),
    };
    ui.label(egui::RichText::new(title.say(lang)).size(20.0).strong());
    ui.add_space(4.0);
    ui.add(egui::Label::new(egui::RichText::new(lede.say(lang)).size(13.0)).wrap());
    if asks && file_password.is_some() {
        act = password_form(ui, failed, tool, lang);
    }
    if let Failure::BadInput(why) | Failure::Refused(why) | Failure::Panicked(why) = &failed.failure
    {
        ui.add_space(10.0);
        egui::CollapsingHeader::new(Tools::Details.say(lang)).show(ui, |ui| {
            let mut words = why.as_str();
            ui.add(
                egui::TextEdit::multiline(&mut words)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
        });
    }
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        let back =
            egui::Button::new(Tools::BackToSettings.say(lang)).min_size(egui::vec2(0.0, 32.0));
        if ui.add(back).clicked() {
            act = Some(Act::BackToSettings);
        }
        ui.add_space(6.0);
        let again = egui::Button::new(Tools::StartOver.say(lang)).min_size(egui::vec2(0.0, 32.0));
        if ui.add(again).clicked() {
            act = Some(Act::StartOver);
        }
    });
    act
}

fn password_form(ui: &mut egui::Ui, failed: &mut Failed, tool: Tool, lang: Lang) -> Option<Act> {
    let mut act = None;
    ui.add_space(12.0);
    let both = tool == Tool::Compare;
    let entry = |ui: &mut egui::Ui, label: Option<String>, text: &mut String| {
        ui.horizontal(|ui| {
            if let Some(label) = label {
                ui.add_sized(
                    [150.0, 26.0],
                    egui::Label::new(egui::RichText::new(label).size(13.0).color(weak(ui))),
                );
            }
            ui.add(
                egui::TextEdit::singleline(text)
                    .password(true)
                    .desired_width(260.0)
                    .hint_text(Tools::Label(Setting::Password).say(lang)),
            )
        })
        .inner
    };
    let older = both.then(|| Tools::PasswordOfOlder.say(lang));
    let first = entry(ui, older, &mut failed.typed);
    let mut second = None;
    if both {
        ui.add_space(4.0);
        second = Some(entry(
            ui,
            Some(Tools::PasswordOfNewer.say(lang)),
            &mut failed.typed_newer,
        ));
    }
    if failed.tried {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(Tools::PasswordDidNotOpen.say(lang))
                .size(12.0)
                .color(ui.visuals().warn_fg_color),
        );
    }
    ui.add_space(8.0);
    let typed = !failed.typed.is_empty() || !failed.typed_newer.is_empty();
    let submit = first.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
        || second.is_some_and(|response| {
            response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
        });
    let again = egui::Button::new(Tools::TryAgain.say(lang)).min_size(egui::vec2(110.0, 32.0));
    if (ui.add_enabled(typed, again).clicked() || submit) && typed {
        act = Some(Act::TryAgain);
    }
    act
}
