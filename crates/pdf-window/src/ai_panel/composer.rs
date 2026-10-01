use eframe::egui;
use pdf_app::ai_layout;
use pdf_app::ai_permission::Mode;
use pdf_app::wording::{Assistant, Lang, Message};

use super::look::{CAPTION, Chip, ROUND, caption, chip, round_button, small};
use super::{
    AiState, EFFORTS, GAP, composer_id, effort_said, mode_means, mode_said, plain_enter,
    shorten_model,
};
use crate::icons::Icon;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Asked {
    pub(super) asked: bool,
    pub(super) attach: bool,
}

const MENU_WIDE: f32 = 292.0;

const MODES: [Mode; 3] = [Mode::ChatOnly, Mode::AskBeforeChanges, Mode::DoIt];

const fn mode_icon(mode: Mode) -> Icon {
    match mode {
        Mode::ChatOnly => Icon::Chat,
        Mode::AskBeforeChanges => Icon::Checkbox,
        Mode::DoIt | Mode::Free => Icon::Pen,
    }
}

fn menu_row(
    ui: &mut egui::Ui,
    (name, what): (&str, &str),
    selected: bool,
    warn: bool,
) -> egui::Response {
    let width = ui.available_width();
    let visuals = ui.visuals().clone();
    let name_galley = ui.painter().layout(
        name.to_owned(),
        egui::TextStyle::Body.resolve(ui.style()),
        if warn {
            visuals.warn_fg_color
        } else {
            visuals.text_color()
        },
        width - 44.0,
    );
    let what_galley = ui.painter().layout(
        what.to_owned(),
        egui::FontId::proportional(CAPTION),
        visuals.weak_text_color(),
        width - 44.0,
    );
    let height = 12.0 + name_galley.size().y + 2.0 + what_galley.size().y;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, ROUND, visuals.widgets.hovered.weak_bg_fill);
    }
    let top = rect.top() + 6.0;
    let below = top + name_galley.size().y + 2.0;
    ui.painter().galley(
        egui::pos2(rect.left() + 10.0, top),
        name_galley,
        visuals.text_color(),
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 10.0, below),
        what_galley,
        visuals.weak_text_color(),
    );
    if selected {
        let slot = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 16.0, rect.center().y),
            egui::vec2(13.0, 13.0),
        );
        Icon::Check.draw(
            ui.painter(),
            slot,
            if warn {
                visuals.warn_fg_color
            } else {
                visuals.selection.stroke.color
            },
        );
    }
    response
}

fn attachment_chip(ui: &mut egui::Ui, name: &str, label: &str, refused: bool, lang: Lang) -> bool {
    let mut remove = false;
    let visuals = ui.visuals().clone();
    let edge = if refused {
        visuals.error_fg_color.gamma_multiply(0.6)
    } else {
        visuals.widgets.noninteractive.bg_stroke.color
    };
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, edge))
        .corner_radius(13)
        .inner_margin(egui::Margin {
            left: 9,
            right: 2,
            top: 1,
            bottom: 1,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let ink = if refused {
                    visuals.error_fg_color
                } else {
                    visuals.text_color()
                };
                ui.add(
                    egui::Label::new(small(super::look::clipped(name, 24)).color(ink)).truncate(),
                );
                ui.label(caption(label));
                if crate::format::quiet_icon_button(ui, Icon::Close, &Message::Close.say(lang))
                    .clicked()
                {
                    remove = true;
                }
            });
        });
    remove
}

impl AiState {
    fn the_context_chips(&mut self, ui: &mut egui::Ui, page: Option<usize>, lang: Lang) {
        if page.is_none() && self.pending.is_empty() {
            return;
        }
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            if let Some(page) = page {
                let words = Assistant::Page(page).say(lang);
                let on = self.include_context;
                let response = chip(
                    ui,
                    &Chip {
                        icon: Some(Icon::Document),
                        trailing: on.then_some(Icon::Close),
                        on,
                        dashed: !on,
                        ..Chip::new(&words)
                    },
                );
                let hover = if on {
                    Assistant::PageChipOn
                } else {
                    Assistant::PageChipOff
                };
                if response.on_hover_text(hover.say(lang)).clicked() {
                    self.include_context = !on;
                }
            }
            for (at, item) in self.pending.iter().enumerate() {
                let label = item.label(lang);
                if attachment_chip(ui, &item.name, &label, item.refused(), lang) {
                    remove = Some(at);
                }
            }
        });
        if let Some(at) = remove {
            self.remove_pending(at);
        }
    }

    fn the_mode_chip(&mut self, ui: &mut egui::Ui, lang: Lang, row: egui::Rect) {
        let say = |message: Message| message.say(lang);
        let label = say(mode_said(self.mode));
        let free = self.mode == Mode::Free;
        let warn = ui.visuals().warn_fg_color;
        let response = chip(
            ui,
            &Chip {
                icon: Some(mode_icon(self.mode)),
                trailing: Some(Icon::Expand),
                ink: free.then_some(warn),
                ..Chip::new(&label)
            },
        )
        .on_hover_text(say(mode_means(self.mode)));
        egui::Popup::menu(&response)
            .anchor(row)
            .align(egui::RectAlign::TOP_START)
            .show(|ui| {
                ui.set_width(MENU_WIDE);
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(caption(say(Message::AiMode)));
                });
                for one in MODES {
                    let on = self.mode == one;
                    if menu_row(ui, (&say(mode_said(one)), &say(mode_means(one))), on, false)
                        .clicked()
                    {
                        if self.mode != one {
                            self.mode = one;
                            self.remember();
                        }
                        ui.close();
                    }
                }
                ui.add_space(2.0);
                super::look::hairline(ui);
                ui.add_space(2.0);
                if menu_row(
                    ui,
                    (&say(Message::AiModeFree), &say(mode_means(Mode::Free))),
                    free,
                    true,
                )
                .clicked()
                {
                    if free {
                        self.mode = Mode::DoIt;
                        self.remember();
                    } else {
                        self.confirming_free = true;
                    }
                    ui.close();
                }
            });
    }

    pub(super) fn confirm_full_access(&mut self, ctx: &egui::Context, lang: Lang) {
        if !self.confirming_free {
            return;
        }
        let say = |message: Message| message.say(lang);
        let mut answer = None;
        egui::Modal::new(egui::Id::new("ai-full-access")).show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.label(
                egui::RichText::new(say(Message::AiFullAccessAsk))
                    .strong()
                    .color(ui.visuals().warn_fg_color)
                    .size(egui::TextStyle::Body.resolve(ui.style()).size * 1.15),
            );
            ui.add_space(6.0);
            ui.label(say(Message::AiFullAccessMeans));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new(say(Message::AiFullAccessConfirm))
                                .color(ui.visuals().warn_fg_color),
                        ))
                        .clicked()
                    {
                        answer = Some(true);
                    }
                    if ui
                        .button(say(Message::Home(pdf_app::wording::Home::Cancel)))
                        .clicked()
                    {
                        answer = Some(false);
                    }
                });
            });
        });
        match answer {
            Some(true) => {
                self.mode = Mode::Free;
                self.remember();
                self.confirming_free = false;
            }
            Some(false) => self.confirming_free = false,
            None => {}
        }
    }

    fn the_model_caption(&mut self, ui: &mut egui::Ui, lang: Lang) {
        if ai_layout::caption_room(ui.available_width(), GAP).is_none() {
            return;
        }
        let none = self.model.is_empty();
        let mut text = if none {
            Message::AiNoModelChosen.say(lang)
        } else {
            shorten_model(&self.model)
        };
        if !none && self.effort != pdf_agent::connect::Effort::Off && EFFORTS.contains(&self.effort)
        {
            let level = effort_said(self.effort).say(lang);
            text.push_str(" \u{b7} ");
            text.push_str(level.rsplit(": ").next().unwrap_or(&level));
        }
        let ink = if none {
            ui.visuals().warn_fg_color
        } else {
            ui.visuals().weak_text_color()
        };
        let response = ui.add(
            egui::Label::new(egui::RichText::new(text).size(CAPTION).color(ink))
                .truncate()
                .sense(egui::Sense::click()),
        );
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response
            .on_hover_text(if none {
                Message::AiNoModelChosen.say(lang)
            } else {
                self.model.clone()
            })
            .clicked()
        {
            self.settings_open = true;
        }
    }

    pub(super) fn the_composer(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        lang: Lang,
        page: Option<usize>,
        (rows, row_height): (usize, f32),
    ) -> Asked {
        let say = |message: Message| message.say(lang);
        let mut asked = false;
        let mut attach = false;
        let focused = ui.memory(|memory| memory.has_focus(composer_id()));
        let visuals = ui.visuals();
        let edge = if focused {
            visuals.selection.stroke.color.gamma_multiply(0.8)
        } else {
            visuals.widgets.noninteractive.bg_stroke.color
        };
        let frame = egui::Frame::new()
            .fill(visuals.extreme_bg_color)
            .stroke(egui::Stroke::new(1.0, edge))
            .corner_radius(10)
            .inner_margin(egui::Margin::same(8));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            self.the_context_chips(ui, page, lang);
            let recalled = self.recall_keys(ui, ctx);
            let mut typed = egui::ScrollArea::vertical()
                .id_salt("ai-composer")
                .max_height(ai_layout::composer_height(rows, row_height, 0.0))
                .show(ui, |ui| {
                    egui::TextEdit::multiline(&mut self.composer)
                        .id(composer_id())
                        .hint_text(say(Message::AiAskHint))
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows)
                        .return_key(Some(egui::KeyboardShortcut::new(
                            egui::Modifiers::SHIFT,
                            egui::Key::Enter,
                        )))
                        .show(ui)
                })
                .inner;
            self.wrapped_rows = typed.galley.rows.len();
            if std::mem::take(&mut self.focus_composer) && !self.settings_open {
                typed.response.request_focus();
            }
            if recalled {
                let end = self.composer.chars().count();
                typed
                    .state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(end),
                    )));
                typed.state.store(ui.ctx(), typed.response.id);
            }
            if typed.response.has_focus() && plain_enter(ui) && self.can_send() {
                asked = true;
            }
            if std::mem::take(&mut self.send_now) && self.can_send() {
                asked = true;
            }
            let row = egui::Rect::from_min_size(
                ui.cursor().min,
                egui::vec2(ui.available_width(), crate::format::CONTROL_HEIGHT),
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if crate::format::icon_button(
                    ui,
                    Icon::Attach,
                    &say(Message::AiAttach),
                    false,
                    !self.busy(),
                )
                .clicked()
                {
                    attach = true;
                }
                self.the_mode_chip(ui, lang, row);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if self.working() {
                        if round_button(ui, Icon::Stop, &say(Message::AiCancel), true).clicked() {
                            self.stop_the_run();
                        }
                    } else if round_button(ui, Icon::Send, &say(Message::AiSend), self.can_send())
                        .clicked()
                    {
                        asked = true;
                    }
                    self.the_model_caption(ui, lang);
                });
            });
        });
        if asked && !self.ready() {
            self.settings_open = true;
            self.notice = Some(super::Notice::plain(if self.key_missing() {
                Message::AiKeyNeeded
            } else {
                Message::AiNoModelChosen
            }));
            asked = false;
        }
        Asked { asked, attach }
    }

    fn recall_keys(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
        use pdf_app::ai_recall::Way;
        if !ui.memory(|memory| memory.has_focus(composer_id())) {
            return false;
        }
        for (key, way) in [
            (egui::Key::ArrowUp, Way::Up),
            (egui::Key::ArrowDown, Way::Down),
        ] {
            if !ui.input(|input| input.key_pressed(key) && input.modifiers.is_none()) {
                continue;
            }
            let caret_at_start = egui::TextEdit::load_state(ctx, composer_id())
                .and_then(|state| state.cursor.char_range())
                .is_some_and(|range| range.primary.index.0 == 0 && range.secondary.index.0 == 0);
            if self.recall_key(way, caret_at_start) {
                ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key));
                return true;
            }
        }
        false
    }
}
