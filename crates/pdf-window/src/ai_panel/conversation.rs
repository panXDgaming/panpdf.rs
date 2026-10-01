use std::collections::BTreeMap;

use eframe::egui;
use pdf_agent::connect::{Said, ToolCall, Turn};
use pdf_app::ai_permission::{Answer as Allowed, describe_change};
use pdf_app::wording::{Assistant, Lang, Message};

use super::cards::{the_card, the_question, went_back};
use super::look::{Chip, ROUND, caption, chip, small, spinner};
use super::steps::{Step, what_the_tools_did};
use super::{AiState, GAP, going_back, shorten_model, tool_icon};
use crate::icons::Icon;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TurnAction {
    Edit,
    AskAgain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TurnShape {
    text: usize,
    attachments: usize,
    notes: usize,
}

impl TurnShape {
    fn of(turn: &Turn, at: usize, notes: &[(usize, Message)]) -> Self {
        Self {
            text: turn.text().len(),
            attachments: turn.attachments().len(),
            notes: notes.iter().filter(|(index, _)| *index == at).count(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Offers {
    edit: bool,
    ask_again: bool,
}

const WHEN_STARTING: f32 = 10.0;

fn a_note(ui: &mut egui::Ui, said: &str) {
    ui.add_space(GAP * 0.5);
    ui.add(egui::Label::new(caption(said)).selectable(true).wrap());
}

fn thinking_so_far(ui: &mut egui::Ui, thinking: &str, lang: Lang) {
    const TAIL: usize = 600;
    ui.add_space(GAP * 0.5);
    let counted = thinking.chars().count();
    let tail: String = thinking
        .chars()
        .skip(counted.saturating_sub(TAIL))
        .collect();
    egui::CollapsingHeader::new(caption(Message::AiThinkingAloud.say(lang)))
        .id_salt("ai-thinking")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Label::new(caption(tail)).selectable(true).wrap());
        });
}

fn said_by(
    ui: &mut egui::Ui,
    turn: &Turn,
    salt: &str,
    offers: Offers,
    lang: Lang,
) -> Option<TurnAction> {
    let mine = turn.said() == Said::Person;
    let mut action = None;
    ui.add_space(GAP);
    let frame = if mine {
        egui::Frame::new()
            .fill(
                ui.visuals()
                    .widgets
                    .inactive
                    .weak_bg_fill
                    .gamma_multiply(0.6),
            )
            .corner_radius(ROUND + 2)
            .inner_margin(egui::Margin::symmetric(10, 8))
    } else {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(2, 0))
    };
    let shown = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        if !turn.attachments().is_empty() {
            ui.horizontal_wrapped(|ui| {
                for attachment in turn.attachments() {
                    egui::Frame::new()
                        .stroke(egui::Stroke::new(
                            1.0,
                            ui.visuals().widgets.noninteractive.bg_stroke.color,
                        ))
                        .corner_radius(ROUND)
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.label(small(&attachment.name));
                        });
                }
            });
            ui.add_space(2.0);
        }
        crate::ai_written::written(ui, turn.text(), salt, lang);
    });
    let rect = shown.response.rect;
    let buttons = 1 + usize::from(offers.edit) + usize::from(offers.ask_again);
    #[expect(clippy::cast_precision_loss, reason = "at most three buttons")]
    let wide = buttons as f32 * 22.0 + (buttons - 1) as f32 * 2.0 + 8.0;
    let pill = egui::Rect::from_min_size(
        egui::pos2(rect.right() - wide - 4.0, rect.top() - 11.0),
        egui::vec2(wide, 24.0),
    );
    if !(ui.rect_contains_pointer(rect) || ui.rect_contains_pointer(pill)) {
        return None;
    }
    let visuals = ui.visuals();
    ui.painter().rect(
        pill,
        ROUND,
        visuals.window_fill,
        egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    let mut tools = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(pill.shrink2(egui::vec2(4.0, 1.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    tools.spacing_mut().item_spacing.x = 2.0;
    let quiet = crate::format::quiet_icon_button;
    if quiet(&mut tools, Icon::Copy, &Message::DraftCopy.say(lang)).clicked() {
        tools.ctx().copy_text(turn.text().to_owned());
    }
    if offers.edit && quiet(&mut tools, Icon::Edit, &Message::AiEditMeans.say(lang)).clicked() {
        action = Some(TurnAction::Edit);
    }
    if offers.ask_again
        && quiet(
            &mut tools,
            Icon::AskAgain,
            &Message::AiAskAgainMeans.say(lang),
        )
        .clicked()
    {
        action = Some(TurnAction::AskAgain);
    }
    action
}

fn the_welcome(ui: &mut egui::Ui, ready: bool, lang: Lang) -> Option<String> {
    let mut chosen = None;
    ui.add_space(GAP * 3.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
    Icon::Assistant.draw_tinted(ui.painter(), rect, ui.visuals().text_color(), true);
    ui.add_space(GAP);
    let said = if ready {
        Message::AiNothingAskedYet.say(lang)
    } else {
        Assistant::ConnectFirst.say(lang)
    };
    ui.add(egui::Label::new(egui::RichText::new(said).size(13.0)).wrap());
    if !ready {
        return None;
    }
    ui.add_space(GAP);
    let suggestions = [
        Assistant::SuggestSummarise,
        Assistant::SuggestSpelling,
        Assistant::SuggestContents,
        Assistant::SuggestTranslate,
    ];
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for suggestion in suggestions {
            let words = suggestion.say(lang);
            if chip(ui, &Chip::new(&words)).clicked() {
                chosen = Some(words);
            }
        }
    });
    chosen
}

impl AiState {
    #[expect(
        clippy::too_many_lines,
        reason = "one conversation, drawn top to bottom: turns, notes, steps, cards, status"
    )]
    pub(super) fn the_conversation(&mut self, ui: &mut egui::Ui, lang: Lang) -> Option<Allowed> {
        let card = self.tools.ask.as_ref().map(|pending| {
            let was = self.tools.text_before(&pending.request);
            (
                describe_change(&pending.request, was.as_deref(), lang),
                pending.may_allow_for_chat,
                pending.of_this_tool,
                pending.call.id.clone(),
            )
        });
        let mut answered = None;
        let idle = !self.working();
        let last_answer = self.last_question().and_then(|_| {
            self.turns.iter().rposition(
                |turn| matches!(turn, Turn::Model { text, .. } if !text.trim().is_empty()),
            )
        });
        let mut action = None;
        let mut put_back = false;
        let mut replied = None;
        let mut suggested = None;
        let ready = self.ready();
        let happening = self.what_is_happening(lang);
        let calls: BTreeMap<&str, &ToolCall> = self
            .turns
            .iter()
            .filter_map(|turn| match turn {
                Turn::Model { calls, .. } => Some(calls),
                _ => None,
            })
            .flatten()
            .map(|call| (call.id.as_str(), call))
            .collect();
        let document_stays = self
            .rewound
            .as_ref()
            .map(going_back::Rewound::changed_the_document);
        let width = ui.available_width();
        if (width - self.drawn_width).abs() > 0.5 {
            self.drawn.clear();
            self.drawn_width = width;
        }
        self.drawn.resize(self.turns.len(), None);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show_viewport(ui, |ui, viewport| {
                let origin = ui.cursor().top();
                let seen = viewport.expand2(egui::vec2(0.0, viewport.height()));
                ui.add_space(WHEN_STARTING);
                if self.turns.is_empty() && document_stays.is_none() {
                    suggested = the_welcome(ui, ready, lang);
                }
                let mut group: Vec<Step<'_>> = Vec::new();
                let mut group_at = 0;
                let mut after: Vec<&Message> = Vec::new();
                for (at, turn) in self.turns.iter().enumerate() {
                    let notes_here = self.notes.iter().filter(|(index, _)| *index == at);
                    let is_results = matches!(turn, Turn::Results { .. });
                    let visible = match turn {
                        Turn::Model { text, .. } => !text.trim().is_empty(),
                        Turn::Person { .. } => true,
                        Turn::Results { .. } => false,
                    };
                    if visible {
                        flush(ui, &mut group, group_at, &mut after, lang);
                    }
                    for (_, said) in notes_here {
                        if is_results && matches!(said, Message::AiChangedTheDocument { .. }) {
                            after.push(said);
                        } else {
                            flush(ui, &mut group, group_at, &mut after, lang);
                            a_note(ui, &said.say(lang));
                        }
                    }
                    if let Turn::Results { results } = turn {
                        if group.is_empty() {
                            group_at = at;
                        }
                        for result in results {
                            group.push(Step {
                                call: calls.get(result.call_id.as_str()).copied(),
                                result,
                            });
                        }
                        continue;
                    }
                    if !visible {
                        continue;
                    }
                    let shape = TurnShape::of(turn, at, &self.notes);
                    let top = ui.cursor().top();
                    if let Some((_, height)) = self.drawn[at].filter(|(was, _)| *was == shape)
                        && !seen
                            .y_range()
                            .intersects(egui::Rangef::new(top - origin, top - origin + height))
                    {
                        ui.add_space(height);
                        continue;
                    }
                    let offers = Offers {
                        edit: idle && turn.said() == Said::Person,
                        ask_again: idle && Some(at) == last_answer,
                    };
                    let salt = format!("turn-{at}");
                    if let Some(chosen) = said_by(ui, turn, &salt, offers, lang) {
                        action = Some((at, chosen));
                    }
                    self.drawn[at] = Some((shape, ui.cursor().top() - top));
                }
                flush(ui, &mut group, group_at, &mut after, lang);
                for (_, said) in self
                    .notes
                    .iter()
                    .filter(|(index, _)| *index >= self.turns.len())
                {
                    a_note(ui, &said.say(lang));
                }
                if let Some(document_stays) = document_stays
                    && idle
                {
                    put_back = went_back(ui, document_stays, lang);
                }
                if let Some((shown, may_remember, of_this_tool, call_id)) = &card {
                    answered = the_card(ui, (shown, *may_remember, *of_this_tool), call_id, lang);
                }
                if let Some(question) = self.tools.question.as_mut() {
                    replied = the_question(ui, question, lang);
                }
                if let Some(arrived) = self.partial.as_ref().filter(|far| !far.is_empty()) {
                    if !arrived.thinking.is_empty() {
                        thinking_so_far(ui, &arrived.thinking, lang);
                    }
                    if !arrived.said.is_empty() {
                        said_by(
                            ui,
                            &Turn::model(arrived.said.clone()),
                            "arriving",
                            Offers::default(),
                            lang,
                        );
                    }
                }
                if let Some((icon, doing)) = happening {
                    ui.add_space(GAP);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        spinner(ui);
                        if let Some(icon) = icon {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                            icon.draw(ui.painter(), rect, ui.visuals().weak_text_color());
                        }
                        ui.label(caption(doing));
                    });
                }
                ui.add_space(GAP);
            });
        if let Some(words) = suggested {
            self.composer = words;
            ui.memory_mut(|memory| memory.request_focus(super::composer_id()));
        }
        if put_back {
            self.put_back();
        }
        if let Some(reply) = replied {
            self.tools.answer_the_question(reply);
        }
        match action {
            Some((at, TurnAction::Edit)) => self.go_back_to(at, false),
            Some((_, TurnAction::AskAgain)) => {
                if let Some(at) = self.last_question() {
                    self.go_back_to(at, true);
                }
            }
            None => {}
        }
        answered
    }

    fn what_is_happening(&self, lang: Lang) -> Option<(Option<Icon>, String)> {
        use crate::ai_actions::Doing;
        match self.tools.doing() {
            Some(Doing::Tool(name, request)) => Some((
                Some(tool_icon(&name)),
                format!("{}\u{2026}", pdf_app::ai_status::doing(&request, lang)),
            )),
            None if self.busy() && !self.checking => {
                let arrived = self.partial.as_ref();
                let said = if arrived.is_some_and(|far| !far.said.trim().is_empty()) {
                    Message::AiWritingTheAnswer
                } else if arrived.is_some_and(|far| !far.thinking.trim().is_empty())
                    || self.model.is_empty()
                {
                    Message::AiThinking
                } else {
                    Message::AiWaitingForModel(shorten_model(&self.model))
                };
                Some((None, said.say(lang)))
            }
            Some(Doing::Asking | Doing::Allowing) | None => None,
        }
    }
}

fn flush(
    ui: &mut egui::Ui,
    group: &mut Vec<Step<'_>>,
    at: usize,
    after: &mut Vec<&Message>,
    lang: Lang,
) {
    if !group.is_empty() {
        what_the_tools_did(ui, group, &format!("run-{at}"), lang);
        group.clear();
    }
    after.dedup();
    for said in after.drain(..) {
        a_note(ui, &said.say(lang));
    }
}
