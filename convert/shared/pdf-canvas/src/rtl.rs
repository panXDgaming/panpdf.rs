use std::ops::Range;

use crate::bidi::{self, Class};
use crate::font::{Face, FontId, Glyph, Shaped, ShapedCluster, graphemes, is_invisible, is_space};
use crate::unicode_data::{FORMS, JOINING_TYPE, LAM_ALEF};

const TATWEEL: char = '\u{0640}';
const LAM: char = '\u{0644}';
const ZWJ: char = '\u{200D}';
const ZWNJ: char = '\u{200C}';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joining {
    U,
    C,
    D,
    L,
    R,
    T,
}

#[must_use]
pub fn joining(c: char) -> Joining {
    let code = u32::from(c);
    let at = JOINING_TYPE.partition_point(|&(first, _, _)| first <= code);
    if at > 0 {
        let (first, last, kind) = JOINING_TYPE[at - 1];
        if (first..=last).contains(&code) {
            return match kind {
                1 => Joining::C,
                2 => Joining::D,
                3 => Joining::L,
                _ => Joining::R,
            };
        }
    }
    match bidi::class(c) {
        Class::NSM | Class::BN if c != ZWNJ => Joining::T,
        _ => Joining::U,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Isolated,
    Final,
    Initial,
    Medial,
}

impl Form {
    const fn of(joins_before: bool, joins_after: bool) -> Self {
        match (joins_before, joins_after) {
            (false, false) => Self::Isolated,
            (true, false) => Self::Final,
            (false, true) => Self::Initial,
            (true, true) => Self::Medial,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Isolated => 0,
            Self::Final => 1,
            Self::Initial => 2,
            Self::Medial => 3,
        }
    }
}

#[must_use]
pub fn presentation_form(c: char, form: Form) -> Option<char> {
    let code = u32::from(c);
    let at = FORMS.binary_search_by_key(&code, |&(base, _)| base).ok()?;
    let found = FORMS[at].1[form.index()];
    if found == 0 {
        return None;
    }
    char::from_u32(found)
}

fn lam_alef(alef: char, form: Form) -> Option<char> {
    let code = u32::from(alef);
    let at = LAM_ALEF.binary_search_by_key(&code, |&(a, _)| a).ok()?;
    let index = usize::from(matches!(form, Form::Final | Form::Medial));
    char::from_u32(LAM_ALEF[at].1[index])
}

struct Unit {
    range: Range<usize>,
    bases: Vec<char>,
    joins_before: bool,
    joins_after: bool,
}

#[derive(Clone, Debug)]
struct Placed {
    gid: u16,
    x: i32,
    y: i32,
    advance: i32,
}

fn base_of(cluster: &str) -> Option<char> {
    cluster
        .chars()
        .find(|&c| joining(c) != Joining::T && !is_invisible(c))
}

fn units(word: &str) -> Vec<Unit> {
    let clusters = graphemes(word);
    let mut out: Vec<Unit> = Vec::new();
    let mut k = 0;
    while k < clusters.len() {
        let range = clusters[k].clone();
        let base = base_of(&word[range.clone()]);
        let next_base = clusters.get(k + 1).and_then(|r| base_of(&word[r.clone()]));
        let lam_with = base == Some(LAM)
            && next_base.is_some_and(|a| lam_alef(a, Form::Isolated).is_some())
            && !word[range.clone()].contains(ZWNJ);
        if let (true, Some(alef)) = (lam_with, next_base) {
            let end = clusters[k + 1].end;
            out.push(Unit {
                range: range.start..end,
                bases: vec![LAM, alef],
                joins_before: false,
                joins_after: false,
            });
            k += 2;
            continue;
        }
        out.push(Unit {
            range,
            bases: base.into_iter().collect(),
            joins_before: false,
            joins_after: false,
        });
        k += 1;
    }
    for k in 1..out.len() {
        let (earlier, later) = (&out[k - 1], &out[k]);
        let text = &word[earlier.range.clone()];
        let forward = text.contains(ZWJ)
            || (!text.contains(ZWNJ)
                && earlier
                    .bases
                    .last()
                    .is_some_and(|&c| matches!(joining(c), Joining::D | Joining::L | Joining::C)));
        let backward = later
            .bases
            .first()
            .is_some_and(|&c| matches!(joining(c), Joining::D | Joining::R | Joining::C));
        if forward && backward && earlier.bases.len() < 2 {
            out[k - 1].joins_after = true;
            out[k].joins_before = true;
        }
    }
    out
}

impl Face {
    fn gid(&self, c: char) -> Option<u16> {
        self.truetype().glyph_for_char(c).filter(|&g| g != 0)
    }

    fn engine(&self, text: &str) -> Option<Vec<pdf_font::shaping::ShapedGlyph>> {
        pdf_font::shaping::shape_cluster(&self.program, self.face_index, text)
    }

    fn carrier(&self) -> Option<u16> {
        if self.cff().is_some() {
            return None;
        }
        ['\u{200B}', ZWNJ, ZWJ, '\u{2060}', '\u{FEFF}', '\u{034F}']
            .into_iter()
            .filter_map(|c| self.gid(c))
            .find(|&g| {
                self.advance(g) == 0 && self.program.path(g).is_none_or(|p| p.segments.is_empty())
            })
    }

    fn shape_unit(&self, word: &str, unit: &Unit) -> Option<Vec<Placed>> {
        let visible: String = word[unit.range.clone()]
            .chars()
            .filter(|c| !is_invisible(*c))
            .collect();
        if visible.is_empty() {
            return Some(Vec::new());
        }
        let alone = self.engine(&visible)?;
        let form = Form::of(unit.joins_before, unit.joins_after);
        let mut glyphs = alone.clone();
        let mut contextual = form == Form::Isolated;
        if form != Form::Isolated && self.gid(TATWEEL).is_some() {
            let mut text = String::new();
            if unit.joins_before {
                text.push(TATWEEL);
            }
            text.push_str(&visible);
            if unit.joins_after {
                text.push(TATWEEL);
            }
            if let Some(mut shaped) = self.engine(&text) {
                let mut origin = 0;
                if unit.joins_after && !shaped.is_empty() {
                    origin = shaped[0].advance;
                    shaped.remove(0);
                }
                if unit.joins_before {
                    shaped.pop();
                }
                if !shaped.is_empty() {
                    for g in &mut shaped {
                        g.x -= origin;
                    }
                    let changed = shaped
                        .iter()
                        .map(|g| g.glyph)
                        .ne(alone.iter().map(|g| g.glyph));
                    contextual = changed;
                    glyphs = shaped;
                }
            }
        }
        let lam_alef_unjoined = unit.bases.len() == 2
            && [LAM, unit.bases[1]].iter().all(|&c| {
                self.gid(c)
                    .is_some_and(|g| glyphs.iter().any(|s| s.glyph == g))
            });
        if !contextual || lam_alef_unjoined {
            let mut text = String::new();
            let mut replaced = false;
            let mut chars = visible.chars().peekable();
            while let Some(c) = chars.next() {
                if unit.bases.len() == 2 && c == LAM {
                    if let Some(lig) = lam_alef(unit.bases[1], form)
                        && self.gid(lig).is_some()
                    {
                        text.push(lig);
                        replaced = true;
                        let mut rest = String::new();
                        for d in chars.by_ref() {
                            if d == unit.bases[1] {
                                break;
                            }
                            rest.push(d);
                        }
                        text.push_str(&rest);
                        continue;
                    }
                } else if unit.bases.first() == Some(&c)
                    && let Some(p) = presentation_form(c, form)
                    && self.gid(p).is_some()
                {
                    text.push(p);
                    replaced = true;
                    continue;
                }
                text.push(c);
            }
            if replaced && let Some(shaped) = self.engine(&text) {
                glyphs = shaped;
            }
        }
        let placed = glyphs
            .iter()
            .map(|g| Placed {
                gid: g.glyph,
                x: g.x,
                y: g.y,
                advance: g.advance,
            })
            .collect::<Vec<_>>();
        Some(placed)
    }

    pub(crate) fn shape_rtl(&self, id: FontId, word: &str) -> Shaped {
        let units = units(word);
        let mut shaped_units: Vec<Vec<Placed>> = Vec::with_capacity(units.len());
        for unit in &units {
            match self.shape_unit(word, unit) {
                Some(glyphs) => shaped_units.push(glyphs),
                None => {
                    let mut plain = self.shape(id, word);
                    plain.clusters.reverse();
                    return plain;
                }
            }
        }
        let mut placed: Vec<Placed> = Vec::new();
        let mut pen = 0;
        for unit in shaped_units.iter().rev() {
            for g in unit {
                let mut g = g.clone();
                g.x += pen;
                placed.push(g);
            }
            pen += unit.iter().map(|g| g.advance).sum::<i32>();
        }
        let visible: String = word.chars().filter(|c| !is_invisible(*c)).collect();
        if let Some(whole) = self.engine(&visible)
            && whole.len() == placed.len()
            && whole.iter().zip(&placed).all(|(w, g)| w.glyph == g.gid)
        {
            for (w, g) in whole.iter().zip(placed.iter_mut()) {
                g.x = w.x;
                g.y = w.y;
                g.advance = w.advance;
            }
        }
        let carrier = self.carrier();
        let mut clusters = Vec::with_capacity(placed.len() + word.len() / 2);
        let mut at = 0;
        let mut pen = 0;
        for (unit, glyphs) in units.iter().zip(&shaped_units).rev() {
            let placed_here = &placed[at..at + glyphs.len()];
            at += glyphs.len();
            let chars: Vec<Range<usize>> = word[unit.range.clone()]
                .char_indices()
                .map(|(k, c)| unit.range.start + k..unit.range.start + k + c.len_utf8())
                .collect();
            let principal = (0..placed_here.len())
                .max_by_key(|&k| (placed_here[k].advance, std::cmp::Reverse(k)))
                .unwrap_or(0);
            let (first, rest) = match (carrier, chars.split_first()) {
                (Some(_), Some((first, rest))) => (vec![first.clone()], rest.to_vec()),
                _ => (chars.clone(), Vec::new()),
            };
            if let Some(gid) = carrier {
                for r in rest.iter().rev() {
                    clusters.push(one(r.clone(), gid, 0, 0, 0, &word[r.clone()]));
                }
            }
            let mut order: Vec<usize> = vec![principal];
            order.extend((0..placed_here.len()).filter(|&k| k != principal));
            let mut unit_pen = pen;
            for (step, &k) in order.iter().enumerate() {
                let g = &placed_here[k];
                let meaning: String = if step == 0 {
                    first.iter().map(|r| &word[r.clone()]).collect()
                } else {
                    String::new()
                };
                let range = if step == 0 { span_of(&first) } else { 0..0 };
                let advance = if step == 0 {
                    placed_here.iter().map(|g| g.advance).sum()
                } else {
                    0
                };
                clusters.push(one(range, g.gid, g.x - unit_pen, g.y, advance, &meaning));
                if step == 0 {
                    unit_pen += advance;
                }
            }
            pen = unit_pen;
        }
        Shaped {
            font: id,
            text: word.to_owned(),
            units_per_em: self.metrics.units_per_em,
            clusters,
        }
    }
}

fn one(
    range: Range<usize>,
    gid: u16,
    x: i32,
    y: i32,
    advance: i32,
    meaning: &str,
) -> ShapedCluster {
    ShapedCluster {
        range,
        glyphs: vec![Glyph {
            gid,
            x,
            y,
            advance,
            meaning: meaning.to_owned(),
        }],
        advance,
        missing: false,
        space: false,
    }
}

fn span_of(ranges: &[Range<usize>]) -> Range<usize> {
    let start = ranges.iter().map(|r| r.start).min().unwrap_or(0);
    let end = ranges.iter().map(|r| r.end).max().unwrap_or(start);
    start..end
}

#[must_use]
pub fn is_rtl_word(text: &str) -> bool {
    text.chars().any(bidi::is_rtl) && !text.chars().any(is_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joining_types() {
        assert_eq!(joining('\u{0628}'), Joining::D);
        assert_eq!(joining('\u{0627}'), Joining::R);
        assert_eq!(joining('\u{064E}'), Joining::T);
        assert_eq!(joining(TATWEEL), Joining::C);
        assert_eq!(joining('a'), Joining::U);
        assert_eq!(joining('\u{05D0}'), Joining::U);
        assert_eq!(
            presentation_form('\u{0628}', Form::Medial),
            Some('\u{FE92}')
        );
        assert_eq!(lam_alef('\u{0627}', Form::Final), Some('\u{FEFC}'));
    }

    fn dejavu() -> (crate::Canvas, FontId) {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../fonts/packaged/DejaVuSans.ttf"
        );
        let mut canvas = crate::Canvas::new();
        let font = canvas
            .add_font(std::fs::read(path).expect("the package's DejaVu Sans"), 0)
            .expect("a face");
        (canvas, font)
    }

    fn read_back(shaped: &Shaped) -> String {
        shaped
            .clusters
            .iter()
            .rev()
            .flat_map(|c| c.glyphs.iter().map(|g| g.meaning.clone()))
            .collect()
    }

    #[test]
    fn arabic_joins_and_reads_back() {
        let (canvas, font) = dejavu();
        let word = "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}";
        let shaped = canvas.shape_rtl(font, word);
        assert_eq!(read_back(&shaped), word);
        assert_eq!(shaped.clusters.len(), 5);
        let fonts = canvas.fonts.borrow();
        let face = &fonts.faces[font.0];
        let isolated: Vec<u16> = word.chars().filter_map(|c| face.gid(c)).collect();
        let drawn: Vec<u16> = shaped.clusters.iter().map(|c| c.glyphs[0].gid).collect();
        assert!(
            drawn.iter().zip(isolated.iter().rev()).all(|(d, i)| d != i),
            "{drawn:?} vs {isolated:?}"
        );
    }

    #[test]
    fn lam_alef_is_one_glyph_read_as_two() {
        let (canvas, font) = dejavu();
        let word = "\u{0633}\u{0644}\u{0627}\u{0645}";
        let shaped = canvas.shape_rtl(font, word);
        assert_eq!(read_back(&shaped), word);
        let drawn = shaped.clusters.iter().filter(|c| c.advance > 0).count();
        assert_eq!(drawn, 3, "{shaped:?}");
    }

    #[test]
    fn hebrew_reads_back() {
        let (canvas, font) = dejavu();
        let word = "\u{05E9}\u{05DC}\u{05D5}\u{05DD}";
        let shaped = canvas.shape_rtl(font, word);
        assert_eq!(read_back(&shaped), word);
    }

    #[test]
    fn forms_from_neighbours() {
        let word = "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}";
        let forms: Vec<Form> = units(word)
            .iter()
            .map(|u| Form::of(u.joins_before, u.joins_after))
            .collect();
        assert_eq!(
            forms,
            [
                Form::Initial,
                Form::Final,
                Form::Initial,
                Form::Medial,
                Form::Final
            ]
        );
        let salam = "\u{0633}\u{0644}\u{0627}\u{0645}";
        let u = units(salam);
        assert_eq!(u.len(), 3);
        assert_eq!(u[1].bases, ['\u{0644}', '\u{0627}']);
        assert!(u[1].joins_before && !u[1].joins_after && !u[2].joins_before);
    }
}
