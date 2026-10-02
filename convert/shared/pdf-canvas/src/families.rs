use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use pdf_font::substitute::{
    FontFlags, FontProvider, FontRequest, FontStyle, ProgramEvidence, SubstitutedFace,
    SubstitutionReason,
};

use crate::font::{graphemes, is_invisible, is_space};
use crate::{Canvas, FontId, Shaped};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub font: FontId,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

#[must_use]
pub fn normalize(family: &str) -> String {
    family
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

#[must_use]
pub fn symbol_text(family: &str, text: &str) -> Option<String> {
    if normalize(family) != "symbol" {
        return None;
    }
    let table = pdf_font::standard14::Standard14::Symbol.built_in_encoding()?;
    let mut changed = false;
    let out: String = text
        .chars()
        .map(|c| {
            let code = match u32::from(c) {
                v @ 0x21..=0x7E => v,
                v @ 0xF021..=0xF0FE => v - 0xF000,
                _ => return c,
            };
            let found = table[code as usize]
                .and_then(|name| pdf_font::standard14::Standard14::Symbol.character_for_name(name));
            match found {
                Some(mapped) if mapped != c => {
                    changed = true;
                    mapped
                }
                _ => c,
            }
        })
        .collect();
    changed.then_some(out)
}

fn substitutes(family: &str) -> &'static [&'static str] {
    const SANS: &[&str] = &[
        "Liberation Sans",
        "Arimo",
        "Arial",
        "DejaVu Sans",
        "Noto Sans",
    ];
    const SERIF: &[&str] = &[
        "Liberation Serif",
        "Tinos",
        "Times New Roman",
        "DejaVu Serif",
        "Noto Serif",
    ];
    const MONO: &[&str] = &[
        "Liberation Mono",
        "Cousine",
        "Courier New",
        "DejaVu Sans Mono",
        "Noto Sans Mono",
    ];
    let key = normalize(family);
    match key.as_str() {
        "calibri" | "calibrilight" => &["Carlito", "Liberation Sans", "Arimo", "DejaVu Sans"],
        "cambria" | "cambriamath" => &["Caladea", "Liberation Serif", "Tinos", "DejaVu Serif"],
        k if k.contains("mono")
            || k.contains("courier")
            || k.contains("consolas")
            || k.contains("console") =>
        {
            MONO
        }
        k if k.contains("times")
            || k.contains("serif") && !k.contains("sans")
            || k.contains("georgia")
            || k.contains("garamond")
            || k.contains("bookantiqua")
            || k.contains("palatino")
            || k.contains("minion")
            || k.contains("roman") =>
        {
            SERIF
        }
        _ => SANS,
    }
}

fn request(family: &str, bold: bool, italic: bool) -> FontRequest {
    FontRequest {
        base_font: family.replace(' ', "").into_bytes(),
        family: family.to_owned(),
        style: FontStyle {
            weight: if bold { 700 } else { 400 },
            italic,
        },
        subtype: b"TrueType".to_vec(),
        cid_subtype: None,
        registry: None,
        ordering: None,
        encoding: None,
        flags: FontFlags(if italic { 32 | 64 } else { 32 }),
        italic_angle: None,
        ascent: None,
        descent: None,
        standard_face: None,
        program: ProgramEvidence::NotEmbedded,
    }
}

pub struct FontBook {
    provider: Option<Arc<dyn FontProvider>>,
    loaded: HashMap<(String, u32), Option<FontId>>,
    picks: HashMap<(String, bool, bool), Option<Pick>>,
    missing: usize,
    rtl_words: HashMap<(FontId, String), Rc<Shaped>>,
}

impl FontBook {
    #[must_use]
    pub fn new(provider: Option<Arc<dyn FontProvider>>) -> Self {
        Self {
            provider,
            loaded: HashMap::new(),
            picks: HashMap::new(),
            missing: 0,
            rtl_words: HashMap::new(),
        }
    }

    pub fn shape_rtl(&mut self, canvas: &Canvas, font: FontId, word: &str) -> Rc<Shaped> {
        if let Some(found) = self.rtl_words.get(&(font, word.to_owned())) {
            return Rc::clone(found);
        }
        let shaped = Rc::new(canvas.shape_rtl(font, word));
        self.rtl_words
            .insert((font, word.to_owned()), Rc::clone(&shaped));
        shaped
    }

    #[must_use]
    pub const fn missing(&self) -> usize {
        self.missing
    }

    fn load(&mut self, canvas: &mut Canvas, face: &SubstitutedFace) -> Option<FontId> {
        let key = (face.identity.sha256.clone(), face.identity.face_index);
        if let Some(found) = self.loaded.get(&key) {
            return *found;
        }
        let id = canvas
            .add_program(Arc::clone(&face.program), face.identity.face_index)
            .ok();
        self.loaded.insert(key, id);
        id
    }

    fn pick(
        &mut self,
        canvas: &mut Canvas,
        face: &SubstitutedFace,
        bold: bool,
        italic: bool,
    ) -> Option<Pick> {
        let font = self.load(canvas, face)?;
        let style = face.identity.style;
        Some(Pick {
            font,
            fake_bold: bold && !style.is_bold(),
            fake_italic: italic && !style.italic,
        })
    }

    pub fn face(
        &mut self,
        canvas: &mut Canvas,
        family: &str,
        bold: bool,
        italic: bool,
    ) -> Option<Pick> {
        let key = (normalize(family), bold, italic);
        if let Some(pick) = self.picks.get(&key) {
            return *pick;
        }
        let provider = self.provider.clone()?;
        let mut names: Vec<&str> = vec![family];
        names.extend(substitutes(family));
        let mut pick = None;
        let mut generic = None;
        for name in &names {
            let Some(face) = provider.primary_face(&request(name, bold, italic)) else {
                continue;
            };
            if matches!(
                face.reason,
                SubstitutionReason::ExactFamily | SubstitutionReason::AliasedFamily
            ) {
                pick = self.pick(canvas, &face, bold, italic);
                if pick.is_some() {
                    break;
                }
            } else if generic.is_none() {
                generic = Some(face);
            }
        }
        if pick.is_none()
            && let Some(face) = generic
        {
            pick = self.pick(canvas, &face, bold, italic);
        }
        if pick.is_none()
            && let Some(face) = provider.fallback_face(&request(family, bold, italic), 'a')
        {
            pick = self.pick(canvas, &face, bold, italic);
        }
        self.picks.insert(key, pick);
        pick
    }

    fn covering(
        &mut self,
        canvas: &mut Canvas,
        family: &str,
        cluster: &str,
        bold: bool,
        italic: bool,
    ) -> Option<Pick> {
        let provider = self.provider.clone()?;
        let first = cluster
            .chars()
            .find(|c| !is_space(*c) && !is_invisible(*c))?;
        let preferred: &[&str] = match first {
            '\u{0E80}'..='\u{0EFF}' => &["Phetsarath OT", "Saysettha OT", "Noto Sans Lao"],
            '\u{0E00}'..='\u{0E7F}' => &[
                "Tahoma",
                "Leelawadee UI",
                "TH Sarabun New",
                "Noto Sans Thai",
            ],
            _ => &[],
        };
        for name in preferred {
            if let Some(face) = provider.primary_face(&request(name, bold, italic))
                && normalize(&face.identity.family) == normalize(name)
                && !face.identity.subfamily.contains("Condensed")
                && face.program.glyph_for_char(first).is_some()
                && let Some(pick) = self.pick(canvas, &face, bold, italic)
            {
                return Some(pick);
            }
        }
        let face = provider.fallback_face(&request(family, bold, italic), first)?;
        self.pick(canvas, &face, bold, italic)
    }

    pub fn runs(
        &mut self,
        canvas: &mut Canvas,
        text: &str,
        family: &str,
        bold: bool,
        italic: bool,
    ) -> Vec<(Range<usize>, Pick)> {
        let Some(primary) = self.face(canvas, family, bold, italic) else {
            return Vec::new();
        };
        let mut out: Vec<(Range<usize>, Pick)> = Vec::new();
        let mut fallbacks: HashMap<char, Option<Pick>> = HashMap::new();
        for range in graphemes(text) {
            let cluster = &text[range.clone()];
            let neutral = cluster.chars().all(|c| is_space(c) || is_invisible(c));
            let pick = if neutral {
                out.last().map_or(primary, |(_, p)| *p)
            } else if covers(canvas, primary.font, cluster) {
                primary
            } else {
                let first = cluster.chars().next().unwrap_or(' ');
                let found = match fallbacks.get(&first) {
                    Some(found) => *found,
                    None => {
                        let found = self.covering(canvas, family, cluster, bold, italic);
                        fallbacks.insert(first, found);
                        found
                    }
                };
                match found {
                    Some(pick) if covers(canvas, pick.font, cluster) => pick,
                    _ => {
                        self.missing += cluster.chars().count();
                        primary
                    }
                }
            };
            match out.last_mut() {
                Some((last, p)) if *p == pick && last.end == range.start => last.end = range.end,
                _ => out.push((range, pick)),
            }
        }
        out
    }
}

fn covers(canvas: &Canvas, font: FontId, cluster: &str) -> bool {
    cluster
        .chars()
        .filter(|c| !is_space(*c) && !is_invisible(*c))
        .all(|c| canvas.has_char(font, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stand_ins() {
        assert_eq!(normalize("Times New Roman"), "timesnewroman");
        assert_eq!(substitutes("Calibri")[0], "Carlito");
        assert_eq!(substitutes("Times New Roman")[0], "Liberation Serif");
        assert_eq!(substitutes("Courier New")[0], "Liberation Mono");
        assert_eq!(substitutes("Phetsarath OT")[0], "Liberation Sans");
    }

    #[test]
    fn symbol_codes() {
        assert_eq!(symbol_text("Symbol", "\u{f061}+b").as_deref(), Some("α+β"));
        assert_eq!(symbol_text("Symbol", "\u{f0a5}").as_deref(), Some("∞"));
        assert_eq!(symbol_text("Arial", "\u{f061}"), None);
        assert_eq!(symbol_text("Symbol", "1 + 2"), None);
    }
}
