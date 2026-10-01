use eframe::egui;
use pdf_app::wording::{Assistant, Lang, Message};

use super::AiState;
use super::look::{Dot, caption, status_dot};
use crate::icons::Icon;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Badge {
    Working,
    NeedsYou,
}

impl AiState {
    pub(crate) fn badge(&self) -> Option<Badge> {
        if self.tools.ask.is_some() || self.tools.question.is_some() {
            Some(Badge::NeedsYou)
        } else if self.working() {
            Some(Badge::Working)
        } else {
            None
        }
    }

    fn dot(&self) -> Dot {
        match self.badge() {
            Some(Badge::NeedsYou) => Dot::NeedsYou,
            Some(Badge::Working) => Dot::Working,
            None if self.ready() => Dot::Ready,
            None => Dot::Offline,
        }
    }

    fn dot_words(&self, lang: Lang) -> String {
        match self.dot() {
            Dot::NeedsYou => Assistant::StatusNeedsYou.say(lang),
            Dot::Working => Assistant::StatusWorking.say(lang),
            Dot::Ready => Assistant::StatusReady(self.model.clone()).say(lang),
            Dot::Offline if self.checking => Message::AiCheckingConnection.say(lang),
            Dot::Offline => Assistant::StatusOffline.say(lang),
        }
    }

    pub(super) fn the_heading(&mut self, ui: &mut egui::Ui, lang: Lang) -> bool {
        let say = |message: Message| message.say(lang);
        let mut close = false;
        let row = egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(ui.available_width(), crate::format::CONTROL_HEIGHT),
        );
        ui.horizontal(|ui| {
            ui.set_min_height(crate::format::CONTROL_HEIGHT);
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(4.0);
            status_dot(ui, self.dot()).on_hover_text(self.dot_words(lang));
            ui.label(caption(say(Message::AiTitle)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::format::icon_button(ui, Icon::Close, &say(Message::Close), false, true)
                    .clicked()
                {
                    close = true;
                }
                if crate::format::icon_button(
                    ui,
                    Icon::Settings,
                    &say(Message::AiConnection),
                    self.settings_open,
                    true,
                )
                .clicked()
                {
                    self.settings_open = !self.settings_open;
                }
                if crate::format::icon_button(
                    ui,
                    Icon::NewChat,
                    &say(Message::AiNewChat),
                    false,
                    !self.turns.is_empty(),
                )
                .clicked()
                {
                    self.new_chat();
                    self.focus_composer = true;
                }
                self.the_chats_button(ui, lang, row);
            });
        });
        close
    }
}
