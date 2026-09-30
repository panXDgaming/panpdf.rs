use std::sync::Arc;

use crate::FontProvider;

#[derive(Clone, Debug, Default)]
pub struct Input {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Settings {
    pairs: Vec<(String, String)>,
}

impl Settings {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                settings.set(key.trim(), value.trim());
            }
        }
        settings
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.pairs.push((key.to_owned(), value.to_owned()));
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[must_use]
    pub fn all(&self, key: &str) -> Vec<&str> {
        self.pairs
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    #[must_use]
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }
}

#[derive(Clone, Debug, Default)]
pub struct Files {
    pub files: Vec<(String, Vec<u8>)>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct FontFile {
    pub family: String,
    pub bytes: Arc<[u8]>,
}

#[derive(Clone, Debug, Default)]
pub struct Fonts {
    pub provider: Option<Arc<dyn FontProvider>>,
    pub files: Vec<FontFile>,
}

impl Fonts {
    #[must_use]
    pub fn file(&self, family: &str) -> Option<&FontFile> {
        let key = |s: &str| -> String {
            s.chars()
                .filter(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_lowercase())
                .collect()
        };
        let wanted = key(family);
        self.files.iter().find(|file| key(&file.family) == wanted)
    }

    #[must_use]
    pub fn layout_provider(&self) -> Option<Arc<dyn FontProvider>> {
        if let Some(provider) = &self.provider {
            return Some(Arc::clone(provider));
        }
        let mut held = crate::held_fonts::HeldFonts::new();
        for file in &self.files {
            let _ = held.add(&file.family, file.bytes.to_vec());
        }
        (!held.is_empty()).then(|| Arc::new(held) as Arc<dyn FontProvider>)
    }
}

pub type Run =
    fn(&[Input], &Settings, &Fonts, &mut dyn FnMut(usize, usize) -> bool) -> Result<Files, String>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_keep_order_and_last_wins() {
        let settings = Settings::parse("size=A4\nmargin = 10\nsize=Letter\nnoise\n");
        assert_eq!(settings.get("size"), Some("Letter"));
        assert_eq!(settings.all("size"), vec!["A4", "Letter"]);
        assert_eq!(settings.get("margin"), Some("10"));
        assert_eq!(settings.get("noise"), None);
    }
}
