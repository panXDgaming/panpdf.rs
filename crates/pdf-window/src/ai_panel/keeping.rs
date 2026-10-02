use std::io::Read;
use std::path::PathBuf;

use pdf_app::ai_key;

use super::{AiState, Provider};

fn state() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::own_folder::folder()
}

fn key_file() -> Option<PathBuf> {
    Some(state()?.join("ai-key"))
}

fn random(count: usize) -> Option<Vec<u8>> {
    let mut bytes = vec![0_u8; count];
    let mut source = std::fs::File::open("/dev/urandom").ok()?;
    source.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

#[cfg(not(unix))]
fn write_privately(_path: &std::path::Path, _bytes: &[u8]) -> bool {
    false
}

#[cfg(unix)]
fn write_privately(path: &std::path::Path, bytes: &[u8]) -> bool {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let Some(folder) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(folder).is_err() {
        return false;
    }
    let beside = folder.join(format!(
        "{}.writing",
        path.file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("x")
    ));
    let made = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&beside);
    let Ok(mut file) = made else {
        return false;
    };
    if file.write_all(bytes).is_err() || file.sync_all().is_err() {
        drop(file);
        let _ = std::fs::remove_file(&beside);
        return false;
    }
    drop(file);
    if std::fs::rename(&beside, path).is_err() {
        let _ = std::fs::remove_file(&beside);
        return false;
    }
    true
}

fn machine(make: bool) -> Option<Vec<u8>> {
    let file = state()?.join("installation");
    let secret = match std::fs::read(&file) {
        Ok(bytes) if bytes.len() >= 32 => bytes,
        _ => {
            if !make {
                return None;
            }
            let fresh = random(32)?;
            if !write_privately(&file, &fresh) {
                return None;
            }
            fresh
        }
    };
    let mut material = secret;
    if let Some(home) = std::env::var_os("HOME") {
        material.extend_from_slice(home.as_encoded_bytes());
    }
    Some(material)
}

const SEPARATOR: char = '\u{1f}';

struct Kept {
    provider: Option<String>,
}

fn bound(provider: &str, key: &str) -> String {
    format!("{provider}{SEPARATOR}{key}")
}

fn unbound(secret: &str) -> (Option<&str>, &str) {
    match secret.split_once(SEPARATOR) {
        Some((provider, key)) => (Some(provider), key),
        None => (None, secret),
    }
}

fn read_kept() -> Option<Kept> {
    let text = std::fs::read_to_string(key_file()?).ok()?;
    let secret = ai_key::unlock(&text, &machine(false)?)?;
    Some(Kept {
        provider: unbound(&secret).0.map(str::to_owned),
    })
}

pub(super) const fn can_keep() -> bool {
    cfg!(unix)
}

pub(super) fn kept_for() -> Option<Provider> {
    read_kept()
        .and_then(|kept| kept.provider)
        .and_then(|name| Provider::by_name(&name))
}

impl AiState {
    fn the_kept_key_is_ours(&self) -> bool {
        read_kept().is_none_or(|kept| {
            kept.provider
                .is_none_or(|provider| provider == self.provider.name())
        })
    }

    pub(super) fn keep_the_key(&mut self) {
        let Some(file) = key_file() else {
            return;
        };
        if !self.remember_key {
            if self.the_kept_key_is_ours() {
                let _ = std::fs::remove_file(&file);
            }
            return;
        }
        if self.key.is_empty() {
            return;
        }
        let secret = bound(self.provider.name(), &self.key);
        let kept = match (machine(true), random(ai_key::SALT)) {
            (Some(machine), Some(salt)) => ai_key::lock(&secret, &machine, &salt)
                .is_some_and(|line| write_privately(&file, line.as_bytes())),
            _ => false,
        };
        if !kept {
            self.remember_key = false;
            if self.the_kept_key_is_ours() {
                let _ = std::fs::remove_file(&file);
            }
        }
    }

    pub(super) fn take_the_kept_key(&mut self) {
        let Some(text) = key_file().and_then(|file| std::fs::read_to_string(file).ok()) else {
            return;
        };
        let Some(secret) = machine(false).and_then(|machine| ai_key::unlock(&text, &machine))
        else {
            if ai_key::looks_like_ours(&text) {
                self.notice = Some(super::Notice::plain(
                    pdf_app::wording::Message::AiKeptKeyUnreadable,
                ));
            }
            return;
        };
        let (for_provider, key) = unbound(&secret);
        if for_provider.is_some_and(|provider| provider != self.provider.name()) {
            return;
        }
        key.clone_into(&mut self.key);
        self.remember_key = true;
        self.connected = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{bound, unbound};

    #[test]
    fn a_kept_key_is_tied_to_the_provider_it_was_kept_for() {
        let secret = bound("OpenAI", "sk-abc");
        assert_eq!(unbound(&secret), (Some("OpenAI"), "sk-abc"));
        assert_ne!(
            unbound(&bound("Claude", "sk-abc")).0,
            unbound(&secret).0,
            "the same key kept for another provider is not the same entry"
        );
    }

    #[test]
    fn a_key_kept_before_keys_were_tied_to_providers_still_reads() {
        assert_eq!(unbound("sk-old"), (None, "sk-old"));
    }

    #[test]
    fn a_key_with_odd_characters_survives_the_binding() {
        for key in ["a b", "ключ", "key=with:chars", "x\ty"] {
            assert_eq!(unbound(&bound("Gemini", key)), (Some("Gemini"), key));
        }
    }
}
