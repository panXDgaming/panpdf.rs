use eframe::egui;
use pdf_app::wording::{Assistant, Lang, Message};

use super::cards::{card_frame, the_notice};
use super::look::{
    Chip, caption, chip, field, foldable, like_a_field, link, primary_button, small, spinner,
    tick_mark,
};
use super::{AiState, EFFORTS, GAP, Notice, Provider, effort_means, effort_said, shorten_model};
use crate::icons::Icon;

const INFO_WIDE: f32 = 300.0;

impl Provider {
    pub(super) fn label(self, lang: Lang) -> String {
        match self {
            Self::OpenAi => "OpenAI".to_owned(),
            Self::Claude => "Anthropic".to_owned(),
            Self::Gemini => "Gemini".to_owned(),
            Self::Ollama => "Ollama".to_owned(),
            Self::LmStudio => "LM Studio".to_owned(),
            Self::Custom => Assistant::ProviderCustom.say(lang),
        }
    }

    pub(super) const fn hosted(self) -> bool {
        matches!(self, Self::OpenAi | Self::Claude | Self::Gemini)
    }

    pub(super) const fn key_page(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some("https://platform.openai.com/api-keys"),
            Self::Claude => Some("https://console.anthropic.com/settings/keys"),
            Self::Gemini => Some("https://aistudio.google.com/apikey"),
            Self::Ollama | Self::LmStudio | Self::Custom => None,
        }
    }

    pub(super) const fn takes_a_key(self) -> bool {
        matches!(
            self,
            Self::OpenAi | Self::Claude | Self::Gemini | Self::Custom
        )
    }

    pub(super) const fn family(self) -> pdf_app::ai_choice::Family {
        match self {
            Self::OpenAi => pdf_app::ai_choice::Family::OpenAi,
            Self::Claude => pdf_app::ai_choice::Family::Anthropic,
            Self::Gemini => pdf_app::ai_choice::Family::Gemini,
            Self::Ollama | Self::LmStudio | Self::Custom => pdf_app::ai_choice::Family::Other,
        }
    }
}

impl AiState {
    pub(super) fn switch_provider(&mut self, chosen: Provider) {
        if chosen == self.provider || self.busy() {
            return;
        }
        let key = std::mem::take(&mut self.key);
        if !key.is_empty() {
            self.other_keys.insert(self.provider.name(), key);
        }
        self.provider = chosen;
        self.base_url = chosen.base_url().into();
        self.model.clear();
        self.key = self.other_keys.remove(chosen.name()).unwrap_or_default();
        self.remember_key = false;
        self.invalidate();
        self.take_the_kept_key();
        self.remember();
    }

    pub(super) fn take_the_models(&mut self, models: Vec<pdf_agent::connect::Model>) {
        self.connected = true;
        self.edited = false;
        self.models = models;
        if !self.models.iter().any(|model| model.id == self.model) {
            let ids: Vec<String> = self.models.iter().map(|model| model.id.clone()).collect();
            self.model = pdf_app::ai_choice::sensible_model(self.provider.family(), &ids)
                .unwrap_or_default();
        }
        self.settings_open = self.model.is_empty();
        self.keep_the_key();
        self.remember();
    }

    pub(super) fn the_summary(&mut self, ui: &mut egui::Ui, lang: Lang) {
        let said = Assistant::ProviderModelSummary {
            provider: self.provider.label(lang),
            model: self.model.clone(),
        }
        .say(lang);
        card_frame(ui, None).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add(egui::Label::new(small(said)).truncate());
                tick_mark(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if link(ui, &Assistant::Change.say(lang)).clicked() {
                        self.settings_open = true;
                    }
                });
            });
        });
    }

    fn the_providers(&mut self, ui: &mut egui::Ui, lang: Lang) {
        ui.label(caption(Message::AiProvider.say(lang)));
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for provider in Provider::ALL {
                let label = provider.label(lang);
                let on = provider == self.provider;
                let response = chip(
                    ui,
                    &Chip {
                        on,
                        ..Chip::new(&label)
                    },
                );
                if response.clicked() && !on {
                    chosen = Some(provider);
                }
            }
        });
        if let Some(provider) = chosen {
            self.switch_provider(provider);
        }
    }

    fn the_key(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, lang: Lang) {
        if !self.provider.takes_a_key() {
            ui.add(egui::Label::new(caption(Assistant::LocalPrivacy.say(lang))).wrap());
            return;
        }
        let name = if self.provider.hosted() {
            Message::AiApiKey.say(lang)
        } else {
            Assistant::KeyOptional.say(lang)
        };
        ui.horizontal(|ui| {
            ui.label(caption(name));
            if let Some(page) = self.provider.key_page() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let words = if self.provider == Provider::Gemini {
                        Assistant::GetAFreeKey.say(lang)
                    } else {
                        Assistant::GetAKey.say(lang)
                    };
                    if link(ui, &words).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(page));
                    }
                });
            }
        });
        let key_id = egui::Id::new("ai-key-field");
        let typed = field(ui, key_id, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.key)
                    .id(key_id)
                    .password(true)
                    .frame(egui::Frame::NONE)
                    .hint_text(Assistant::KeyHint.say(lang))
                    .desired_width(f32::INFINITY),
            )
        });
        if typed.changed() {
            self.invalidate();
        }
        if typed.lost_focus()
            && ui.input(|input| input.key_pressed(egui::Key::Enter))
            && self.may_connect()
        {
            self.start_models(ctx);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let privacy = Assistant::KeyPrivacy(self.provider.label(lang)).say(lang);
            ui.add(egui::Label::new(caption(privacy)).wrap());
        });
        ui.horizontal(|ui| {
            if super::keeping::can_keep()
                && ui
                    .checkbox(
                        &mut self.remember_key,
                        small(Message::AiKeepTheKey.say(lang)),
                    )
                    .on_hover_text(Message::AiKeepTheKeyMeans.say(lang))
                    .changed()
            {
                self.keep_the_key();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                Icon::Info.draw(
                    ui.painter(),
                    rect.shrink(1.0),
                    ui.visuals().weak_text_color(),
                );
                response.on_hover_ui(|ui| {
                    ui.set_max_width(INFO_WIDE);
                    ui.label(small(Message::AiPrivacy.say(lang)));
                });
            });
        });
    }

    fn the_address(&mut self, ui: &mut egui::Ui, lang: Lang) {
        ui.label(caption(Message::AiBaseUrl.say(lang)));
        let address_id = egui::Id::new("ai-address-field");
        let address = field(ui, address_id, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.base_url)
                    .id(address_id)
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY),
            )
        });
        if address.changed() {
            self.invalidate();
        }
        if address.lost_focus() {
            self.remember();
        }
    }

    fn the_models(&mut self, ui: &mut egui::Ui, lang: Lang) {
        ui.label(caption(Message::AiModel.say(lang)));
        let shown = if self.model.is_empty() {
            Message::AiNoModelChosen.say(lang)
        } else {
            shorten_model(&self.model)
        };
        let mut picked = None;
        ui.scope(|ui| {
            like_a_field(ui);
            egui::ComboBox::from_id_salt("ai-model")
                .width(ui.available_width())
                .selected_text(shown)
                .wrap_mode(egui::TextWrapMode::Truncate)
                .height(320.0)
                .show_ui(ui, |ui| {
                    let current = (!self.models.iter().any(|model| model.id == self.model)
                        && !self.model.is_empty())
                    .then(|| self.model.clone());
                    for id in current
                        .iter()
                        .chain(self.models.iter().map(|model| &model.id))
                    {
                        if ui
                            .selectable_label(self.model == *id, shorten_model(id))
                            .clicked()
                        {
                            picked = Some(id.clone());
                        }
                    }
                });
        });
        if let Some(model) = picked {
            self.model = model;
            self.notice = None;
            self.remember();
        }
    }

    fn the_thinking(&mut self, ui: &mut egui::Ui, lang: Lang) {
        ui.label(caption(Message::AiEffort.say(lang)));
        let level = effort_said(self.effort).say(lang);
        let level = level.rsplit(": ").next().unwrap_or(&level).to_owned();
        let mut picked = None;
        ui.scope(|ui| {
            like_a_field(ui);
            egui::ComboBox::from_id_salt("ai-effort")
                .width(ui.available_width())
                .selected_text(capitalised(&level))
                .show_ui(ui, |ui| {
                    for one in EFFORTS {
                        let said = effort_said(one).say(lang);
                        let name = capitalised(said.rsplit(": ").next().unwrap_or(&said));
                        if ui
                            .selectable_label(self.effort == one, name)
                            .on_hover_text(effort_means(one).say(lang))
                            .clicked()
                        {
                            picked = Some(one);
                        }
                    }
                });
        });
        if let Some(effort) = picked
            && effort != self.effort
        {
            self.effort = effort;
            self.remember();
        }
    }

    fn may_connect(&self) -> bool {
        !self.busy() && !self.base_url.trim().is_empty() && !self.key_missing()
    }

    pub(super) fn the_connection(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, lang: Lang) {
        let mut disconnect = false;
        let mut close = false;
        card_frame(ui, None).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add_enabled_ui(!self.busy() || self.checking, |ui| {
                self.the_providers(ui, lang);
                ui.add_space(2.0);
                self.the_key(ui, ctx, lang);
                if !self.provider.hosted() {
                    self.the_address(ui, lang);
                }
                if !self.models.is_empty() || !self.model.is_empty() {
                    self.the_models(ui, lang);
                }
                foldable(
                    ui,
                    egui::Id::new("ai-advanced"),
                    &Assistant::Advanced.say(lang),
                    false,
                    |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        if self.provider.hosted() {
                            self.the_address(ui, lang);
                        }
                        self.the_thinking(ui, lang);
                    },
                );
            });
            if let Some(notice) = self.notice.clone() {
                let plain = Notice {
                    action: None,
                    ..notice
                };
                if the_notice(ui, &plain, lang).dismissed {
                    self.notice = None;
                }
            }
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                if self.checking {
                    primary_button(ui, &Assistant::Connecting.say(lang), false);
                    spinner(ui);
                } else if self.ready() && !self.edited {
                    if primary_button(ui, &Assistant::Done.say(lang), true).clicked() {
                        close = true;
                    }
                } else if primary_button(ui, &Assistant::Connect.say(lang), self.may_connect())
                    .clicked()
                {
                    self.start_models(ctx);
                }
                if self.provider.takes_a_key()
                    && !self.key.is_empty()
                    && ui
                        .add(
                            egui::Button::new(Message::AiDisconnect.say(lang))
                                .min_size(egui::vec2(0.0, crate::format::CONTROL_HEIGHT)),
                        )
                        .clicked()
                {
                    disconnect = true;
                }
            });
        });
        if disconnect {
            self.disconnect();
        }
        if close {
            self.settings_open = false;
        }
    }
}

fn capitalised(word: &str) -> String {
    let mut letters = word.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}
