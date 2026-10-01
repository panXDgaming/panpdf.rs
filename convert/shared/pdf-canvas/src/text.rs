use std::ops::Range;
use std::rc::Rc;

use crate::bidi;
use crate::families::{FontBook, Pick};
use crate::page::{Page, Rgb, TextStyle};
use crate::{Canvas, Shaped, ShapedCluster};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
    Auto,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Rgb,
}

impl Span {
    #[must_use]
    pub fn new(text: &str, family: &str, size: f32) -> Self {
        Self {
            text: text.to_owned(),
            family: family.to_owned(),
            size,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: Rgb::BLACK,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Piece {
    pub shaped: Rc<Shaped>,
    pub clusters: Range<usize>,
    pub style: TextStyle,
    pub underline: bool,
    pub strike: bool,
    pub x: f32,
    pub width: f32,
    pub underline_y: f32,
    pub underline_thickness: f32,
    pub x_height: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Line {
    pub pieces: Vec<Piece>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub gap: f32,
    pub spaces: usize,
    pub last: bool,
    pub rtl: bool,
    pub said: Option<String>,
}

impl Line {
    #[must_use]
    pub fn height(&self) -> f32 {
        self.ascent + self.descent + self.gap
    }

    #[must_use]
    pub fn text(&self) -> String {
        if let Some(said) = &self.said {
            return said.clone();
        }
        let mut out = String::new();
        for piece in &self.pieces {
            let clusters = &piece.shaped.clusters[piece.clusters.clone()];
            if let (Some(first), Some(last)) = (clusters.first(), clusters.last()) {
                out.push_str(&piece.shaped.text[first.range.start..last.range.end]);
            }
        }
        out
    }

    pub fn draw(&self, page: &mut Page, x: f32, y: f32, word_spacing: f32) {
        let mut shift = 0.0;
        for piece in &self.pieces {
            let start = x + piece.x + shift;
            let spaces = piece.shaped.clusters[piece.clusters.clone()]
                .iter()
                .filter(|c| c.space)
                .count();
            let drawn = page.show(
                &piece.shaped,
                piece.clusters.clone(),
                &piece.style,
                start,
                y,
                word_spacing,
            );
            if piece.underline {
                let uy = y + piece.underline_y;
                page.stroke_line(
                    start,
                    uy,
                    start + drawn,
                    uy,
                    piece.underline_thickness,
                    piece.style.color,
                );
            }
            if piece.strike {
                let sy = y + piece.x_height * 0.5;
                page.stroke_line(
                    start,
                    sy,
                    start + drawn,
                    sy,
                    piece.underline_thickness,
                    piece.style.color,
                );
            }
            shift += spaces as f32 * word_spacing;
        }
    }
}

fn lao_or_thai(c: char) -> bool {
    matches!(c, '\u{0E01}'..='\u{0E7F}' | '\u{0E80}'..='\u{0EFF}')
}

fn leading_vowel(c: char) -> bool {
    matches!(c, '\u{0E40}'..='\u{0E44}' | '\u{0EC0}'..='\u{0EC4}')
}

fn following_vowel(c: char) -> bool {
    matches!(
        c,
        '\u{0E30}' | '\u{0E32}' | '\u{0E33}' | '\u{0E45}' | '\u{0EB0}' | '\u{0EB2}' | '\u{0EB3}'
    )
}

fn consonant(c: char) -> bool {
    matches!(c, '\u{0E01}'..='\u{0E2E}' | '\u{0E81}'..='\u{0EAE}' | '\u{0EDC}'..='\u{0EDF}')
}

fn cjk(c: char) -> bool {
    matches!(c, '\u{2E80}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}' | '\u{F900}'..='\u{FAFF}' | '\u{FF00}'..='\u{FFEF}' | '\u{20000}'..='\u{3FFFF}')
}

fn may_break(before: &str, after: &str, next: Option<&str>) -> bool {
    let (Some(b), Some(a)) = (before.chars().next(), after.chars().next()) else {
        return false;
    };
    if matches!(
        before.chars().last(),
        Some('-' | '/' | '\u{2013}' | '\u{2014}')
    ) && !a.is_ascii_digit()
    {
        return true;
    }
    if cjk(a) || cjk(b) {
        return !matches!(a, '。' | '、' | '，' | '）' | '」' | '』');
    }
    if lao_or_thai(a) && lao_or_thai(b) {
        if leading_vowel(b) {
            return false;
        }
        if leading_vowel(a) {
            return true;
        }
        if consonant(a) {
            let marked = after.chars().count() > 1;
            let vowel_next = next
                .and_then(|n| n.chars().next())
                .is_some_and(following_vowel);
            return (marked || vowel_next) && !following_vowel(b) || leading_vowel(b);
        }
    }
    false
}

struct Item {
    run: usize,
    cluster: usize,
    width: f32,
    space: bool,
    newline: bool,
    break_before: bool,
    at: usize,
    end: usize,
}

struct Run {
    shaped: Rc<Shaped>,
    span: usize,
    offset: usize,
    pick: Pick,
    ascent: f32,
    descent: f32,
    gap: f32,
    underline_y: f32,
    underline_thickness: f32,
    x_height: f32,
}

pub fn layout(
    canvas: &mut Canvas,
    book: &mut FontBook,
    spans: &[Span],
    width: Option<f32>,
) -> Vec<Line> {
    layout_directed(canvas, book, spans, width, Direction::Ltr)
}

#[allow(clippy::too_many_lines)]
pub fn layout_directed(
    canvas: &mut Canvas,
    book: &mut FontBook,
    spans: &[Span],
    width: Option<f32>,
    direction: Direction,
) -> Vec<Line> {
    let mut runs: Vec<Run> = Vec::new();
    let mut span_start = 0;
    for (index, span) in spans.iter().enumerate() {
        let here = span_start;
        span_start += span.text.len();
        if span.text.is_empty() {
            continue;
        }
        for (range, pick) in book.runs(canvas, &span.text, &span.family, span.bold, span.italic) {
            let offset = here + range.start;
            let shaped = canvas.shape(pick.font, &span.text[range]);
            let m = canvas.font_metrics(pick.font);
            let s = span.size;
            runs.push(Run {
                shaped: Rc::new(shaped),
                span: index,
                offset,
                pick,
                ascent: m.pt(m.ascent.max(m.win_ascent), s),
                descent: m.pt((-m.descent).max(m.win_descent), s),
                gap: if m.win_ascent + m.win_descent >= m.ascent - m.descent + m.line_gap {
                    0.0
                } else {
                    m.pt(m.line_gap, s)
                },
                underline_y: m.pt(m.underline_position, s),
                underline_thickness: m.pt(m.underline_thickness, s).max(0.5),
                x_height: m.pt(m.x_height, s),
            });
        }
    }
    let mut items: Vec<Item> = Vec::new();
    let mut previous_text: Option<String> = None;
    for (r, run) in runs.iter().enumerate() {
        let size = spans[run.span].size;
        let clusters = &run.shaped.clusters;
        for (c, cluster) in clusters.iter().enumerate() {
            let text = &run.shaped.text[cluster.range.clone()];
            let newline = text.contains('\n');
            let next = clusters
                .get(c + 1)
                .map(|n| &run.shaped.text[n.range.clone()]);
            let break_before = match (&previous_text, items.last()) {
                (Some(before), Some(last)) => {
                    (last.space && !cluster.space) || may_break(before, text, next)
                }
                _ => false,
            };
            items.push(Item {
                run: r,
                cluster: c,
                width: run.shaped.width(c..c + 1, size),
                space: cluster.space,
                newline,
                break_before,
                at: run.offset + cluster.range.start,
                end: run.offset + cluster.range.end,
            });
            previous_text = Some(text.to_owned());
        }
    }
    let levels = Levels::of(spans, direction);
    if levels.is_some() {
        fit_words(canvas, book, &runs, spans, &mut items);
    }
    let mut lines = Vec::new();
    let mut start = 0;
    while start < items.len() {
        let mut end = start;
        let mut used = 0.0;
        let mut last_break: Option<usize> = None;
        let mut forced = false;
        while end < items.len() {
            let item = &items[end];
            if item.newline {
                forced = true;
                break;
            }
            if end > start && item.break_before {
                last_break = Some(end);
            }
            let fits = width.is_none_or(|w| item.space || used + item.width <= w + 0.01);
            if !fits && end > start {
                break;
            }
            used += item.width;
            end += 1;
        }
        let stop = if forced || end == items.len() {
            end
        } else {
            last_break.filter(|b| *b > start).unwrap_or(end)
        };
        let last = forced || stop == items.len();
        lines.push(match &levels {
            Some(levels) => levels.line(canvas, book, &runs, spans, &items[start..stop], last),
            None => make_line(&runs, spans, &items[start..stop], last),
        });
        start = if forced { stop + 1 } else { stop };
        while !forced && start < items.len() && items[start].space {
            start += 1;
        }
    }
    if lines.is_empty() {
        let mut line = Line {
            last: true,
            rtl: levels.as_ref().is_some_and(|l| l.rtl),
            ..Line::default()
        };
        if let Some(run) = runs.first() {
            line.ascent = run.ascent;
            line.descent = run.descent;
            line.gap = run.gap;
        } else if let Some(span) = spans.first() {
            line.ascent = span.size * 0.9;
            line.descent = span.size * 0.25;
        }
        lines.push(line);
    }
    lines
}

fn make_line(runs: &[Run], spans: &[Span], items: &[Item], last: bool) -> Line {
    let mut line = Line {
        last,
        ..Line::default()
    };
    let trailing = items.iter().rev().take_while(|i| i.space).count();
    let mut x = 0.0;
    let mut k = 0;
    while k < items.len() {
        let run_index = items[k].run;
        let first = items[k].cluster;
        let mut width = 0.0;
        let mut spaces = 0;
        let mut j = k;
        while j < items.len() && items[j].run == run_index {
            width += items[j].width;
            if items[j].space && j < items.len() - trailing {
                spaces += 1;
            }
            j += 1;
        }
        let run = &runs[run_index];
        let span = &spans[run.span];
        line.ascent = line.ascent.max(run.ascent);
        line.descent = line.descent.max(run.descent);
        line.gap = line.gap.max(run.gap);
        line.spaces += spaces;
        line.pieces.push(Piece {
            shaped: Rc::clone(&run.shaped),
            clusters: first..items[j - 1].cluster + 1,
            style: TextStyle {
                size: span.size,
                color: span.color,
                fake_bold: run.pick.fake_bold,
                fake_italic: run.pick.fake_italic,
            },
            underline: span.underline,
            strike: span.strike,
            x,
            width,
            underline_y: run.underline_y,
            underline_thickness: run.underline_thickness,
            x_height: run.x_height,
        });
        x += width;
        k = j;
    }
    let trailing_width: f32 = items.iter().rev().take(trailing).map(|i| i.width).sum();
    line.width = x - trailing_width;
    if line.pieces.is_empty()
        && let Some(item) = items.first()
    {
        let run = &runs[item.run];
        line.ascent = run.ascent;
        line.descent = run.descent;
        line.gap = run.gap;
    }
    line
}

fn needs_bidi(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            bidi::class(c),
            bidi::Class::R
                | bidi::Class::AL
                | bidi::Class::AN
                | bidi::Class::RLE
                | bidi::Class::RLO
                | bidi::Class::RLI
                | bidi::Class::FSI
                | bidi::Class::LRE
                | bidi::Class::LRO
                | bidi::Class::LRI
        )
    })
}

#[must_use]
pub fn starts_right_to_left(text: &str) -> bool {
    text.chars()
        .find_map(|c| match bidi::class(c) {
            bidi::Class::L => Some(false),
            bidi::Class::R | bidi::Class::AL => Some(true),
            _ => None,
        })
        .unwrap_or(false)
}

fn rtl_letter(text: &str) -> bool {
    text.chars().next().is_some_and(bidi::is_rtl)
}

struct Levels {
    rtl: bool,
    text: String,
    char_at: Vec<usize>,
    paragraphs: Vec<(usize, bidi::Paragraph)>,
}

impl Levels {
    fn of(spans: &[Span], direction: Direction) -> Option<Self> {
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
        if direction != Direction::Rtl && !needs_bidi(&text) {
            return None;
        }
        let rtl = match direction {
            Direction::Ltr => false,
            Direction::Rtl => true,
            Direction::Auto => starts_right_to_left(&text),
        };
        let chars: Vec<char> = text.chars().collect();
        let mut char_at = vec![0; text.len() + 1];
        let mut index = 0;
        for (at, c) in text.char_indices() {
            for slot in &mut char_at[at..at + c.len_utf8()] {
                *slot = index;
            }
            index += 1;
        }
        char_at[text.len()] = index;
        let mut paragraphs = Vec::new();
        let mut from = 0;
        for k in 0..=chars.len() {
            if k == chars.len() || matches!(chars[k], '\n' | '\r' | '\u{2029}') {
                paragraphs.push((from, bidi::paragraph(&chars[from..k], Some(u8::from(rtl)))));
                from = k + 1;
            }
        }
        Some(Self {
            rtl,
            text,
            char_at,
            paragraphs,
        })
    }

    fn of_line(&self, items: &[Item]) -> Vec<u8> {
        let (Some(first), Some(last)) = (items.first(), items.last()) else {
            return Vec::new();
        };
        let from = self.char_at[first.at];
        let to = self.char_at[last.end].max(from);
        let Some((start, paragraph)) = self.paragraphs.iter().rev().find(|(s, _)| *s <= from)
        else {
            return vec![u8::from(self.rtl); items.len()];
        };
        let local =
            (from - start).min(paragraph.levels.len())..(to - start).min(paragraph.levels.len());
        let levels = paragraph.line_levels(local);
        items
            .iter()
            .map(|i| {
                levels
                    .get(self.char_at[i.at] - from)
                    .copied()
                    .unwrap_or(paragraph.level)
            })
            .collect()
    }

    fn line(
        &self,
        canvas: &Canvas,
        book: &mut FontBook,
        runs: &[Run],
        spans: &[Span],
        items: &[Item],
        last: bool,
    ) -> Line {
        let mut line = Line {
            last,
            rtl: self.rtl,
            ..Line::default()
        };
        for item in items {
            let run = &runs[item.run];
            line.ascent = line.ascent.max(run.ascent);
            line.descent = line.descent.max(run.descent);
            line.gap = line.gap.max(run.gap);
        }
        let trailing = items.iter().rev().take_while(|i| i.space).count();
        let visible = &items[..items.len() - trailing];
        if visible.is_empty() {
            return line;
        }
        line.said = Some(visible.iter().map(|i| &self.text[i.at..i.end]).collect());
        let levels = self.of_line(visible);
        let letter = |i: &Item| rtl_letter(&self.text[i.at..i.end]);
        let mut word_of: Vec<Option<usize>> = vec![None; visible.len()];
        let mut words: Vec<Range<usize>> = Vec::new();
        let mut k = 0;
        while k < visible.len() {
            if levels[k] % 2 == 1 && letter(&visible[k]) {
                let start = k;
                while k + 1 < visible.len()
                    && visible[k + 1].run == visible[start].run
                    && levels[k + 1] == levels[start]
                    && letter(&visible[k + 1])
                {
                    k += 1;
                }
                for slot in &mut word_of[start..=k] {
                    *slot = Some(words.len());
                }
                words.push(start..k + 1);
            }
            k += 1;
        }
        let mut shown: Vec<(usize, String, ShapedCluster)> = Vec::new();
        let mut done = vec![false; words.len()];
        for k in bidi::visual_order(&levels) {
            let item = &visible[k];
            let run = &runs[item.run];
            if let Some(w) = word_of[k] {
                if std::mem::replace(&mut done[w], true) {
                    continue;
                }
                let range = words[w].clone();
                let word = &self.text[visible[range.start].at..visible[range.end - 1].end];
                let shaped = book.shape_rtl(canvas, run.pick.font, word);
                for cluster in shaped.clusters.iter().cloned() {
                    let text = if cluster.range.is_empty() {
                        cluster.glyphs.iter().map(|g| g.meaning.as_str()).collect()
                    } else {
                        shaped.text[cluster.range.clone()].to_owned()
                    };
                    shown.push((item.run, text, cluster));
                }
                continue;
            }
            let text = &self.text[item.at..item.end];
            let mut cluster = run.shaped.clusters[item.cluster].clone();
            let mut chars = text.chars();
            if levels[k] % 2 == 1
                && let (Some(c), None) = (chars.next(), chars.next())
                && let Some(mirrored) = bidi::mirror(c)
                && let Some(mut drawn) = canvas
                    .shape(run.pick.font, mirrored.encode_utf8(&mut [0; 4]))
                    .clusters
                    .into_iter()
                    .next()
                && !drawn.missing
            {
                for (n, glyph) in drawn.glyphs.iter_mut().enumerate() {
                    glyph.meaning = if n == 0 {
                        text.to_owned()
                    } else {
                        String::new()
                    };
                }
                cluster = drawn;
            }
            shown.push((item.run, text.to_owned(), cluster));
        }
        let mut x = 0.0;
        let mut at = 0;
        while at < shown.len() {
            let run_index = shown[at].0;
            let run = &runs[run_index];
            let span = &spans[run.span];
            let mut text = String::new();
            let mut clusters = Vec::new();
            while at < shown.len() && shown[at].0 == run_index {
                let (_, piece, mut cluster) = shown[at].clone();
                let start = text.len();
                text.push_str(&piece);
                cluster.range = start..text.len();
                clusters.push(cluster);
                at += 1;
            }
            line.spaces += clusters.iter().filter(|c| c.space).count();
            let count = clusters.len();
            let shaped = Shaped {
                font: run.shaped.font,
                text,
                units_per_em: run.shaped.units_per_em,
                clusters,
            };
            let width = shaped.total_width(span.size);
            line.pieces.push(Piece {
                shaped: Rc::new(shaped),
                clusters: 0..count,
                style: TextStyle {
                    size: span.size,
                    color: span.color,
                    fake_bold: run.pick.fake_bold,
                    fake_italic: run.pick.fake_italic,
                },
                underline: span.underline,
                strike: span.strike,
                x,
                width,
                underline_y: run.underline_y,
                underline_thickness: run.underline_thickness,
                x_height: run.x_height,
            });
            x += width;
        }
        line.width = x;
        line
    }
}

fn fit_words(
    canvas: &Canvas,
    book: &mut FontBook,
    runs: &[Run],
    spans: &[Span],
    items: &mut [Item],
) {
    let text_of = |item: &Item| {
        let run = &runs[item.run];
        &run.shaped.text[run.shaped.clusters[item.cluster].range.clone()]
    };
    let mut k = 0;
    while k < items.len() {
        if items[k].space || items[k].newline || !rtl_letter(text_of(&items[k])) {
            k += 1;
            continue;
        }
        let start = k;
        while k + 1 < items.len()
            && items[k + 1].run == items[start].run
            && !items[k + 1].space
            && !items[k + 1].newline
            && rtl_letter(text_of(&items[k + 1]))
        {
            k += 1;
        }
        let run = &runs[items[start].run];
        let first = run.shaped.clusters[items[start].cluster].range.start;
        let last = run.shaped.clusters[items[k].cluster].range.end;
        let whole = book
            .shape_rtl(canvas, run.pick.font, &run.shaped.text[first..last])
            .total_width(spans[run.span].size);
        let alone: f32 = items[start..=k].iter().map(|i| i.width).sum();
        if alone > 0.0 {
            for item in &mut items[start..=k] {
                item.width *= whole / alone;
            }
        }
        k += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::may_break;

    #[test]
    fn syllable_breaks() {
        assert!(!may_break("ກັ", "ບ", None));
        assert!(may_break("ບ", "ໂ", Some("ຮ")));
        assert!(!may_break("ໂ", "ຮ", Some("ງ")));
        assert!(may_break("ດ", "ກ", Some("າ")));
        assert!(!may_break("ab", "cd", None));
        assert!(may_break("well-", "known", None));
    }
}
