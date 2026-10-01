use std::ops::Range;

use pdf_font::glyph::GlyphProgram;
use pdf_font::truetype::TrueTypeFont;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontId(pub(crate) usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontError {
    Unreadable(String),
    NotTrueType,
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(why) => write!(f, "font not readable: {why}"),
            Self::NotTrueType => f.write_str("font has neither TrueType nor OpenType CFF outlines"),
        }
    }
}

impl std::error::Error for FontError {}

#[derive(Clone, Debug, PartialEq)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascent: i32,
    pub descent: i32,
    pub line_gap: i32,
    pub win_ascent: i32,
    pub win_descent: i32,
    pub cap_height: i32,
    pub x_height: i32,
    pub bbox: [i32; 4],
    pub italic_angle: f64,
    pub underline_position: i32,
    pub underline_thickness: i32,
    pub family: String,
    pub subfamily: String,
    pub postscript_name: String,
    pub bold: bool,
    pub italic: bool,
}

impl FontMetrics {
    #[must_use]
    pub fn pt(&self, units: i32, size: f32) -> f32 {
        units as f32 * size / f32::from(self.units_per_em.max(1))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub gid: u16,
    pub x: i32,
    pub y: i32,
    pub advance: i32,
    pub meaning: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapedCluster {
    pub range: Range<usize>,
    pub glyphs: Vec<Glyph>,
    pub advance: i32,
    pub missing: bool,
    pub space: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shaped {
    pub font: FontId,
    pub text: String,
    pub units_per_em: u16,
    pub clusters: Vec<ShapedCluster>,
}

impl Shaped {
    #[must_use]
    pub fn width(&self, range: Range<usize>, size: f32) -> f32 {
        let units: i64 = self.clusters[range]
            .iter()
            .map(|c| i64::from(c.advance))
            .sum();
        units as f32 * size / f32::from(self.units_per_em.max(1))
    }

    #[must_use]
    pub fn total_width(&self, size: f32) -> f32 {
        self.width(0..self.clusters.len(), size)
    }

    #[must_use]
    pub fn any_missing(&self) -> bool {
        self.clusters.iter().any(|c| c.missing)
    }
}

pub(crate) struct Face {
    pub program: std::sync::Arc<GlyphProgram>,
    pub face_index: u32,
    pub metrics: FontMetrics,
}

impl Face {
    pub(crate) fn truetype(&self) -> &TrueTypeFont {
        match self.program.as_ref() {
            GlyphProgram::TrueType(font) => font,
            GlyphProgram::Cff {
                sfnt: Some(font), ..
            } => font,
            _ => unreachable!("only TrueType and OpenType CFF faces are admitted"),
        }
    }

    pub(crate) fn cff(&self) -> Option<(&pdf_font::cff::CffFont, &[u8])> {
        match self.program.as_ref() {
            GlyphProgram::Cff { font, data, .. } => Some((font, data)),
            _ => None,
        }
    }

    pub(crate) fn fixed_code(&self, gid: u16) -> Option<u16> {
        let (font, _) = self.cff()?;
        Some(font.cid_for_glyph(gid).unwrap_or(gid))
    }

    pub(crate) fn parse(bytes: Vec<u8>, face_index: u32) -> Result<Self, FontError> {
        let program = GlyphProgram::parse_face(bytes, face_index)
            .map_err(|error| FontError::Unreadable(format!("{error:?}")))?;
        Self::from_program(std::sync::Arc::new(program), face_index)
    }

    pub(crate) fn from_program(
        program: std::sync::Arc<GlyphProgram>,
        face_index: u32,
    ) -> Result<Self, FontError> {
        let font = match program.as_ref() {
            GlyphProgram::TrueType(font) => font,
            GlyphProgram::Cff {
                sfnt: Some(font), ..
            } => font.as_ref(),
            _ => return Err(FontError::NotTrueType),
        };
        let metrics = read_metrics(font, face_index);
        Ok(Self {
            program,
            face_index,
            metrics,
        })
    }

    pub(crate) fn has_char(&self, c: char) -> bool {
        self.truetype().glyph_for_char(c).is_some_and(|g| g != 0)
    }

    pub(crate) fn advance(&self, gid: u16) -> i32 {
        i32::from(self.truetype().advance_width(gid).unwrap_or(0))
    }

    pub(crate) fn shape(&self, id: FontId, text: &str) -> Shaped {
        let clusters = graphemes(text)
            .into_iter()
            .map(|range| self.shape_cluster(text, range))
            .collect();
        Shaped {
            font: id,
            text: text.to_owned(),
            units_per_em: self.metrics.units_per_em,
            clusters,
        }
    }

    fn shape_cluster(&self, text: &str, range: Range<usize>) -> ShapedCluster {
        let cluster = &text[range.clone()];
        let font = self.truetype();
        let first = cluster.chars().next().unwrap_or(' ');
        if cluster.chars().all(is_space) {
            let gid = font
                .glyph_for_char(' ')
                .or_else(|| font.glyph_for_char('\u{a0}'))
                .unwrap_or(0);
            let one = if gid == 0 {
                i32::from(self.metrics.units_per_em) / 4
            } else {
                self.advance(gid)
            };
            let advance = if first == '\t' { one * 4 } else { one };
            return ShapedCluster {
                range,
                glyphs: vec![Glyph {
                    gid,
                    x: 0,
                    y: 0,
                    advance,
                    meaning: cluster.to_owned(),
                }],
                advance,
                missing: false,
                space: true,
            };
        }
        if cluster.chars().all(is_invisible) {
            return ShapedCluster {
                range,
                glyphs: Vec::new(),
                advance: 0,
                missing: false,
                space: false,
            };
        }
        let visible: String = cluster.chars().filter(|c| !is_invisible(*c)).collect();
        let missing = visible.chars().any(|c| !self.has_char(c));
        if !missing
            && let Some(shaped) =
                pdf_font::shaping::shape_cluster(&self.program, self.face_index, &visible)
            && shaped.iter().all(|g| g.glyph != 0)
        {
            let mut glyphs: Vec<Glyph> = shaped
                .iter()
                .map(|g| Glyph {
                    gid: g.glyph,
                    x: g.x,
                    y: g.y,
                    advance: g.advance,
                    meaning: String::new(),
                })
                .collect();
            let advance = glyphs.iter().map(|g| g.advance).sum();
            assign_meanings(font, &mut glyphs, cluster);
            return ShapedCluster {
                range,
                glyphs,
                advance,
                missing: false,
                space: false,
            };
        }
        let mut glyphs = Vec::new();
        let mut pen = 0;
        for c in visible.chars() {
            let gid = font.glyph_for_char(c).unwrap_or(0);
            let advance = self.advance(gid);
            glyphs.push(Glyph {
                gid,
                x: pen,
                y: 0,
                advance,
                meaning: c.to_string(),
            });
            pen += advance;
        }
        if let Some(first) = glyphs.first_mut()
            && visible.len() != cluster.len()
            && !missing
        {
            first.meaning = cluster.to_owned();
            for other in &mut glyphs[1..] {
                other.meaning.clear();
            }
        }
        ShapedCluster {
            range,
            advance: pen,
            glyphs,
            missing,
            space: false,
        }
    }
}

fn assign_meanings(font: &TrueTypeFont, glyphs: &mut [Glyph], cluster: &str) {
    if glyphs.len() == 1 {
        glyphs[0].meaning = cluster.to_owned();
        return;
    }
    let mut left = String::new();
    let reach = |glyphs: &[Glyph], c: char| {
        let gid = font.glyph_for_char(c)?;
        glyphs
            .iter()
            .position(|g| g.gid == gid && g.meaning.is_empty())
    };
    for c in cluster.chars() {
        if let Some(at) = reach(glyphs, c) {
            glyphs[at].meaning.push(c);
            continue;
        }
        let parts = match c {
            '\u{0E33}' => Some(['\u{0E4D}', '\u{0E32}']),
            '\u{0EB3}' => Some(['\u{0ECD}', '\u{0EB2}']),
            _ => None,
        };
        if let Some([one, two]) = parts
            && let Some(a) = reach(glyphs, one)
        {
            glyphs[a].meaning.push(one);
            glyphs[a].meaning.pop();
            glyphs[a].meaning.push(c);
            if let Some(b) = reach(glyphs, two) {
                glyphs[b].meaning.push('\u{0}');
            }
            continue;
        }
        left.push(c);
    }
    for glyph in glyphs.iter_mut() {
        if glyph.meaning == "\u{0}" {
            glyph.meaning.clear();
        }
    }
    if !left.is_empty() {
        let at = glyphs
            .iter()
            .position(|g| g.meaning.is_empty())
            .unwrap_or(0);
        glyphs[at].meaning.push_str(&left);
    }
}

#[must_use]
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\u{a0}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

#[must_use]
pub fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}' | '\u{AD}' | '\n' | '\r' | '\u{0}'..='\u{8}' | '\u{B}'..='\u{1F}'
    )
}

#[must_use]
pub fn extends(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
        | '\u{0483}'..='\u{0489}'
        | '\u{0591}'..='\u{05BD}' | '\u{05BF}' | '\u{05C1}'..='\u{05C2}' | '\u{05C4}'..='\u{05C5}' | '\u{05C7}'
        | '\u{0610}'..='\u{061A}' | '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{06D6}'..='\u{06DC}' | '\u{06DF}'..='\u{06E4}' | '\u{06E7}'..='\u{06E8}' | '\u{06EA}'..='\u{06ED}'
        | '\u{0900}'..='\u{0903}' | '\u{093A}'..='\u{093C}' | '\u{093E}'..='\u{094F}' | '\u{0951}'..='\u{0957}' | '\u{0962}'..='\u{0963}'
        | '\u{0981}'..='\u{0983}' | '\u{09BC}' | '\u{09BE}'..='\u{09CD}' | '\u{09D7}' | '\u{09E2}'..='\u{09E3}'
        | '\u{0E31}' | '\u{0E33}'..='\u{0E3A}' | '\u{0E47}'..='\u{0E4E}'
        | '\u{0EB1}' | '\u{0EB3}'..='\u{0EBC}' | '\u{0EC8}'..='\u{0ECE}'
        | '\u{0F18}'..='\u{0F19}' | '\u{0F35}' | '\u{0F37}' | '\u{0F39}' | '\u{0F71}'..='\u{0F84}' | '\u{0F86}'..='\u{0F87}' | '\u{0F8D}'..='\u{0FBC}'
        | '\u{102B}'..='\u{103E}' | '\u{1056}'..='\u{1059}' | '\u{105E}'..='\u{1060}' | '\u{1062}'..='\u{1064}' | '\u{1067}'..='\u{106D}' | '\u{1071}'..='\u{1074}' | '\u{1082}'..='\u{108D}'
        | '\u{17B4}'..='\u{17D3}' | '\u{17DD}'
        | '\u{1AB0}'..='\u{1AFF}' | '\u{1DC0}'..='\u{1DFF}' | '\u{200C}'..='\u{200D}' | '\u{20D0}'..='\u{20FF}'
        | '\u{FE00}'..='\u{FE0F}' | '\u{FE20}'..='\u{FE2F}'
        | '\u{1F3FB}'..='\u{1F3FF}' | '\u{E0100}'..='\u{E01EF}'
    )
}

fn joins_next(c: char) -> bool {
    matches!(
        c,
        '\u{094D}' | '\u{09CD}' | '\u{17D2}' | '\u{1039}' | '\u{0F84}' | '\u{200D}'
    )
}

#[must_use]
pub fn graphemes(text: &str) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut previous: Option<char> = None;
    for (at, c) in text.char_indices() {
        let end = at + c.len_utf8();
        let join = match (previous, out.last_mut()) {
            (Some(p), Some(_)) => {
                (extends(c) && !is_space(p) && !is_invisible(p))
                    || joins_next(p)
                    || (p == '\r' && c == '\n')
            }
            _ => false,
        };
        if join && let Some(last) = out.last_mut() {
            last.end = end;
        } else {
            out.push(at..end);
        }
        previous = Some(c);
    }
    out
}

fn be_i16(data: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn sfnt_table<'a>(data: &'a [u8], index: u32, tag: &[u8; 4]) -> Option<&'a [u8]> {
    let mut start = 0_usize;
    if data.get(0..4) == Some(b"ttcf") {
        start = be_u32(data, 12 + 4 * index as usize)? as usize;
    }
    let count = u16::from_be_bytes(data.get(start + 4..start + 6)?.try_into().ok()?);
    for n in 0..usize::from(count) {
        let record = start + 12 + n * 16;
        if data.get(record..record + 4)? == tag {
            let offset = be_u32(data, record + 8)? as usize;
            let length = be_u32(data, record + 12)? as usize;
            return data.get(offset..offset + length);
        }
    }
    None
}

fn read_metrics(font: &TrueTypeFont, index: u32) -> FontMetrics {
    let upem = font.units_per_em().max(16);
    let em = f64::from(upem);
    let data = font.program_bytes();
    let (ascent, descent) = font
        .line_metrics()
        .map_or((0.8 * em, -0.2 * em), |(a, d)| (a * em, d * em));
    let hhea = sfnt_table(data, index, b"hhea");
    let os2 = sfnt_table(data, index, b"OS/2");
    let post = sfnt_table(data, index, b"post");
    let line_gap = hhea.and_then(|t| be_i16(t, 8)).map_or(0, i32::from);
    let be_u16 = |t: &[u8], at: usize| -> Option<i32> {
        Some(i32::from(u16::from_be_bytes(
            t.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let win_ascent = os2.and_then(|t| be_u16(t, 74)).unwrap_or(0);
    let win_descent = os2.and_then(|t| be_u16(t, 76)).unwrap_or(0);
    let x_height = os2
        .filter(|t| t.len() >= 90)
        .and_then(|t| be_i16(t, 86))
        .map_or((em * 0.5) as i32, i32::from);
    let cap_height = font.cap_height().map_or((em * 0.7) as i32, i32::from);
    let bbox = font
        .font_box()
        .map_or([0, descent as i32, upem.into(), ascent as i32], |b| {
            b.map(i32::from)
        });
    let (underline_position, underline_thickness) = post
        .and_then(|t| Some((be_i16(t, 8)?, be_i16(t, 10)?)))
        .map_or((-(em * 0.1) as i32, (em * 0.05) as i32), |(p, t)| {
            (i32::from(p), i32::from(t).max(1))
        });
    let (family, subfamily) = font.names();
    let style = font.os2_style();
    let subfamily = subfamily.unwrap_or_default();
    let lower = subfamily.to_ascii_lowercase();
    let bold = style.map_or(lower.contains("bold"), |(weight, _, bold)| {
        bold || weight >= 600
    });
    let italic = style.map_or(
        lower.contains("italic") || lower.contains("oblique"),
        |(_, italic, _)| italic,
    );
    FontMetrics {
        units_per_em: upem,
        ascent: ascent.round() as i32,
        descent: descent.round() as i32,
        line_gap,
        win_ascent,
        win_descent,
        cap_height,
        x_height,
        bbox,
        italic_angle: font.italic_angle().unwrap_or(0.0),
        underline_position,
        underline_thickness,
        family: family.unwrap_or_default(),
        subfamily,
        postscript_name: font.postscript_name().unwrap_or_default(),
        bold,
        italic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_keep_marks_on_their_letter() {
        let text = "ນ້ຳໃຈ ที่นี่ e\u{301}";
        let parts: Vec<&str> = graphemes(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(parts, ["ນ້ຳ", "ໃ", "ຈ", " ", "ที่", "นี่", " ", "e\u{301}"]);
        assert_eq!(graphemes("a\r\nb").len(), 3);
        assert_eq!(graphemes("").len(), 0);
    }
}
