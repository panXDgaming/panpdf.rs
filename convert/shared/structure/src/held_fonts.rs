use std::sync::Arc;

use pdf_content::{
    FaceIdentity, FontProvider, FontRequest, FontStyle, GlyphProgram, SubstitutedFace,
    SubstitutionReason,
};

#[derive(Clone, Debug)]
struct Held {
    handed: String,
    key: String,
    named: String,
    bytes: Arc<[u8]>,
    program: Arc<GlyphProgram>,
    identity: Arc<FaceIdentity>,
}

#[derive(Clone, Debug, Default)]
pub struct HeldFonts {
    faces: Vec<Held>,
}

fn key_of(family: &str) -> String {
    family
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

impl HeldFonts {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, family: &str, bytes: Vec<u8>) -> Result<(), String> {
        let sha256 = pdf_content::sha256_hex(&bytes);
        let kept: Arc<[u8]> = Arc::from(bytes.as_slice());
        let tables = pdf_content::TrueTypeFont::parse_face(bytes.clone(), 0).ok();
        let (named, subfamily) = tables.as_ref().map_or((None, None), |font| font.names());
        let named = named.filter(|name| !key_of(name).is_empty());
        let style = tables
            .as_ref()
            .and_then(pdf_content::TrueTypeFont::os2_style)
            .map_or_else(FontStyle::default, |(weight, italic, bold)| FontStyle {
                weight: if weight == 0 {
                    if bold { 700 } else { 400 }
                } else {
                    weight.clamp(100, 900)
                },
                italic,
            });
        let program =
            GlyphProgram::parse_face(bytes, 0).map_err(|error| format!("{family}: {error:?}"))?;
        self.faces.push(Held {
            handed: family.to_owned(),
            key: key_of(family),
            named: named.as_deref().map(key_of).unwrap_or_default(),
            bytes: kept,
            program: Arc::new(program),
            identity: Arc::new(FaceIdentity {
                family: named.unwrap_or_else(|| family.to_owned()),
                subfamily: subfamily.unwrap_or_else(|| "Regular".to_owned()),
                origin: format!("held:{family}"),
                sha256,
                face_index: 0,
                style,
            }),
        });
        Ok(())
    }

    #[must_use]
    pub fn files(&self) -> Vec<crate::bytes_tool::FontFile> {
        self.faces
            .iter()
            .map(|held| crate::bytes_tool::FontFile {
                family: held.handed.clone(),
                bytes: Arc::clone(&held.bytes),
            })
            .collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    fn face(held: &Held, reason: SubstitutionReason) -> SubstitutedFace {
        SubstitutedFace {
            program: Arc::clone(&held.program),
            identity: Arc::clone(&held.identity),
            reason,
        }
    }
}

impl FontProvider for HeldFonts {
    fn primary_face(&self, request: &FontRequest) -> Option<SubstitutedFace> {
        let wanted = key_of(&request.family);
        if wanted.is_empty() {
            return None;
        }
        self.faces
            .iter()
            .filter(|held| held.key == wanted || held.named == wanted)
            .min_by_key(|held| held.identity.style.distance(request.style))
            .map(|held| Self::face(held, SubstitutionReason::ExactFamily))
    }

    fn fallback_face(&self, request: &FontRequest, character: char) -> Option<SubstitutedFace> {
        self.faces
            .iter()
            .filter(|held| held.program.glyph_for_char(character).is_some())
            .min_by_key(|held| held.identity.style.distance(request.style))
            .map(|held| Self::face(held, SubstitutionReason::ScriptCoverage))
    }

    fn description(&self) -> String {
        let names: Vec<String> = self
            .faces
            .iter()
            .map(|held| format!("{} {}", held.identity.family, held.identity.subfamily))
            .collect();
        format!("faces held in memory: {}", names.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_match_without_spaces_or_case() {
        assert_eq!(key_of("Noto Sans Lao"), key_of("NotoSansLao"));
        assert_ne!(key_of("DejaVu Sans"), key_of("DejaVu Serif"));
    }

    #[test]
    fn bold_and_italic_answer_by_their_own_faces() {
        let packaged = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../fonts/packaged");
        let mut fonts = HeldFonts::new();
        for stem in [
            "LiberationSans-Regular",
            "LiberationSans-Bold",
            "LiberationSans-Italic",
            "LiberationSans-BoldItalic",
        ] {
            let Ok(bytes) = std::fs::read(format!("{packaged}/{stem}.ttf")) else {
                return;
            };
            fonts.add(stem, bytes).unwrap();
        }
        let ask = |weight, italic| {
            let request = FontRequest::for_family("Liberation Sans", FontStyle { weight, italic });
            let face = fonts.primary_face(&request).unwrap();
            assert_eq!(face.identity.family, "Liberation Sans");
            face.identity.origin.trim_start_matches("held:").to_owned()
        };
        assert_eq!(ask(400, false), "LiberationSans-Regular");
        assert_eq!(ask(700, false), "LiberationSans-Bold");
        assert_eq!(ask(400, true), "LiberationSans-Italic");
        assert_eq!(ask(700, true), "LiberationSans-BoldItalic");
        let bold_a = fonts
            .fallback_face(
                &FontRequest::for_family(
                    "Nothing",
                    FontStyle {
                        weight: 700,
                        italic: false,
                    },
                ),
                'a',
            )
            .unwrap();
        assert_eq!(bold_a.identity.origin, "held:LiberationSans-Bold");
        assert_eq!(fonts.files()[1].family, "LiberationSans-Bold");
    }

    #[test]
    fn a_non_font_is_refused() {
        let mut fonts = HeldFonts::new();
        assert!(fonts.add("x", b"not a font".to_vec()).is_err());
        assert!(fonts.is_empty());
    }
}
