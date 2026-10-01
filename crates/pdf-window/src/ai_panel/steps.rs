use eframe::egui;
use pdf_agent::connect::{ToolCall, ToolResult};
use pdf_agent::json::Json;
use pdf_app::wording::{Assistant, Lang};

use super::look::{CAPTION, ROUND, caption, disclosure, small};
use super::{GAP, tool_icon};

pub(super) struct Step<'a> {
    pub(super) call: Option<&'a ToolCall>,
    pub(super) result: &'a ToolResult,
}

const MOST_ARGUMENT: usize = 400;
const MOST_RESULT: usize = 4_000;

fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(most).collect();
    kept.push('\u{2026}');
    kept
}

fn value_said(value: &Json) -> String {
    match value {
        Json::Text(text) => text.clone(),
        Json::Null => "none".to_owned(),
        Json::Bool(on) => on.to_string(),
        Json::Number(number) => {
            if number.fract() == 0.0 && number.abs() < 1e15 {
                format!("{number:.0}")
            } else {
                number.to_string()
            }
        }
        other => other.write(),
    }
}

pub(super) fn arguments_said(arguments: &Json) -> String {
    match arguments {
        Json::Object(pairs) => pairs
            .iter()
            .map(|(name, value)| format!("{name}: {}", cut(&value_said(value), MOST_ARGUMENT)))
            .collect::<Vec<_>>()
            .join("\n"),
        Json::Null => String::new(),
        other => cut(&value_said(other), MOST_ARGUMENT),
    }
}

fn failed_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
    let error = ui.visuals().error_fg_color;
    ui.painter().circle_filled(rect.center(), 5.5, error);
    let cross = egui::Stroke::new(1.3, ui.visuals().window_fill);
    let centre = rect.center();
    ui.painter().line_segment(
        [
            centre + egui::vec2(-2.2, -2.2),
            centre + egui::vec2(2.2, 2.2),
        ],
        cross,
    );
    ui.painter().line_segment(
        [
            centre + egui::vec2(2.2, -2.2),
            centre + egui::vec2(-2.2, 2.2),
        ],
        cross,
    );
}

pub(super) fn what_the_tools_did(ui: &mut egui::Ui, steps: &[Step<'_>], salt: &str, lang: Lang) {
    if steps.is_empty() {
        return;
    }
    let failed = steps.iter().filter(|step| step.result.is_error).count();
    let group_id = egui::Id::new(("ai-steps", salt));
    let mut open = ui
        .data(|data| data.get_temp::<bool>(group_id))
        .unwrap_or(false);
    ui.add_space(GAP * 0.5);
    let header = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        disclosure(ui, open);
        ui.label(small(Assistant::Steps(steps.len()).say(lang)).weak());
        if failed > 0 {
            ui.label(
                small(format!(
                    "\u{b7} {}",
                    Assistant::StepsFailed(failed).say(lang)
                ))
                .color(ui.visuals().error_fg_color),
            );
        }
    });
    let toggle = ui.interact(
        header.response.rect.expand2(egui::vec2(0.0, 2.0)),
        group_id.with("header"),
        egui::Sense::click(),
    );
    if toggle.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if toggle.clicked() {
        open = !open;
    }
    ui.data_mut(|data| data.insert_temp(group_id, open));
    if !open {
        return;
    }
    egui::Frame::new()
        .stroke(egui::Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(ROUND)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            for (at, step) in steps.iter().enumerate() {
                one_step(ui, step, group_id.with(at), lang);
            }
        });
}

fn one_step(ui: &mut egui::Ui, step: &Step<'_>, id: egui::Id, lang: Lang) {
    let name = step.call.map_or("", |call| call.name.as_str());
    let request = step
        .call
        .and_then(|call| pdf_agent::tools::request::parse(&call.name, &call.arguments).ok());
    let target = request
        .as_ref()
        .and_then(pdf_app::ai_status::target)
        .map(|target| target.say(lang));
    let mut open = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
    let failed = step.result.is_error;
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
        let ink = if failed {
            ui.visuals().error_fg_color
        } else {
            ui.visuals().text_color()
        };
        tool_icon(name).draw(ui.painter(), rect.shrink2(egui::vec2(0.0, 1.0)), ink);
        ui.label(small(pdf_app::ai_status::did(name, lang)).color(ink));
        if let Some(target) = &target {
            ui.add(egui::Label::new(caption(target)).truncate());
        }
        if failed {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                failed_mark(ui);
            });
        }
    });
    let toggle = ui
        .interact(row.response.rect, id.with("row"), egui::Sense::click())
        .on_hover_text(if failed {
            Assistant::StepFailed.say(lang)
        } else {
            String::new()
        });
    if toggle.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if toggle.clicked() {
        open = !open;
    }
    ui.data_mut(|data| data.insert_temp(id, open));
    if !open {
        return;
    }
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(ROUND)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;
            if let Some(call) = step.call {
                let asked = arguments_said(&call.arguments);
                if !asked.is_empty() {
                    ui.label(caption(Assistant::StepAsked.say(lang)));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(asked)
                                .monospace()
                                .size(CAPTION)
                                .color(ui.visuals().text_color()),
                        )
                        .selectable(true)
                        .wrap(),
                    );
                }
            }
            ui.label(caption(Assistant::StepResult.say(lang)));
            let ink = if failed {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            };
            egui::ScrollArea::vertical()
                .id_salt(id.with("result"))
                .max_height(180.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(cut(&step.result.text, MOST_RESULT))
                                .size(CAPTION)
                                .color(ink),
                        )
                        .selectable(true)
                        .wrap(),
                    );
                });
        });
}
