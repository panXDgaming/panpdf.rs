use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use pdf_agent::history::{Chat, is_a_chat_id, name_for, newest_first, title_of, write_turns};

use super::AiState;

const MOST_KEPT: usize = 200;

const SECONDS_BEFORE_TRYING_AGAIN: u64 = 10;

fn folder() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::own_folder::own_file("chats")
}

pub(super) fn now() -> u64 {
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
            let gone = trim(&folder, &self.chat_id);
            self.shelve(&text, &gone);
        } else {
            self.save_again_at = now() + SECONDS_BEFORE_TRYING_AGAIN;
        }
    }

    pub(super) fn shelve(&mut self, text: &str, gone: &[String]) {
        let Some(listed) = self.history.as_mut() else {
            return;
        };
        let Some(saved) = pdf_agent::history::read(text) else {
            self.history = None;
            return;
        };
        let list = Arc::make_mut(listed);
        list.retain(|chat| chat.id != saved.id && !gone.contains(&chat.id));
        list.insert(0, saved);
    }

    pub(super) fn the_chats(&mut self) -> Arc<Vec<Chat>> {
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
            self.history = Some(Arc::new(chats));
        }
        self.history.as_ref().map_or_else(Arc::default, Arc::clone)
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
        if let Some(listed) = self.history.as_mut() {
            Arc::make_mut(listed).retain(|chat| chat.id != id);
        }
    }
}

fn trim(folder: &std::path::Path, keep: &str) -> Vec<String> {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return Vec::new();
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
        return Vec::new();
    }
    chats.sort();
    let over = chats.len() + 1 - MOST_KEPT;
    let mut gone = Vec::new();
    for (_, path) in chats.into_iter().take(over) {
        if std::fs::remove_file(&path).is_ok()
            && let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str)
        {
            gone.push(name.to_owned());
        }
    }
    gone
}
