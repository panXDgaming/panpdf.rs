#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Script {
    Lao,
    Thai,
    Khmer,
    Myanmar,
    Latin,
    Cyrillic,
    Greek,
    Arabic,
    Hebrew,
    Devanagari,
    Cjk,
    Hangul,
    Common,
    Other,
}

impl Script {
    #[must_use]
    pub fn of(c: char) -> Self {
        match u32::from(c) {
            0x0E80..=0x0EFF => Self::Lao,
            0x0E00..=0x0E7F => Self::Thai,
            0x1780..=0x17FF | 0x19E0..=0x19FF => Self::Khmer,
            0x1000..=0x109F | 0xAA60..=0xAA7F => Self::Myanmar,
            0x0041..=0x005A
            | 0x0061..=0x007A
            | 0x00C0..=0x00D6
            | 0x00D8..=0x00F6
            | 0x00F8..=0x024F
            | 0x1E00..=0x1EFF => Self::Latin,
            0x0400..=0x052F => Self::Cyrillic,
            0x0370..=0x03FF => Self::Greek,
            0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Self::Arabic,
            0x0590..=0x05FF | 0xFB1D..=0xFB4F => Self::Hebrew,
            0x0900..=0x097F => Self::Devanagari,
            0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FFFF => Self::Cjk,
            0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F => Self::Hangul,
            _ if c.is_alphabetic() => Self::Other,
            _ => Self::Common,
        }
    }

    #[must_use]
    pub fn is_complex(self) -> bool {
        matches!(
            self,
            Self::Lao
                | Self::Thai
                | Self::Khmer
                | Self::Myanmar
                | Self::Arabic
                | Self::Hebrew
                | Self::Devanagari
        )
    }

    #[must_use]
    pub fn is_east_asian(self) -> bool {
        matches!(self, Self::Cjk | Self::Hangul)
    }

    #[must_use]
    pub fn is_right_to_left(self) -> bool {
        matches!(self, Self::Arabic | Self::Hebrew)
    }

    #[must_use]
    pub fn joins_words(self) -> bool {
        matches!(
            self,
            Self::Lao | Self::Thai | Self::Khmer | Self::Myanmar | Self::Cjk
        )
    }

    #[must_use]
    pub fn language(self) -> Option<&'static str> {
        Some(match self {
            Self::Lao => "lo-LA",
            Self::Thai => "th-TH",
            Self::Khmer => "km-KH",
            Self::Myanmar => "my-MM",
            Self::Latin => "en-US",
            Self::Cyrillic => "ru-RU",
            Self::Greek => "el-GR",
            Self::Arabic => "ar-SA",
            Self::Hebrew => "he-IL",
            Self::Devanagari => "hi-IN",
            Self::Cjk => "zh-CN",
            Self::Hangul => "ko-KR",
            Self::Common | Self::Other => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Lao => "lao",
            Self::Thai => "thai",
            Self::Khmer => "khmer",
            Self::Myanmar => "myanmar",
            Self::Latin => "latin",
            Self::Cyrillic => "cyrillic",
            Self::Greek => "greek",
            Self::Arabic => "arabic",
            Self::Hebrew => "hebrew",
            Self::Devanagari => "devanagari",
            Self::Cjk => "cjk",
            Self::Hangul => "hangul",
            Self::Common => "common",
            Self::Other => "other",
        }
    }
}

#[must_use]
pub fn dominant(text: &str) -> Script {
    let mut counts: Vec<(Script, usize)> = Vec::new();
    for c in text.chars() {
        let script = Script::of(c);
        if script == Script::Common {
            continue;
        }
        match counts.iter_mut().find(|(s, _)| *s == script) {
            Some(entry) => entry.1 += 1,
            None => counts.push((script, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|(script, count)| (*count, std::cmp::Reverse(*script)))
        .map_or(Script::Common, |(script, _)| script)
}

#[must_use]
pub fn split(text: &str) -> Vec<(Script, &str)> {
    let mut pieces: Vec<(Script, usize, usize)> = Vec::new();
    for (at, c) in text.char_indices() {
        let script = Script::of(c);
        let end = at + c.len_utf8();
        match pieces.last_mut() {
            Some(last) if script == Script::Common || last.0 == script => last.2 = end,
            Some(last) if last.0 == Script::Common => {
                last.0 = script;
                last.2 = end;
            }
            _ => pieces.push((script, at, end)),
        }
    }
    pieces
        .into_iter()
        .map(|(script, start, end)| (script, &text[start..end]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies() {
        assert_eq!(Script::of('ກ'), Script::Lao);
        assert_eq!(Script::of('໌'), Script::Lao);
        assert_eq!(Script::of('ก'), Script::Thai);
        assert_eq!(Script::of('é'), Script::Latin);
        assert_eq!(Script::of('5'), Script::Common);
        assert!(Script::Lao.is_complex() && Script::Lao.joins_words());
        assert!(!Script::Lao.is_right_to_left());
    }

    #[test]
    fn splits_by_script_with_common_joining() {
        let pieces = split("ບົດທີ 1 Lesson 2 บทที่");
        let scripts: Vec<Script> = pieces.iter().map(|(s, _)| *s).collect();
        assert_eq!(scripts, vec![Script::Lao, Script::Latin, Script::Thai]);
        assert_eq!(
            pieces.iter().map(|(_, t)| *t).collect::<String>(),
            "ບົດທີ 1 Lesson 2 บทที่"
        );
        assert_eq!(dominant("ab ກຂຄ"), Script::Lao);
        assert_eq!(dominant("12 !"), Script::Common);
    }
}
