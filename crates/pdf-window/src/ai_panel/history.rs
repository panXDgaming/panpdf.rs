use std::collections::BTreeSet;
use std::path::PathBuf;

use eframe::egui;
use pdf_agent::history::{Chat, is_a_chat_id, name_for, newest_first, title_of, write_turns};

use pdf_app::wording::{Lang, Message};

use super::AiState;

const MOST_KEPT: usize = 200;

const SECONDS_BEFORE_TRYING_AGAIN: u64 = 10;

fn folder() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::own_folder::own_file("chats")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn taken(folder: &std::path::Path) -> BTreeSet<String> {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return BTreeSet::new();
    };
    listing
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

#[cfg(unix)]
fn make_the_folder(folder: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)
}

#[cfg(not(unix))]
fn make_the_folder(folder: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)
}

#[cfg(unix)]
fn open_for_writing(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_for_writing(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

fn write_privately(file: &std::path::Path, beside: &std::path::Path, text: &str) -> bool {
    use std::io::Write;
    let _ = std::fs::remove_file(beside);
    let Ok(mut made) = open_for_writing(beside) else {
        return false;
    };
    if made.write_all(text.as_bytes()).is_err() {
        drop(made);
        let _ = std::fs::remove_file(beside);
        return false;
    }
    drop(made);
    if std::fs::rename(beside, file).is_err() {
        let _ = std::fs::remove_file(beside);
        return false;
    }
    true
}

impl AiState {
    pub(crate) fn save_the_chat(&mut self) {
        if self.turns.len() == self.saved_turns || self.turns.is_empty() {
            return;
        }
        if now() < self.save_again_at {
            return;
        }
        let Some(folder) = folder() else {
            return;
        };
        if make_the_folder(&folder).is_err() {
            self.save_again_at = now() + SECONDS_BEFORE_TRYING_AGAIN;
            return;
        }
        if self.chat_id.is_empty() {
            self.chat_id = name_for(now(), &taken(&folder));
        }
        let chat = Chat {
            id: self.chat_id.clone(),
            title: title_of(&self.turns),
            changed: now(),
            model: self.model.clone(),
            documents: self.documents.clone(),
            places: self.places.clone(),
            turns: Vec::new(),
        };
        let text = write_turns(&chat, &self.turns);
        let file = folder.join(&self.chat_id);
        let beside = folder.join(format!("{}.writing", self.chat_id));
        if write_privately(&file, &beside, &text) {
            self.saved_turns = self.turns.len();
            self.history = None;
            trim(&folder, &self.chat_id);
        } else {
            self.save_again_at = now() + SECONDS_BEFORE_TRYING_AGAIN;
        }
    }

    pub(super) fn the_chats(&mut self) -> &[Chat] {
        if self.history.is_none() {
            let chats = folder().map_or_else(Vec::new, |folder| {
                let Ok(listing) = std::fs::read_dir(&folder) else {
                    return Vec::new();
                };
                let texts = listing
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_none())
                    .filter_map(|path| {
                        let name = path.file_name()?.to_str()?.to_owned();
                        Some((name, std::fs::read_to_string(path).ok()?))
                    });
                newest_first(texts)
            });
            self.history = Some(chats);
        }
        self.history.as_deref().unwrap_or_default()
    }

    pub(super) fn open_a_chat(&mut self, chat: &Chat) {
        let here = self.documents.last().cloned();
        let place = self.places.last().cloned().unwrap_or_default();
        self.take_up(chat);
        if let Some(here) = here {
            self.moved_to(here, place);
        }
    }

    pub(super) fn take_up(&mut self, chat: &Chat) {
        self.cancel();
        self.tools.clear();
        self.tools.plan = pdf_agent::context::plan_in(&chat.turns);
        self.notes.clear();
        self.pending.clear();
        self.drawn.clear();
        self.model_calls = 0;
        self.summarising = false;
        self.partial = None;
        self.notice = None;
        self.context.clear();
        self.rewound = None;
        self.recall.forget();
        self.turns.clone_from(&chat.turns);
        self.chat_id.clone_from(&chat.id);
        self.documents.clone_from(&chat.documents);
        self.places.clone_from(&chat.places);
        self.saved_turns = chat.turns.len();
    }

    pub(super) fn forget_a_chat(&mut self, id: &str) {
        if !is_a_chat_id(id) {
            return;
        }
        if let Some(folder) = folder() {
            let _ = std::fs::remove_file(folder.join(id));
        }
        if self.chat_id == id {
            self.new_chat();
        }
        self.history = None;
    }

    pub(super) fn the_history(&mut self, ui: &mut egui::Ui, lang: Lang) {
        let say = |message: Message| message.say(lang);
        let title = if self.turns.is_empty() {
            say(Message::AiNewChatTitle)
        } else {
            let made = title_of(&self.turns);
            if made.is_empty() {
                say(Message::AiUntitledChat)
            } else {
                made
            }
        };
        let shown: String = if title.chars().count() > 26 {
            title
                .chars()
                .take(25)
                .chain(std::iter::once('\u{2026}'))
                .collect()
        } else {
            title
        };
        let mut open = None;
        let mut forget = None;
        let here = self.chat_id.clone();
        let place = self.places.last().cloned().unwrap_or_default();
        ui.menu_button(
            egui::RichText::new(format!("{shown}  \u{2304}")).strong(),
            |ui| {
                ui.set_min_width(300.0);
                ui.label(egui::RichText::new(say(Message::AiHistory)).small().weak());
                let chats: Vec<Chat> = self.the_chats().to_vec();
                if chats.is_empty() {
                    ui.weak(say(Message::AiNoChatsYet));
                    return;
                }
                let (ours, others): (Vec<&Chat>, Vec<&Chat>) = chats
                    .iter()
                    .partition(|chat| !place.is_empty() && chat.places.contains(&place));
                egui::ScrollArea::vertical()
                    .id_salt("ai-history")
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for (heading, group) in [
                            (Message::AiThisDocument, &ours),
                            (Message::AiOtherChats, &others),
                        ] {
                            if group.is_empty() {
                                continue;
                            }
                            if !ours.is_empty() {
                                ui.label(egui::RichText::new(say(heading)).small().strong());
                            }
                            for chat in group {
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button("\u{2715}")
                                        .on_hover_text(say(Message::AiForgetChat))
                                        .clicked()
                                    {
                                        forget = Some(chat.id.clone());
                                    }
                                    if one_chat(ui, chat, chat.id == here, lang).clicked() {
                                        open = Some((*chat).clone());
                                        ui.close();
                                    }
                                });
                            }
                        }
                    });
            },
        )
        .response
        .on_hover_text(say(Message::AiHistory));
        if let Some(chat) = open {
            self.open_a_chat(&chat);
        }
        if let Some(id) = forget {
            self.forget_a_chat(&id);
        }
    }
}

fn trim(folder: &std::path::Path, keep: &str) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    let mut chats: Vec<(std::time::SystemTime, PathBuf)> = listing
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| is_a_chat_id(name) && name != keep)
        })
        .map(|entry| {
            let changed = entry
                .metadata()
                .and_then(|facts| facts.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (changed, entry.path())
        })
        .collect();
    if chats.len() < MOST_KEPT {
        return;
    }
    chats.sort();
    let over = chats.len() + 1 - MOST_KEPT;
    for (_, path) in chats.into_iter().take(over) {
        let _ = std::fs::remove_file(path);
    }
}

fn one_chat(ui: &mut egui::Ui, chat: &Chat, here: bool, lang: Lang) -> egui::Response {
    use egui::text::{LayoutJob, TextFormat};
    let title = if chat.title.is_empty() {
        Message::AiUntitledChat.say(lang)
    } else {
        chat.title.clone()
    };
    let style = ui.style();
    let visuals = &style.visuals;
    let mut job = LayoutJob::default();
    job.append(
        &title,
        0.0,
        TextFormat {
            font_id: egui::TextStyle::Body.resolve(style),
            color: visuals.text_color(),
            ..TextFormat::default()
        },
    );
    let mut under = chat.documents.join(", ");
    if !chat.model.is_empty() {
        if !under.is_empty() {
            under.push_str("  \u{00b7}  ");
        }
        under.push_str(chat.model.rsplit('/').next().unwrap_or(&chat.model));
    }
    if !under.is_empty() {
        job.append(
            &format!("\n{under}"),
            0.0,
            TextFormat {
                font_id: egui::TextStyle::Small.resolve(style),
                color: visuals.weak_text_color(),
                ..TextFormat::default()
            },
        );
    }
    job.wrap.max_width = 260.0;
    ui.add(
        egui::Button::new(job)
            .frame(false)
            .selected(here)
            .min_size(egui::vec2(260.0, 0.0)),
    )
}
