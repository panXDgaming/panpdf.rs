use eframe::egui;
use pdf_agent::tools::request::{PlanStep, StepState};
use pdf_app::ai_permission::{Answer as Allowed, Shown, WrittenTo};
use pdf_app::wording::{Assistant, Lang, Message};

use super::look::{
    CAPTION, ROUND, caption, disclosure, field, link, primary_button, secondary_button, small,
};
use super::{GAP, Notice, NoticeAction};
use crate::ai_actions::{Question, QuestionReply};

pub(super) fn card_frame(ui: &egui::Ui, edge: Option<egui::Color32>) -> egui::Frame {
    let visuals = ui.visuals();
    egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0,
            edge.unwrap_or(visuals.widgets.noninteractive.bg_stroke.color),
        ))
        .corner_radius(ROUND + 2)
        .inner_margin(egui::Margin::same(10))
}

const SNIPPET: usize = 220;

fn clipped(text: &str, whole: bool) -> (String, bool) {
    if whole || text.chars().count() <= SNIPPET {
        return (text.to_owned(), text.chars().count() > SNIPPET);
    }
    let mut cut: String = text.chars().take(SNIPPET).collect();
    cut.push('\u{2026}');
    (cut, true)
}

fn side_by_side(ui: &mut egui::Ui, label: &str, text: &str, tint: egui::Color32, lang: Lang) {
    egui::Frame::new()
        .fill(tint)
        .corner_radius(ROUND)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(caption(label));
            if text.trim().is_empty() {
                ui.label(caption(Assistant::Removed.say(lang)).italics());
            } else {
                ui.add(egui::Label::new(egui::RichText::new(text).size(12.0)).selectable(true));
            }
        });
}

pub(super) fn the_card(
    ui: &mut egui::Ui,
    (shown, may_remember, of_this_tool): (&Shown, bool, usize),
    call_id: &str,
    lang: Lang,
) -> Option<Allowed> {
    let say = |message: Message| message.say(lang);
    let mut answered = None;
    let warn = ui.visuals().warn_fg_color;
    let removed = ui.visuals().error_fg_color.gamma_multiply(0.09);
    let added = ui.visuals().selection.bg_fill.gamma_multiply(0.3);
    let open_id = egui::Id::new(("ai-card-all", call_id));
    let mut whole = ui.data_mut(|data| *data.get_temp_mut_or_default::<bool>(open_id));
    ui.add_space(GAP);
    card_frame(ui, Some(warn.gamma_multiply(0.8))).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        let diff = shown.before.is_some() || shown.after.is_some();
        if !diff {
            ui.label(caption(say(Message::AiWantsTo)));
        }
        ui.add(
            egui::Label::new(egui::RichText::new(&shown.headline).strong().size(12.5))
                .selectable(true),
        );
        if let Some(place) = &shown.written_to {
            let (label, path) = match place {
                WrittenTo::Folder(path) => (Assistant::WillBeWrittenIn, path),
                WrittenTo::File(path) => (Assistant::WillBeWrittenAs, path),
            };
            ui.label(caption(label.say(lang)));
            ui.add(
                egui::Label::new(egui::RichText::new(path).size(12.0))
                    .selectable(true)
                    .wrap(),
            );
        }
        let mut clipped_any = false;
        if let Some(before) = &shown.before {
            let (text, cut) = clipped(before, whole);
            clipped_any |= cut;
            side_by_side(ui, &Assistant::Before.say(lang), &text, removed, lang);
        }
        if let Some(after) = &shown.after {
            let (text, cut) = clipped(after, whole);
            clipped_any |= cut;
            side_by_side(ui, &Assistant::After.say(lang), &text, added, lang);
        }
        if clipped_any {
            let words = if whole {
                Assistant::ShowLess
            } else {
                Assistant::ShowAll
            };
            if link(ui, &words.say(lang)).clicked() {
                whole = !whole;
            }
        }
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            if primary_button(ui, &say(Message::AiAllowOnce), true).clicked() {
                answered = Some(Allowed::Once);
            }
            if secondary_button(ui, &say(Message::AiRefuse)).clicked() {
                answered = Some(Allowed::Refuse);
            }
        });
        if of_this_tool > 1 || may_remember {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = GAP * 1.5;
                if of_this_tool > 1
                    && link(
                        ui,
                        &Message::AiAllowAll {
                            count: of_this_tool,
                        }
                        .say(lang),
                    )
                    .clicked()
                {
                    answered = Some(Allowed::AllOfThem);
                }
                if may_remember && link(ui, &say(Message::AiAllowForThisChat)).clicked() {
                    answered = Some(Allowed::ForThisChat);
                }
            });
        }
    });
    ui.data_mut(|data| data.insert_temp(open_id, whole));
    answered
}

pub(super) fn the_question(
    ui: &mut egui::Ui,
    question: &mut Question,
    lang: Lang,
) -> Option<QuestionReply> {
    let say = |message: Message| message.say(lang);
    let mut reply = None;
    ui.add_space(GAP);
    card_frame(
        ui,
        Some(ui.visuals().selection.stroke.color.gamma_multiply(0.7)),
    )
    .show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(caption(say(Message::AiQuestionForYou)));
        ui.add(
            egui::Label::new(egui::RichText::new(&question.asked).strong().size(12.5))
                .selectable(true),
        );
        for (label, means) in &question.options {
            let mut job = egui::text::LayoutJob::default();
            let style = ui.style();
            job.append(
                label,
                0.0,
                egui::TextFormat {
                    font_id: egui::TextStyle::Body.resolve(style),
                    color: style.visuals.text_color(),
                    ..egui::TextFormat::default()
                },
            );
            if !means.is_empty() {
                job.append(
                    &format!("\n{means}"),
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::proportional(CAPTION),
                        color: style.visuals.weak_text_color(),
                        ..egui::TextFormat::default()
                    },
                );
            }
            let width = ui.available_width();
            job.wrap.max_width = width - 2.0 * ui.spacing().button_padding.x;
            if ui
                .add(
                    egui::Button::new(job)
                        .min_size(egui::vec2(width, 0.0))
                        .corner_radius(ROUND),
                )
                .clicked()
            {
                reply = Some(QuestionReply::Said(label.clone()));
            }
        }
        ui.horizontal(|ui| {
            let skip = secondary_button(ui, &say(Message::AiSkipQuestion));
            let answer = ui.add_enabled(
                !question.own.trim().is_empty(),
                egui::Button::new(say(Message::AiAnswer)).min_size(egui::vec2(0.0, 28.0)),
            );
            let own_id = egui::Id::new("ai-own-answer");
            let typed = field(ui, own_id, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut question.own)
                        .id(own_id)
                        .frame(egui::Frame::NONE)
                        .hint_text(say(Message::AiOwnAnswer))
                        .desired_width(ui.available_width()),
                )
            });
            let entered =
                typed.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if (answer.clicked() || entered) && !question.own.trim().is_empty() {
                reply = Some(QuestionReply::Said(question.own.trim().to_owned()));
            }
            if skip.clicked() {
                reply = Some(QuestionReply::Skipped);
            }
        });
    });
    reply
}

pub(super) fn the_notice(ui: &mut egui::Ui, notice: &Notice, lang: Lang) -> Noticed {
    let mut noticed = Noticed::default();
    let tone = if notice.action == Some(NoticeAction::Continue) {
        ui.visuals().warn_fg_color
    } else {
        ui.visuals().error_fg_color
    };
    egui::Frame::new()
        .fill(tone.gamma_multiply(0.07))
        .stroke(egui::Stroke::new(1.0, tone.gamma_multiply(0.5)))
        .corner_radius(ROUND + 2)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            let full = ui.available_width();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.scope(|ui| {
                    ui.set_width(full - 26.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(notice.said.say(lang)).size(12.5))
                            .wrap(),
                    );
                });
                if crate::format::quiet_icon_button(
                    ui,
                    crate::icons::Icon::Close,
                    &Message::AiDismiss.say(lang),
                )
                .clicked()
                {
                    noticed.dismissed = true;
                }
            });
            if let Some(detail) = &notice.detail {
                ui.add(egui::Label::new(caption(detail)).selectable(true).wrap());
            }
            if let Some(action) = notice.action {
                let words = match action {
                    NoticeAction::Continue => Message::AiContinue.say(lang),
                    NoticeAction::Retry => Message::AiRetry.say(lang),
                    NoticeAction::OpenSettings => Assistant::OpenSettings.say(lang),
                };
                ui.add_space(2.0);
                if primary_button(ui, &words, true).clicked() {
                    noticed.chosen = Some(action);
                }
            }
        });
    noticed
}

#[derive(Default)]
pub(super) struct Noticed {
    pub(super) dismissed: bool,
    pub(super) chosen: Option<NoticeAction>,
}

pub(super) fn went_back(ui: &mut egui::Ui, document_stays: bool, lang: Lang) -> bool {
    let mut put_back = false;
    ui.add_space(GAP);
    card_frame(ui, None).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(caption(Message::AiWentBack.say(lang)));
        if document_stays {
            ui.add(
                egui::Label::new(
                    small(Message::AiWentBackDocumentStays.say(lang))
                        .color(ui.visuals().warn_fg_color),
                )
                .wrap(),
            );
        }
        if link(ui, &Message::AiPutBack.say(lang)).clicked() {
            put_back = true;
        }
    });
    put_back
}

pub(super) fn the_run_strip(ui: &mut egui::Ui, steps: usize, lang: Lang) -> bool {
    let mut undo = false;
    ui.horizontal(|ui| {
        ui.label(caption(Message::AiRunMadeChanges { steps }.say(lang)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if link(ui, &Message::AiUndoRun.say(lang)).clicked() {
                undo = true;
            }
        });
    });
    undo
}

fn plan_mark(ui: &mut egui::Ui, state: StepState) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
    let painter = ui.painter();
    let centre = rect.center();
    let ink = ui.visuals().selection.stroke.color;
    let weak = ui.visuals().weak_text_color();
    match state {
        StepState::Pending => {
            painter.circle_stroke(centre, 4.5, egui::Stroke::new(1.2, weak));
        }
        StepState::InProgress => {
            painter.circle_stroke(centre, 5.5, egui::Stroke::new(1.2, ink));
            painter.circle_filled(centre, 3.0, ink);
        }
        StepState::Done => {
            painter.circle_filled(centre, 5.5, weak);
            let tick = egui::Stroke::new(1.5, ui.visuals().panel_fill);
            painter.line_segment(
                [
                    centre + egui::vec2(-2.5, 0.0),
                    centre + egui::vec2(-0.7, 2.2),
                ],
                tick,
            );
            painter.line_segment(
                [
                    centre + egui::vec2(-0.7, 2.2),
                    centre + egui::vec2(2.8, -2.4),
                ],
                tick,
            );
        }
    }
}

const PLAN_MOST_HEIGHT: f32 = 120.0;

pub(super) fn the_plan(ui: &mut egui::Ui, steps: &[PlanStep], lang: Lang) {
    let done = steps
        .iter()
        .filter(|step| step.state == StepState::Done)
        .count();
    let open_id = egui::Id::new("ai-plan-open");
    let mut open = ui
        .data(|data| data.get_temp::<bool>(open_id))
        .unwrap_or(true);
    let current = steps
        .iter()
        .find(|step| step.state == StepState::InProgress)
        .or_else(|| steps.iter().find(|step| step.state == StepState::Pending));
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(ROUND + 2)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let header = ui.horizontal(|ui| {
                disclosure(ui, open);
                ui.label(small(
                    Message::AiPlan {
                        done,
                        total: steps.len(),
                    }
                    .say(lang),
                ));
                if open {
                    return;
                }
                if let Some(step) = current {
                    ui.add(egui::Label::new(caption(&step.text)).truncate());
                }
            });
            let toggle = ui.interact(
                header.response.rect,
                egui::Id::new("ai-plan-header"),
                egui::Sense::click(),
            );
            if toggle.clicked() {
                open = !open;
            }
            if toggle.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if open {
                ui.add_space(2.0);
                egui::ScrollArea::vertical()
                    .id_salt("ai-plan")
                    .max_height(PLAN_MOST_HEIGHT)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for step in steps {
                            ui.horizontal_top(|ui| {
                                plan_mark(ui, step.state);
                                let text = egui::RichText::new(&step.text).size(12.0);
                                let text = match step.state {
                                    StepState::Done => text.weak(),
                                    StepState::InProgress => text.strong(),
                                    StepState::Pending => text,
                                };
                                ui.add(egui::Label::new(text).wrap());
                            });
                        }
                    });
            }
        });
    ui.data_mut(|data| data.insert_temp(open_id, open));
}
