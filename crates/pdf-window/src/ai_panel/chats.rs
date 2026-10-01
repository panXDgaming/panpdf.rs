use eframe::egui;
use pdf_agent::history::Chat;
use pdf_app::ai_chats::{sections, when};
use pdf_app::wording::{Assistant, Lang, Message};

use super::going_back::whole_chat;
use super::history::now;
use super::look::{CAPTION, ROUND, caption, framed_button, hairline, small};
use super::{AiState, GAP};
use crate::icons::Icon;

const LIST_WIDE: f32 = 332.0;
const ONE_LINE: f32 = 32.0;
const TWO_LINES: f32 = 46.0;
const ICON: f32 = 22.0;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Row {
    Open(String),
    Copy(String),
    AskToDelete(String),
    Delete(String),
    Keep,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum How {
    Plain,
    Open,
    Confirming,
}

struct RowState {
    how: How,
    with_document: bool,
    can_delete: bool,
    copied: bool,
    now: u64,
    lang: Lang,
}

fn fitted(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(8.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    ui.painter().layout_job(job)
}

fn asking_row(ui: &mut egui::Ui, rect: egui::Rect, chat: &Chat, lang: Lang) -> Option<Row> {
    let mut chosen = None;
    let mut inside = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(8.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    inside.label(small(Message::AiDeleteThisChatSure.say(lang)));
    inside.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let delete =
            egui::RichText::new(Assistant::DeleteChat.say(lang)).color(ui.visuals().error_fg_color);
        if framed_button(ui, delete).clicked() {
            chosen = Some(Row::Delete(chat.id.clone()));
        }
        if framed_button(ui, egui::RichText::new(Assistant::KeepChat.say(lang))).clicked() {
            chosen = Some(Row::Keep);
        }
    });
    chosen
}

fn icons_of_row(
    ui: &mut egui::Ui,
    strip: egui::Rect,
    chat: &Chat,
    state: &RowState,
) -> Option<Row> {
    let lang = state.lang;
    let mut chosen = None;
    let mut icons = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(strip)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    icons.spacing_mut().item_spacing.x = 2.0;
    let copy_icon = if state.copied {
        Icon::Check
    } else {
        Icon::Copy
    };
    if crate::format::quiet_icon_button(&mut icons, copy_icon, &Message::AiCopyWholeChat.say(lang))
        .clicked()
    {
        chosen = Some(Row::Copy(chat.id.clone()));
    }
    if state.can_delete
        && crate::format::quiet_icon_button(
            &mut icons,
            Icon::Delete,
            &Message::AiForgetChat.say(lang),
        )
        .clicked()
    {
        chosen = Some(Row::AskToDelete(chat.id.clone()));
    }
    chosen
}

fn the_row(ui: &mut egui::Ui, chat: &Chat, state: &RowState) -> Option<Row> {
    let two = state.with_document && !chat.documents.is_empty();
    let height = if two { TWO_LINES } else { ONE_LINE };
    let confirming = state.how == How::Confirming;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        if confirming {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        },
    );
    let lang = state.lang;
    if confirming {
        return asking_row(ui, rect, chat, lang);
    }
    let visuals = ui.visuals().clone();
    let hot = ui.rect_contains_pointer(rect);
    if state.how == How::Open {
        ui.painter()
            .rect_filled(rect, ROUND, visuals.selection.bg_fill.gamma_multiply(0.3));
    } else if hot {
        ui.painter()
            .rect_filled(rect, ROUND, visuals.widgets.hovered.weak_bg_fill);
    }
    let title = if chat.title.is_empty() {
        Message::AiUntitledChat.say(lang)
    } else {
        chat.title.clone()
    };
    let when_said = when(chat.changed, state.now).say(lang);
    let when_galley = fitted(
        ui,
        &when_said,
        egui::FontId::proportional(CAPTION),
        visuals.weak_text_color(),
        120.0,
    );
    let icons_wide = ICON * 2.0 + 2.0;
    let side = if hot {
        icons_wide
    } else {
        when_galley.size().x
    };
    let left = rect.left() + 8.0;
    let title_galley = fitted(
        ui,
        &title,
        egui::TextStyle::Body.resolve(ui.style()),
        visuals.text_color(),
        rect.width() - 16.0 - side - 8.0,
    );
    let top = if two {
        rect.top() + 6.0
    } else {
        rect.center().y - title_galley.size().y / 2.0
    };
    ui.painter().galley(
        egui::pos2(left, top),
        title_galley.clone(),
        visuals.text_color(),
    );
    if two {
        let galley = fitted(
            ui,
            &chat.documents.join(", "),
            egui::FontId::proportional(CAPTION),
            visuals.weak_text_color(),
            rect.width() - 16.0,
        );
        ui.painter().galley(
            egui::pos2(left, top + title_galley.size().y + 1.0),
            galley,
            visuals.weak_text_color(),
        );
    }
    let mut chosen = None;
    if hot {
        let strip = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 8.0 - icons_wide, rect.top() + 4.0),
            egui::vec2(icons_wide, ICON),
        );
        chosen = icons_of_row(ui, strip, chat, state);
    } else {
        let at = egui::pos2(
            rect.right() - 8.0 - when_galley.size().x,
            top + (title_galley.size().y - when_galley.size().y) / 2.0 + 1.0,
        );
        ui.painter()
            .galley(at, when_galley, visuals.weak_text_color());
    }
    if chosen.is_none() && response.clicked() {
        chosen = Some(Row::Open(chat.id.clone()));
    }
    chosen
}

impl AiState {
    pub(super) fn the_chats_button(&mut self, ui: &mut egui::Ui, lang: Lang, row: egui::Rect) {
        let button = crate::format::icon_button(
            ui,
            Icon::History,
            &Message::AiHistory.say(lang),
            self.chats_open,
            true,
        );
        let mut picked: Option<Row> = None;
        let shown = egui::Popup::menu(&button)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .anchor(row)
            .align(egui::RectAlign::BOTTOM_START)
            .show(|ui| {
                ui.set_width(LIST_WIDE);
                picked = self.the_list(ui, lang);
            });
        if shown.is_none() {
            self.deleting = None;
            self.copied_chat = None;
        }
        self.chats_open = shown.is_some();
        match picked {
            Some(Row::Open(id)) => {
                let chats = self.the_chats();
                if let Some(chat) = chats.iter().find(|chat| chat.id == id)
                    && chat.id != self.chat_id
                {
                    self.open_a_chat(chat);
                }
                egui::Popup::close_all(ui.ctx());
            }
            Some(Row::Copy(id)) => {
                let chats = self.the_chats();
                if let Some(chat) = chats.iter().find(|chat| chat.id == id) {
                    ui.ctx()
                        .copy_text(whole_chat(&chat.turns, &chat.model, lang));
                    self.copied_chat = Some(id);
                }
            }
            Some(Row::AskToDelete(id)) => self.deleting = Some(id),
            Some(Row::Delete(id)) => {
                self.deleting = None;
                self.forget_a_chat(&id);
            }
            Some(Row::Keep) => self.deleting = None,
            None => {}
        }
    }

    fn the_list(&mut self, ui: &mut egui::Ui, lang: Lang) -> Option<Row> {
        let chats = self.the_chats();
        if chats.is_empty() {
            ui.add_space(GAP);
            ui.label(caption(Message::AiNoChatsYet.say(lang)));
            ui.add_space(GAP);
            return None;
        }
        let place = self.places.last().cloned().unwrap_or_default();
        let split = sections(&chats, &place, |chat| chat.places.as_slice());
        let now = now();
        let working = self.working();
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("ai-chats")
            .max_height(400.0)
            .auto_shrink([true, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let mut first = true;
                for (heading, group, with_document) in [
                    (Message::AiThisDocument, &split.here, false),
                    (Message::AiOtherChats, &split.elsewhere, true),
                ] {
                    if group.is_empty() {
                        continue;
                    }
                    if !first {
                        ui.add_space(GAP * 0.5);
                        hairline(ui);
                    }
                    first = false;
                    ui.add_space(GAP * 0.5);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(caption(heading.say(lang)));
                    });
                    for chat in group {
                        let how = if self.deleting.as_deref() == Some(chat.id.as_str()) {
                            How::Confirming
                        } else if chat.id == self.chat_id {
                            How::Open
                        } else {
                            How::Plain
                        };
                        let state = RowState {
                            how,
                            with_document,
                            can_delete: !(working && chat.id == self.chat_id),
                            copied: self.copied_chat.as_deref() == Some(chat.id.as_str()),
                            now,
                            lang,
                        };
                        if let Some(row) = the_row(ui, chat, &state) {
                            picked = Some(row);
                        }
                    }
                }
            });
        picked
    }
}
