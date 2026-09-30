use std::rc::Rc;

use convert_pdf_canvas::ShapedCluster;
use convert_pdf_canvas::bidi;

use crate::fonts::{FontBook, Part, Pick, graphemes, is_space};
use crate::model::{
    Align, Border, Direction, ImageRef, Inline, LineHeight, Paragraph, Rgb, TabStop, TextStyle,
    VerticalShift,
};
use crate::page::{GlyphRun, Item};

#[derive(Clone, Debug, PartialEq)]
pub struct LaidLine {
    pub height: f32,
    pub items: Vec<Item>,
    pub baseline: f32,
    pub notes: Vec<usize>,
}

#[derive(Clone, Debug)]
enum Kind {
    Text {
        text: String,
        shaped: Rc<ShapedCluster>,
        parts: Option<Rc<Vec<Part>>>,
        pick: Pick,
        size: f32,
        style: usize,
    },
    Image {
        image: ImageRef,
        width: f32,
        height: f32,
    },
    Tab,
    Break,
    Anchor(usize),
}

#[derive(Clone, Debug)]
struct Atom {
    kind: Kind,
    start: usize,
    end: usize,
    width: f32,
    space: bool,
    break_after: bool,
    ascent: f32,
    descent: f32,
    gap: f32,
}

fn text_atom(
    book: &FontBook,
    styles: &[&TextStyle],
    style_index: usize,
    text: &str,
) -> Option<Atom> {
    let style = styles[style_index];
    let pick = book.pick(style, text)?;
    let complex = text.chars().next().is_some_and(crate::fonts::is_complex);
    let base = if complex {
        style.size_complex.unwrap_or(style.size)
    } else {
        style.size
    };
    let size = base * pick.scale;
    let shaped = book.shape(pick.face, text);
    let metrics = book.face(pick.face).metrics;
    #[allow(clippy::cast_precision_loss)]
    let width = shaped.advance as f32 * size / metrics.units_per_em + style.letter_spacing;
    Some(Atom {
        space: text.chars().all(is_space) || text == " ",
        kind: Kind::Text {
            text: text.to_owned(),
            shaped,
            parts: None,
            pick,
            size,
            style: style_index,
        },
        start: 0,
        end: 0,
        width,
        break_after: false,
        ascent: metrics.ascent * size,
        descent: metrics.descent * size,
        gap: metrics.line_gap * size,
    })
}

struct Levels {
    paragraph: bidi::Paragraph,
    char_at: Vec<usize>,
}

impl Levels {
    fn level_at(&self, at: usize) -> u8 {
        let index = self.char_at[at.min(self.char_at.len() - 1)];
        self.paragraph
            .levels
            .get(index)
            .copied()
            .unwrap_or(self.paragraph.level)
    }

    fn rtl(&self) -> bool {
        self.paragraph.level % 2 == 1
    }

    fn of_line(&self, atoms: &[Atom]) -> Vec<u8> {
        let last = self.char_at.len().saturating_sub(1);
        let char_of = |byte: usize| self.char_at[byte.min(last)];
        let (Some(first), Some(end)) = (atoms.first(), atoms.last()) else {
            return Vec::new();
        };
        let from = char_of(first.start);
        let to = char_of(end.end).max(from);
        let levels = self.paragraph.line_levels(from..to);
        atoms
            .iter()
            .map(|a| {
                let at = char_of(a.start).saturating_sub(from);
                levels.get(at).copied().unwrap_or(self.paragraph.level)
            })
            .collect()
    }
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

fn rtl_cluster(text: &str) -> bool {
    text.chars().next().is_some_and(bidi::is_rtl)
}

fn words_and_mirrors(
    atoms: Vec<Atom>,
    styles: &[&TextStyle],
    levels: &Levels,
    book: &FontBook,
) -> Vec<Atom> {
    let mut out: Vec<Atom> = Vec::with_capacity(atoms.len());
    for mut atom in atoms {
        let Kind::Text {
            text, pick, style, ..
        } = &atom.kind
        else {
            out.push(atom);
            continue;
        };
        if !rtl_cluster(text) {
            let level = levels.level_at(atom.start);
            let mut chars = text.chars();
            if level % 2 == 1
                && let (Some(c), None) = (chars.next(), chars.next())
                && let Some(mirrored) = bidi::mirror(c)
            {
                let mut shaped = (*book.shape(pick.face, &mirrored.to_string())).clone();
                if let Some(glyph) = shaped.glyphs.first_mut() {
                    glyph.meaning = text.clone();
                }
                if let Kind::Text { shaped: s, .. } = &mut atom.kind {
                    *s = Rc::new(shaped);
                }
            }
            out.push(atom);
            continue;
        }
        let (text, pick, style) = (text.clone(), *pick, *style);
        let joins = out.last().is_some_and(|last| {
            matches!(&last.kind, Kind::Text { text: t, pick: q, style: st, parts, .. }
                    if (rtl_cluster(t) || parts.is_some()) && q.face == pick.face && q.fake_bold == pick.fake_bold
                        && q.fake_italic == pick.fake_italic && q.scale == pick.scale && *st == style)
                && last.end == atom.start
        });
        if joins && let Some(last) = out.last_mut() {
            if let Kind::Text { text: t, .. } = &mut last.kind {
                t.push_str(&text);
            }
            last.end = atom.end;
            last.break_after = atom.break_after;
            last.width += atom.width;
        } else {
            out.push(atom);
        }
    }
    for atom in &mut out {
        let Kind::Text {
            text,
            pick,
            size,
            parts,
            style,
            ..
        } = &mut atom.kind
        else {
            continue;
        };
        if !rtl_cluster(text) {
            continue;
        }
        let shaped = book.shape_rtl(pick.face, text);
        let units = book.face(pick.face).metrics.units_per_em;
        let advance: i32 = shaped.iter().map(|(_, c)| c.advance).sum();
        #[allow(clippy::cast_precision_loss)]
        {
            atom.width = advance as f32 * *size / units + styles[*style].letter_spacing;
        }
        *parts = Some(shaped);
    }
    out
}

fn atoms<'a>(p: &'a Paragraph, book: &FontBook) -> (Vec<Atom>, Vec<&'a TextStyle>, Option<Levels>) {
    let mut styles: Vec<&TextStyle> = Vec::new();
    let mut full = String::new();
    let mut starts: Vec<usize> = Vec::new();
    let mut out: Vec<(usize, Atom)> = Vec::new();
    let style_of = |style: &'a TextStyle, styles: &mut Vec<&'a TextStyle>| {
        styles
            .iter()
            .position(|s| std::ptr::eq(*s, style) || **s == *style)
            .unwrap_or_else(|| {
                styles.push(style);
                styles.len() - 1
            })
    };
    for inline in &p.inlines {
        match inline {
            Inline::Text { text, style } => {
                let index = style_of(style, &mut styles);
                let text = if style.caps {
                    text.to_uppercase()
                } else {
                    text.clone()
                };
                let base = full.len();
                full.push_str(&text);
                for range in graphemes(&text) {
                    let cluster = &text[range.clone()];
                    starts.push(base + range.start);
                    if cluster == "\n" || cluster == "\r\n" {
                        out.push((base + range.start, bare(Kind::Break)));
                        continue;
                    }
                    if cluster == "\t" {
                        out.push((base + range.start, bare(Kind::Tab)));
                        continue;
                    }
                    if let Some(atom) = text_atom(book, &styles, index, cluster) {
                        out.push((base + range.start, atom));
                    }
                }
            }
            Inline::Image {
                image,
                width,
                height,
            } => {
                starts.push(full.len());
                out.push((
                    full.len(),
                    Atom {
                        kind: Kind::Image {
                            image: *image,
                            width: *width,
                            height: *height,
                        },
                        start: 0,
                        end: 0,
                        width: *width,
                        space: false,
                        break_after: false,
                        ascent: *height,
                        descent: 0.0,
                        gap: 0.0,
                    },
                ));
                full.push('\u{FFFC}');
            }
            Inline::LineBreak => {
                starts.push(full.len());
                out.push((full.len(), bare(Kind::Break)));
                full.push('\n');
            }
            Inline::Tab { .. } => {
                starts.push(full.len());
                out.push((full.len(), bare(Kind::Tab)));
                full.push('\t');
            }
            Inline::Anchor(note) => {
                let mut atom = bare(Kind::Anchor(*note));
                atom.break_after = false;
                out.push((full.len(), atom));
            }
        }
    }
    let breaks = pdf_edit::layout::line_break_opportunities(&full, &starts);
    let mut atoms: Vec<Atom> = Vec::with_capacity(out.len());
    for (index, (offset, mut atom)) in out.iter().cloned().enumerate() {
        let end = out.get(index + 1).map_or(full.len(), |next| next.0);
        atom.break_after = breaks.binary_search(&end).is_ok();
        atom.start = offset;
        atom.end = end.max(offset);
        atoms.push(atom);
    }
    if p.direction == Direction::Ltr && !needs_bidi(&full) {
        return (atoms, styles, None);
    }
    let chars: Vec<char> = full.chars().collect();
    let mut char_at = vec![0; full.len() + 1];
    let mut index = 0;
    for (at, c) in full.char_indices() {
        for slot in &mut char_at[at..at + c.len_utf8()] {
            *slot = index;
        }
        index += 1;
    }
    char_at[full.len()] = index;
    let base = match p.direction {
        Direction::Ltr => Some(0),
        Direction::Rtl => Some(1),
        Direction::Auto => None,
    };
    let levels = Levels {
        paragraph: bidi::paragraph(&chars, base),
        char_at,
    };
    let atoms = words_and_mirrors(atoms, &styles, &levels, book);
    (atoms, styles, Some(levels))
}

const fn bare(kind: Kind) -> Atom {
    Atom {
        kind,
        start: 0,
        end: 0,
        width: 0.0,
        space: false,
        break_after: true,
        ascent: 0.0,
        descent: 0.0,
        gap: 0.0,
    }
}

fn next_stop(x: f32, stops: &[TabStop]) -> (f32, bool) {
    if let Some(stop) = stops.iter().find(|s| s.pos > x + 0.5) {
        return (stop.pos, stop.right);
    }
    (((x + 0.5) / 36.0).floor() * 36.0 + 36.0, false)
}

fn next_tab(x: f32, stops: &[TabStop]) -> f32 {
    next_stop(x, stops).0
}

fn tab_width(x: f32, stops: &[TabStop], following: &[Atom]) -> f32 {
    let (stop, right) = next_stop(x, stops);
    if right {
        let after: f32 = following
            .iter()
            .take_while(|a| !matches!(a.kind, Kind::Tab | Kind::Break))
            .map(|a| a.width)
            .sum();
        (stop - x - after).max(0.0)
    } else {
        stop - x
    }
}

fn marker_place(p: &Paragraph, marker_width: f32, size: f32) -> (f32, f32) {
    let first = p.indent_left + p.indent_first;
    match &p.marker {
        Some(marker) if marker.outside => (first - marker_width - size * 0.5, first),
        Some(_) => {
            let end = first + marker_width;
            let text = if end <= p.indent_left - 0.01 {
                p.indent_left
            } else {
                let from_left = end - p.indent_left;
                p.indent_left + next_tab(from_left, &p.tabs).max(from_left + size * 0.25)
            };
            (first, text)
        }
        None => (first, first),
    }
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn lay_out(p: &Paragraph, width: f32, book: &FontBook) -> Vec<LaidLine> {
    let (atoms, styles, levels) = atoms(p, book);
    let rtl = levels.as_ref().is_some_and(Levels::rtl);
    let marker_atoms: Vec<Atom> = p.marker.as_ref().map_or_else(Vec::new, |marker| {
        let marker_styles = [&marker.style];
        graphemes(&marker.text)
            .into_iter()
            .filter_map(|r| text_atom(book, &marker_styles, 0, &marker.text[r]))
            .collect()
    });
    let marker_width: f32 = marker_atoms.iter().map(|a| a.width).sum();
    let marker_size = p.marker.as_ref().map_or(12.0, |m| m.style.size);
    let (marker_x, first_start) = marker_place(p, marker_width, marker_size);
    let right = width - p.indent_right;

    let mut lines: Vec<(std::ops::Range<usize>, bool)> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut x = 0.0_f32;
    let mut last_ok: Option<usize> = None;
    let line_start = |lines: &Vec<(std::ops::Range<usize>, bool)>| {
        if lines.is_empty() {
            first_start
        } else {
            p.indent_left
        }
    };
    while i < atoms.len() {
        let atom = &atoms[i];
        let left = line_start(&lines);
        let room = right - left;
        if matches!(atom.kind, Kind::Break) {
            lines.push((start..i + 1, true));
            start = i + 1;
            i = start;
            x = 0.0;
            last_ok = None;
            continue;
        }
        let w = if matches!(atom.kind, Kind::Tab) {
            let from_indent = left + x - p.indent_left;
            tab_width(from_indent, &p.tabs, &atoms[i + 1..])
        } else {
            atom.width
        };
        if !atom.space && x + w > room + 0.01 && i > start {
            let end = last_ok.map_or(i, |b| b + 1);
            lines.push((start..end, false));
            start = end;
            i = start;
            x = 0.0;
            last_ok = None;
            continue;
        }
        x += w;
        if atom.break_after {
            last_ok = Some(i);
        }
        i += 1;
    }
    if start < atoms.len() || lines.is_empty() || lines.last().is_some_and(|l| l.1) {
        lines.push((start..atoms.len(), true));
    }

    let empty_metrics = || -> (f32, f32, f32, f32) {
        let pick = book.pick(&p.empty_style, "x");
        pick.map_or(
            (
                0.8 * p.empty_style.size,
                0.2 * p.empty_style.size,
                0.0,
                p.empty_style.size,
            ),
            |pick| {
                let m = book.face(pick.face).metrics;
                let size = p.empty_style.size;
                (m.ascent * size, m.descent * size, m.line_gap * size, size)
            },
        )
    };

    let count = lines.len();
    let mut out = Vec::with_capacity(count);
    for (number, (range, forced)) in lines.into_iter().enumerate() {
        let first = number == 0;
        let left = if first { first_start } else { p.indent_left };
        let room = right - left;
        let line_atoms = &atoms[range];
        let mut visible = line_atoms.len();
        while visible > 0
            && (line_atoms[visible - 1].space
                || matches!(line_atoms[visible - 1].kind, Kind::Break))
        {
            visible -= 1;
        }
        let mut widths = Vec::with_capacity(line_atoms.len());
        let mut x = 0.0_f32;
        for (at, atom) in line_atoms.iter().enumerate() {
            let w = match atom.kind {
                Kind::Tab => {
                    let from_indent = left + x - p.indent_left;
                    tab_width(from_indent, &p.tabs, &line_atoms[at + 1..])
                }
                Kind::Break => 0.0,
                _ => atom.width,
            };
            widths.push(w);
            x += w;
        }
        let used: f32 = widths[..visible].iter().sum();
        let free = (room - used).max(0.0);
        let spaces = line_atoms[..visible].iter().filter(|a| a.space).count();
        let last_tab = line_atoms[..visible]
            .iter()
            .rposition(|a| matches!(a.kind, Kind::Tab));
        let (at_start, at_end) = if rtl { (free, 0.0) } else { (0.0, free) };
        let (offset, word_spacing) = match p.align {
            Align::Left => (at_start, 0.0),
            Align::Center => (free / 2.0, 0.0),
            Align::Right => (at_end, 0.0),
            Align::Justify => {
                let is_last = number + 1 == count || forced;
                if is_last || spaces == 0 || last_tab.is_some() {
                    (at_start, 0.0)
                } else {
                    #[allow(clippy::cast_precision_loss)]
                    (0.0, free / spaces as f32)
                }
            }
        };

        let mut ascent = 0.0_f32;
        let mut descent = 0.0_f32;
        let mut gap = 0.0_f32;
        let mut biggest = 0.0_f32;
        let mut tallest_image = 0.0_f32;
        let mut any_text = false;
        let with_marker = if first { &marker_atoms[..] } else { &[] };
        for atom in line_atoms.iter().chain(with_marker) {
            match &atom.kind {
                Kind::Text { size, .. } => {
                    any_text = true;
                    ascent = ascent.max(atom.ascent);
                    descent = descent.max(atom.descent);
                    gap = gap.max(atom.gap);
                    biggest = biggest.max(*size);
                }
                Kind::Image { height, .. } => {
                    tallest_image = tallest_image.max(*height);
                    ascent = ascent.max(*height);
                }
                _ => {}
            }
        }
        if !any_text {
            let (a, d, g, s) = empty_metrics();
            ascent = ascent.max(a);
            descent = descent.max(d);
            gap = gap.max(g);
            biggest = biggest.max(s);
        }
        let natural = ascent + descent + gap;
        let height = match p.line_height {
            LineHeight::Multiple(m) => {
                let text = (ascent.max(0.0) + descent + gap) * m;
                if tallest_image > 0.0 {
                    text.max(tallest_image + descent)
                } else {
                    text
                }
            }
            LineHeight::OfSize(f) => (biggest * f).max(tallest_image + descent),
            LineHeight::Exact(v) => v,
            LineHeight::AtLeast(v) => v.max(natural),
        };
        let baseline = (height - (ascent + descent)) / 2.0 + ascent;

        let mut items = Vec::new();
        let (edge_left, edge_right) = if rtl {
            (p.indent_right, width - p.indent_left)
        } else {
            (p.indent_left, right)
        };
        if let Some(bg) = p.background {
            items.push(Item::Rect {
                x: edge_left,
                y: 0.0,
                width: edge_right - edge_left,
                height,
                fill: Some(bg),
                stroke: None,
            });
        }
        borders(
            p,
            first,
            number + 1 == count,
            (edge_left, edge_right),
            height,
            &mut items,
        );
        if first && !marker_atoms.is_empty() {
            let (marker_atoms, marker_at) = if rtl {
                (
                    marker_in_order(&marker_atoms, p),
                    width - marker_x - marker_width,
                )
            } else {
                (marker_atoms.clone(), marker_x)
            };
            place_atoms(
                &marker_atoms,
                &marker_atoms.iter().map(|a| a.width).collect::<Vec<_>>(),
                &[p.marker.as_ref().map_or(&p.empty_style, |m| &m.style)],
                marker_at,
                baseline,
                0.0,
                &mut items,
            );
        }
        let line_left = if rtl { p.indent_right } else { left };
        let spoken = p.preformatted
            && levels.is_none()
            && line_atoms[..visible]
                .windows(2)
                .any(|w| w[0].space && w[1].space);
        let first_item = items.len();
        if let Some(levels) = &levels {
            let order = bidi::visual_order(&levels.of_line(&line_atoms[..visible]));
            let shown: Vec<Atom> = order.iter().map(|&k| line_atoms[k].clone()).collect();
            let shown_widths: Vec<f32> = order.iter().map(|&k| widths[k]).collect();
            place_atoms(
                &shown,
                &shown_widths,
                &styles,
                line_left + offset,
                baseline,
                word_spacing,
                &mut items,
            );
        } else {
            place_atoms(
                &line_atoms[..visible],
                &widths[..visible],
                &styles,
                line_left + offset,
                baseline,
                word_spacing,
                &mut items,
            );
        }
        if spoken {
            for item in &mut items[first_item..] {
                if let Item::Glyphs(run) = item {
                    run.spoken = true;
                }
            }
        }
        let notes = line_atoms
            .iter()
            .filter_map(|a| match a.kind {
                Kind::Anchor(n) => Some(n),
                _ => None,
            })
            .collect();
        out.push(LaidLine {
            height,
            items,
            baseline,
            notes,
        });
    }
    out
}

fn marker_in_order(atoms: &[Atom], p: &Paragraph) -> Vec<Atom> {
    let Some(marker) = &p.marker else {
        return atoms.to_vec();
    };
    let chars: Vec<char> = marker.text.chars().collect();
    let resolved = bidi::paragraph(&chars, Some(1));
    let mut levels = Vec::with_capacity(atoms.len());
    let mut index = 0;
    for range in graphemes(&marker.text) {
        levels.push(resolved.levels.get(index).copied().unwrap_or(1));
        index += marker.text[range].chars().count();
    }
    if levels.len() != atoms.len() {
        return atoms.to_vec();
    }
    bidi::visual_order(&levels)
        .into_iter()
        .map(|k| atoms[k].clone())
        .collect()
}

fn borders(
    p: &Paragraph,
    first: bool,
    last: bool,
    (left, right): (f32, f32),
    height: f32,
    items: &mut Vec<Item>,
) {
    let line = |x1: f32, y1: f32, x2: f32, y2: f32, b: Border| Item::Line {
        x1,
        y1,
        x2,
        y2,
        width: b.width,
        colour: b.colour,
    };
    if first && let Some(b) = p.borders.top {
        items.push(line(left, 0.0, right, 0.0, b));
    }
    if last && let Some(b) = p.borders.bottom {
        items.push(line(left, height, right, height, b));
    }
    if let Some(b) = p.borders.left {
        items.push(line(left, 0.0, left, height, b));
    }
    if let Some(b) = p.borders.right {
        items.push(line(right, 0.0, right, height, b));
    }
}

#[derive(PartialEq)]
struct RunKey {
    face: usize,
    size: f32,
    colour: Rgb,
    fake_bold: bool,
    fake_italic: bool,
    shift: VerticalShift,
    spaced: bool,
}

#[allow(clippy::too_many_arguments)]
fn place_atoms(
    atoms: &[Atom],
    widths: &[f32],
    styles: &[&TextStyle],
    x: f32,
    baseline: f32,
    word_spacing: f32,
    items: &mut Vec<Item>,
) {
    let base = items.len();
    let mut pen = x;
    let mut run: Option<(RunKey, GlyphRun)> = None;
    let mut spans: Vec<(usize, f32, f32, f32)> = Vec::new();
    let flush = |run: &mut Option<(RunKey, GlyphRun)>, items: &mut Vec<Item>| {
        if let Some((_, glyphs)) = run.take()
            && !glyphs.clusters.is_empty()
        {
            items.push(Item::Glyphs(glyphs));
        }
    };
    for (atom, width) in atoms.iter().zip(widths) {
        let advance = *width + if atom.space { word_spacing } else { 0.0 };
        match &atom.kind {
            Kind::Text {
                text,
                shaped,
                parts,
                pick,
                size,
                style,
            } => {
                let st = styles[*style];
                let shift_y = match st.shift {
                    VerticalShift::None => 0.0,
                    VerticalShift::Super => -size * 0.45,
                    VerticalShift::Sub => size * 0.2,
                };
                let key = RunKey {
                    face: pick.face,
                    size: *size,
                    colour: st.colour,
                    fake_bold: pick.fake_bold,
                    fake_italic: pick.fake_italic,
                    shift: st.shift,
                    spaced: st.letter_spacing != 0.0,
                };
                let joins = run.as_ref().is_some_and(|(k, _)| *k == key && !key.spaced);
                if !joins {
                    flush(&mut run, items);
                    run = Some((
                        key,
                        GlyphRun {
                            face: pick.face,
                            size: *size,
                            colour: st.colour,
                            fake_bold: pick.fake_bold,
                            fake_italic: pick.fake_italic,
                            x: pen,
                            y: baseline + shift_y,
                            word_spacing,
                            width: 0.0,
                            clusters: Vec::new(),
                            spoken: false,
                        },
                    ));
                }
                if let Some((_, glyphs)) = run.as_mut() {
                    match parts {
                        Some(parts) => glyphs.clusters.extend(parts.iter().cloned()),
                        None => glyphs.clusters.push((text.clone(), Rc::clone(shaped))),
                    }
                    glyphs.width += advance;
                }
                match spans.last_mut() {
                    Some(last) if last.0 == *style && (last.2 - pen).abs() < 0.01 => {
                        last.2 = pen + advance;
                        last.3 = last.3.max(*size);
                    }
                    _ => spans.push((*style, pen, pen + advance, *size)),
                }
            }
            Kind::Image {
                image,
                width: w,
                height: h,
            } => {
                flush(&mut run, items);
                items.push(Item::Image {
                    image: *image,
                    x: pen,
                    y: baseline - h,
                    width: *w,
                    height: *h,
                });
            }
            Kind::Tab | Kind::Break => flush(&mut run, items),
            Kind::Anchor(_) => {}
        }
        pen += advance;
    }
    flush(&mut run, items);
    let mut behind = Vec::new();
    for (style, from, to, size) in spans {
        let st = styles[style];
        if let Some(bg) = st.background {
            behind.push(Item::Rect {
                x: from,
                y: baseline - size * 0.9,
                width: to - from,
                height: size * 1.15,
                fill: Some(bg),
                stroke: None,
            });
        }
        let thickness = (size / 16.0).max(0.4);
        if st.underline {
            let y = baseline + size * 0.12;
            items.push(Item::Line {
                x1: from,
                y1: y,
                x2: to,
                y2: y,
                width: thickness,
                colour: st.colour,
            });
        }
        if st.strike {
            let y = baseline - size * 0.3;
            items.push(Item::Line {
                x1: from,
                y1: y,
                x2: to,
                y2: y,
                width: thickness,
                colour: st.colour,
            });
        }
        if let Some(uri) = &st.link {
            items.push(Item::Link {
                x: from,
                y: baseline - size * 0.9,
                width: to - from,
                height: size * 1.15,
                uri: uri.clone(),
            });
        }
    }
    for (index, item) in behind.into_iter().enumerate() {
        items.insert(base + index, item);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::Inline;

    fn package() -> FontBook {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../panpdf.rs/fonts/packaged"
        );
        let provider = pdf_font::system_fonts::SystemFontProvider::discover_in(&[dir.into()]);
        FontBook::new(Some(Arc::new(provider)))
    }

    fn paragraph(text: &str, direction: Direction) -> Paragraph {
        Paragraph {
            inlines: vec![Inline::Text {
                text: text.into(),
                style: TextStyle {
                    family: "DejaVu Sans".into(),
                    ..TextStyle::default()
                },
            }],
            direction,
            ..Paragraph::default()
        }
    }

    fn runs(line: &LaidLine) -> Vec<(f32, f32, String)> {
        line.items
            .iter()
            .filter_map(|item| match item {
                Item::Glyphs(run) => Some((
                    run.x,
                    run.x + run.width,
                    run.clusters.iter().map(|(t, _)| t.as_str()).collect(),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_right_to_left_paragraph_starts_on_the_right() {
        let book = package();
        let p = paragraph("D03.8 \u{05E9}\u{05DC}\u{05D5}\u{05DD}", Direction::Rtl);
        let lines = lay_out(&p, 300.0, &book);
        assert_eq!(lines.len(), 1);
        let runs = runs(&lines[0]);
        let right = runs.iter().map(|r| r.1).fold(0.0, f32::max);
        assert!((right - 300.0).abs() < 0.5, "{runs:?}");
        let text: String = runs.into_iter().map(|r| r.2).collect();
        assert_eq!(text, "\u{05DD}\u{05D5}\u{05DC}\u{05E9} D03.8");
    }

    #[test]
    fn right_to_left_words_in_a_left_to_right_line_are_reversed() {
        let book = package();
        let p = paragraph("a \u{05D0}\u{05D1} \u{05D2}\u{05D3} b", Direction::Ltr);
        let lines = lay_out(&p, 300.0, &book);
        let text: String = runs(&lines[0]).into_iter().map(|r| r.2).collect();
        assert_eq!(text, "a \u{05D3}\u{05D2} \u{05D1}\u{05D0} b");
    }

    #[test]
    fn brackets_mirror_at_right_to_left_levels() {
        let book = package();
        let p = paragraph("\u{05D0} (\u{05D1})", Direction::Rtl);
        let lines = lay_out(&p, 300.0, &book);
        let line = &lines[0];
        let mut glyphs = Vec::new();
        for item in &line.items {
            if let Item::Glyphs(run) = item {
                for (text, cluster) in &run.clusters {
                    glyphs.push((text.clone(), cluster.glyphs.first().map(|g| g.gid)));
                }
            }
        }
        let open = glyphs.iter().find(|g| g.0 == "(").expect("(");
        let close = glyphs.iter().find(|g| g.0 == ")").expect(")");
        let at = |t: &str| glyphs.iter().position(|g| g.0 == t);
        assert!(at("(") > at(")"), "{glyphs:?}");
        assert_ne!(open.1, close.1);
    }

    #[test]
    fn left_to_right_paragraphs_are_untouched() {
        let book = package();
        let p = paragraph("plain text, ສະບາຍດີ", Direction::Ltr);
        let (_, _, levels) = atoms(&p, &book);
        assert!(levels.is_none());
    }
}
