use pdf_content::{Code, Confidence};
use pdf_paint::{Color, ColorSpace, PaintAtomKind, TextShowPaint};
use pdf_session::PageView;

use crate::actual_text::ActualTexts;
use crate::geometry::Shown;
use crate::model::{Rect, Shift, Style};

pub const UNREAD: &str = "\u{FFFD}";

#[derive(Clone, Debug)]
pub struct Glyphs {
    pub text: String,
    pub rect: Rect,
    pub baseline: f64,
    pub style: Style,
    pub blank: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Wrap,
    WrapWithSpace,
    Paragraph,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub clusters: Vec<Glyphs>,
    pub rect: Rect,
    pub baseline: f64,
    pub em: f64,
    pub end: End,
    pub horizontal: bool,
}

#[derive(Clone, Debug)]
pub struct TextBlock {
    pub lines: Vec<Line>,
    pub read: bool,
    pub block: usize,
}

fn cluster_text(text: &TextShowPaint, glyphs: std::ops::Range<usize>) -> (String, bool) {
    let mut out = String::new();
    let mut legacy = false;
    let glyphless = text
        .font_request
        .as_deref()
        .is_some_and(|request| request.base_font.ends_with(b"GlyphLessFont"));
    for glyph in text.glyphs.get(glyphs).unwrap_or_default() {
        let meaning = text.text.text_of(Code {
            value: glyph.code.value,
            byte_len: glyph.code.bytes.len(),
        });
        match meaning {
            Some(meaning) if !meaning.text.is_empty() => {
                out.push_str(&meaning.text);
                legacy |= meaning.confidence == Confidence::Deciphered;
            }
            _ if glyphless => match char::from_u32(glyph.code.value) {
                Some(c) if !c.is_control() => out.push(c),
                _ => out.push_str(UNREAD),
            },
            _ => out.push_str(UNREAD),
        }
    }
    (out, legacy)
}

fn channel(value: f64) -> u8 {
    let v = (value.clamp(0.0, 1.0) * 255.0).round();
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        v as u8
    }
}

#[must_use]
pub fn rgb(color: &Color, space: &ColorSpace) -> [u8; 3] {
    let gray = |g: f64| [channel(g); 3];
    let cmyk = |c: f64, m: f64, y: f64, k: f64| {
        [
            channel((1.0 - c) * (1.0 - k)),
            channel((1.0 - m) * (1.0 - k)),
            channel((1.0 - y) * (1.0 - k)),
        ]
    };
    match color {
        Color::DeviceGray(g) | Color::CalGray(g) => gray(*g),
        Color::DeviceRgb(r, g, b) | Color::CalRgb(r, g, b) => {
            [channel(*r), channel(*g), channel(*b)]
        }
        Color::DeviceCmyk(c, m, y, k) => cmyk(*c, *m, *y, *k),
        Color::IccBased(values) | Color::DeviceN(values) => match values.as_slice() {
            [g] if matches!(space, ColorSpace::IccBased(_)) => gray(*g),
            [r, g, b] => [channel(*r), channel(*g), channel(*b)],
            [c, m, y, k] => cmyk(*c, *m, *y, *k),
            _ => [0, 0, 0],
        },
        Color::Separation(tint) => gray(1.0 - *tint),
        _ => [0, 0, 0],
    }
}

fn style_of(text: &TextShowPaint, em: f64, legacy: bool) -> Style {
    let request = text.font_request.as_deref();
    let family = request.map_or_else(String::new, |request| clean_family(&request.family));
    let bold = request.is_some_and(|request| request.style.is_bold());
    let italic = request.is_some_and(|request| request.style.italic);
    let state = &text.state;
    Style {
        family,
        size: (em.abs() * 2.0).round() / 2.0,
        bold,
        italic,
        underline: false,
        color: rgb(&state.fill_color.value, &state.fill_color_space.value),
        legacy,
        baseline: Shift::None,
    }
}

#[must_use]
pub fn clean_family(name: &str) -> String {
    let name = match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => name,
    };
    let name = name.split([',', '-']).next().unwrap_or(name);
    let name = name
        .strip_suffix("PSMT")
        .or_else(|| name.strip_suffix("MT"))
        .unwrap_or(name);
    name.trim().to_owned()
}

#[must_use]
pub fn read_blocks(view: &PageView, shown: &Shown, actual: &ActualTexts) -> Vec<TextBlock> {
    read_blocks_with(view, shown, true, actual)
}

#[must_use]
pub fn read_blocks_with(
    view: &PageView,
    shown: &Shown,
    dedupe: bool,
    actual: &ActualTexts,
) -> Vec<TextBlock> {
    let index = &view.index;
    let mut spoken: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut blocks = Vec::with_capacity(index.blocks.len());
    for (number, semantic) in index.blocks.iter().enumerate() {
        let rows: Vec<Vec<pdf_edit::ClusterRef>> = semantic
            .lines
            .iter()
            .map(|line| {
                index.lines[*line]
                    .clusters
                    .iter()
                    .map(|cluster| {
                        let cluster = &index.clusters[*cluster];
                        pdf_edit::ClusterRef {
                            anchor: pdf_edit::SourceAnchor::of(&view.graph.atoms[cluster.atom].id),
                            glyphs: cluster.glyphs.clone(),
                        }
                    })
                    .collect()
            })
            .collect();
        let laid = semantic.layout.or(semantic.bounds);
        let reading = laid.and_then(|laid| {
            let frame = (laid[0], laid[2]);
            let reading =
                pdf_edit::read_block(&view.program, &view.graph, &rows, frame, (0, 0), None)
                    .ok()?;
            let faces =
                pdf_edit::read_block_faces(&view.program, &view.graph, &rows, frame, (0, 0), None)
                    .ok();
            Some((reading, faces))
        });
        let mut lines = Vec::with_capacity(semantic.lines.len());
        for (row, line) in semantic.lines.iter().enumerate() {
            let (end, faces) = match &reading {
                Some((reading, faces)) => {
                    let at = reading.lines.iter().position(|read| read.row == Some(row));
                    let end = at.map_or(End::Paragraph, |at| {
                        let gap_after = reading
                            .lines
                            .get(at + 1)
                            .is_some_and(|next| next.row.is_none());
                        match reading.lines[at].end {
                            _ if gap_after => End::Paragraph,
                            pdf_edit::LineEnd::Wrap => End::Wrap,
                            pdf_edit::LineEnd::WrapWithSpace => End::WrapWithSpace,
                            _ => End::Paragraph,
                        }
                    });
                    let faces = at.and_then(|at| faces.as_ref().and_then(|faces| faces.get(at)));
                    (end, faces)
                }
                None => (End::Paragraph, None),
            };
            let clusters_of_row = &index.lines[*line].clusters;
            let faces = faces.filter(|faces| faces.len() == clusters_of_row.len());
            let mut clusters = Vec::with_capacity(clusters_of_row.len());
            for (position, at) in clusters_of_row.iter().enumerate() {
                let cluster = &index.clusters[*at];
                let PaintAtomKind::Text(paint) = &view.graph.atoms[cluster.atom].kind else {
                    continue;
                };
                let (mut text, legacy) = cluster_text(paint, cluster.glyphs.clone());
                if let Some((span, said)) = actual.get(&cluster.atom) {
                    text = if spoken.insert(span.as_str()) {
                        said.clone()
                    } else {
                        String::new()
                    };
                }
                let mut style = style_of(paint, cluster.em, legacy);
                if let Some(face) = faces.and_then(|faces| faces.get(position)) {
                    style.bold |= face.bold;
                    style.italic |= face.italic;
                    style.underline = face.underline;
                }
                let bounds = cluster.layout.or(cluster.bounds).unwrap_or_else(|| {
                    let o = cluster.origin;
                    [
                        o.x,
                        o.y,
                        o.x + cluster.advance.abs().max(0.1),
                        o.y + cluster.em.abs(),
                    ]
                });
                let rect = shown.rect(bounds);
                let baseline = shown.point(cluster.baseline.x, cluster.baseline.y).1;
                clusters.push(Glyphs {
                    blank: !text.is_empty() && text.chars().all(char::is_whitespace),
                    text,
                    rect,
                    baseline,
                    style,
                });
            }
            let mut clusters = if dedupe {
                without_overprints(clusters)
            } else {
                clusters
            };
            if clusters.is_empty() {
                continue;
            }
            let rect = clusters
                .iter()
                .skip(1)
                .fold(clusters[0].rect, |rect, glyphs| rect.union(&glyphs.rect));
            let (em, baseline) = clusters
                .iter()
                .filter(|glyphs| !glyphs.blank)
                .map(|glyphs| (glyphs.style.size, glyphs.baseline))
                .fold((0.0, clusters[0].baseline), |best, next| {
                    if next.0 > best.0 { next } else { best }
                });
            let horizontal = {
                let first = clusters[0].rect.center();
                let last = clusters[clusters.len() - 1].rect.center();
                (last.0 - first.0).abs() >= (last.1 - first.1).abs()
            };
            mark_shifts(&mut clusters, em, baseline);
            lines.push(Line {
                clusters,
                rect,
                baseline,
                em,
                end,
                horizontal,
            });
        }
        if lines.is_empty() {
            continue;
        }
        let read = reading.is_some();
        if !read {
            infer_ends(&mut lines);
        }
        if let Some(last) = lines.last_mut() {
            last.end = End::Paragraph;
        }
        blocks.push(TextBlock {
            lines,
            read,
            block: number,
        });
    }
    blocks
}

#[must_use]
pub fn group_blocks(view: &PageView, shown: &Shown) -> Vec<TextBlock> {
    fn walk(
        graph: &pdf_paint::PaintGraph,
        frame: Option<Rect>,
        depth: usize,
        out: &mut Vec<(Rect, Glyphs)>,
    ) {
        if depth > 8 {
            return;
        }
        for atom in &graph.atoms {
            match &atom.kind {
                PaintAtomKind::TransparencyGroup(group) => {
                    walk(&group.graph, frame, depth + 1, out)
                }
                PaintAtomKind::Text(paint) if depth > 0 => {
                    let Some(frame) = frame else { continue };
                    let (text, legacy) = cluster_text(paint, 0..paint.glyphs.len());
                    if text.trim().is_empty() {
                        continue;
                    }
                    let style = style_of(paint, paint.size_on_page().unwrap_or(10.0), legacy);
                    out.push((
                        frame,
                        Glyphs {
                            text,
                            rect: frame,
                            baseline: frame.y1,
                            style,
                            blank: false,
                        },
                    ));
                }
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    for atom in &view.graph.atoms {
        if let PaintAtomKind::TransparencyGroup(group) = &atom.kind {
            let frame = atom.kind.user_bounds().map(|b| shown.rect(b));
            walk(&group.graph, frame, 1, &mut found);
        }
    }
    found
        .into_iter()
        .map(|(rect, glyphs)| {
            let em = glyphs.style.size;
            TextBlock {
                lines: vec![Line {
                    clusters: vec![glyphs],
                    rect,
                    baseline: rect.y1,
                    em,
                    end: End::Paragraph,
                    horizontal: true,
                }],
                read: false,
                block: usize::MAX,
            }
        })
        .collect()
}

#[must_use]
pub fn annotation_blocks(
    view: &PageView,
    shown: &Shown,
    page_text: &[TextBlock],
) -> Vec<TextBlock> {
    let mut blocks = Vec::new();
    for annotation in &view.annotations {
        if !matches!(
            annotation.outcome,
            pdf_paint::AnnotationOutcome::Drawn
                | pdf_paint::AnnotationOutcome::DrawnFromStaleFieldAppearance
        ) {
            continue;
        }
        let mut pieces: Vec<Glyphs> = Vec::new();
        for atom in &annotation.graph.atoms {
            let PaintAtomKind::Text(paint) = &atom.kind else {
                continue;
            };
            if paint.state.text.rendering_mode.value == pdf_paint::TextRenderingMode::Invisible {
                continue;
            }
            let dingbats = paint.font_request.as_deref().is_some_and(|request| {
                let name = format!(
                    "{} {}",
                    String::from_utf8_lossy(&request.base_font),
                    request.family
                )
                .to_ascii_lowercase();
                name.contains("dingbat") || name.contains("wingding") || name.contains("zadb")
            });
            if dingbats || paint.glyphs.is_empty() {
                continue;
            }
            let (text, legacy) = cluster_text(paint, 0..paint.glyphs.len());
            if text.chars().all(|c| c.is_whitespace() || c == '\u{FFFD}') {
                continue;
            }
            let em = paint.size_on_page().unwrap_or(10.0).max(1.0);
            let ctm = paint.state.ctm.value;
            let origins: Vec<(f64, f64)> = paint
                .glyphs
                .iter()
                .map(|glyph| {
                    let at = ctm
                        .multiply(glyph.matrix)
                        .transform(pdf_paint::Point { x: 0.0, y: 0.0 });
                    shown.point(at.x, at.y)
                })
                .filter(|(x, y)| x.is_finite() && y.is_finite())
                .collect();
            let Some(&(x0, baseline)) = origins.first() else {
                continue;
            };
            let outline = paint.outline_bounds().map(|bounds| shown.rect(bounds));
            let last = origins.iter().map(|o| o.0).fold(x0, f64::max);
            let x1 = outline.map_or(last + 0.5 * em, |r| r.x1.max(last));
            let rect = Rect::new(
                x0.min(outline.map_or(x0, |r| r.x0)),
                baseline - 0.8 * em,
                x1,
                baseline + 0.25 * em,
            );
            let style = style_of(paint, em, legacy);
            pieces.push(Glyphs {
                text,
                rect,
                baseline,
                style,
                blank: false,
            });
        }
        if pieces.is_empty() {
            continue;
        }
        pieces.sort_by(|a, b| {
            a.baseline
                .total_cmp(&b.baseline)
                .then(a.rect.x0.total_cmp(&b.rect.x0))
        });
        let mut lines: Vec<Line> = Vec::new();
        for piece in pieces {
            let em = piece.style.size.max(1.0);
            match lines.last_mut() {
                Some(line) if (line.baseline - piece.baseline).abs() < 0.3 * em => {
                    let gap = piece.rect.x0 - line.rect.x1;
                    if gap > 0.2 * em {
                        line.clusters.push(Glyphs {
                            text: " ".to_owned(),
                            rect: Rect::new(
                                line.rect.x1,
                                line.rect.y0,
                                piece.rect.x0,
                                line.rect.y1,
                            ),
                            baseline: line.baseline,
                            style: piece.style.clone(),
                            blank: true,
                        });
                    }
                    line.rect = line.rect.union(&piece.rect);
                    line.em = line.em.max(em);
                    line.clusters.push(piece);
                }
                _ => lines.push(Line {
                    rect: piece.rect,
                    baseline: piece.baseline,
                    em,
                    end: End::Paragraph,
                    horizontal: true,
                    clusters: vec![piece],
                }),
            }
        }
        lines.retain(|line| {
            let said: String = line.clusters.iter().map(|g| g.text.as_str()).collect();
            let said = said.split_whitespace().collect::<String>();
            !page_text
                .iter()
                .flat_map(|block| &block.lines)
                .any(|other| {
                    other.rect.overlap(&line.rect) > 0.5 * line.rect.area().min(other.rect.area())
                        && other
                            .clusters
                            .iter()
                            .map(|g| g.text.as_str())
                            .collect::<String>()
                            .split_whitespace()
                            .collect::<String>()
                            == said
                })
        });
        if !lines.is_empty() {
            blocks.push(TextBlock {
                lines,
                read: false,
                block: usize::MAX,
            });
        }
    }
    blocks
}

#[must_use]
pub fn without_overprints(clusters: Vec<Glyphs>) -> Vec<Glyphs> {
    let mut kept: Vec<Glyphs> = Vec::with_capacity(clusters.len());
    for glyphs in clusters {
        if !glyphs.blank {
            let copy = kept.iter_mut().rev().take(12).find(|earlier| {
                earlier.text == glyphs.text && {
                    let overlap = earlier.rect.overlap(&glyphs.rect);
                    let smaller = earlier.rect.area().min(glyphs.rect.area());
                    smaller > 0.0 && overlap >= 0.6 * smaller
                }
            });
            if let Some(earlier) = copy {
                earlier.style.bold = true;
                continue;
            }
        }
        kept.push(glyphs);
    }
    kept
}

#[must_use]
pub fn overprints(clusters: &[Glyphs]) -> usize {
    clusters.len() - without_overprints(clusters.to_vec()).len()
}

fn mark_shifts(clusters: &mut [Glyphs], em: f64, baseline: f64) {
    for glyphs in clusters {
        if glyphs.blank || glyphs.style.size >= em * 0.85 {
            continue;
        }
        if glyphs.baseline < baseline - 0.2 * em {
            glyphs.style.baseline = Shift::Up;
        } else if glyphs.baseline > baseline + 0.1 * em {
            glyphs.style.baseline = Shift::Down;
        }
    }
}

fn numbered(text: &str) -> bool {
    let text = text.trim_start();
    let digits = text
        .chars()
        .take_while(|c| {
            c.is_ascii_digit()
                || ('\u{0ED0}'..='\u{0ED9}').contains(c)
                || ('\u{0E50}'..='\u{0E59}').contains(c)
        })
        .count();
    (1..=3).contains(&digits)
        && text
            .chars()
            .nth(digits)
            .is_some_and(|c| c == '.' || c == ')')
        && text
            .chars()
            .nth(digits + 1)
            .is_some_and(|c| !c.is_ascii_digit())
}

pub fn rejoin_wrapped(lines: &mut [Line]) {
    if lines.len() < 2 {
        return;
    }
    let mut gaps: Vec<f64> = lines
        .windows(2)
        .map(|pair| pair[1].baseline - pair[0].baseline)
        .filter(|gap| *gap > 0.0)
        .collect();
    gaps.sort_by(f64::total_cmp);
    let Some(usual) = gaps.get(gaps.len() / 2).copied() else {
        return;
    };
    let left = lines
        .iter()
        .map(|line| line.rect.x0)
        .fold(f64::INFINITY, f64::min);
    let right = lines
        .iter()
        .map(|line| line.rect.x1)
        .fold(f64::NEG_INFINITY, f64::max);
    for at in 0..lines.len() - 1 {
        let (this, next) = (&lines[at], &lines[at + 1]);
        if this.end != End::Paragraph || !this.horizontal || !next.horizontal {
            continue;
        }
        let em = this.em.max(1.0);
        let gap = next.baseline - this.baseline;
        let even = gap > 0.0 && (gap - usual).abs() <= 0.12 * usual && gap < 2.2 * em;
        let same_size = (next.em - this.em).abs() <= 0.1 * em;
        let opens = at == 0 || lines[at - 1].end == End::Paragraph;
        let step = next.rect.x0 - this.rect.x0;
        let hanging = step > 0.5 * em
            && step <= 4.0 * em
            && lines[at + 1..]
                .iter()
                .take_while(|line| line.end != End::Paragraph)
                .all(|line| (line.rect.x0 - next.rect.x0).abs() <= 0.5 * em);
        let aligned = step.abs() <= 0.5 * em
            || (opens
                && step < 0.0
                && -step <= 4.0 * em
                && (next.rect.x0 - left).abs() <= 0.5 * em)
            || (opens && hanging);
        let first_word = next
            .clusters
            .iter()
            .take_while(|glyphs| !glyphs.blank)
            .map(|glyphs| glyphs.rect.width())
            .sum::<f64>()
            .min(3.0 * em);
        let full = this.rect.x1 + 0.25 * em + first_word > right;
        let shared = lines
            .iter()
            .enumerate()
            .any(|(other, line)| other != at && line.rect.x1 >= right - 0.5 * em);
        let long = this.rect.width() >= 25.0 * em;
        let code = this.clusters.iter().any(|g| {
            let family = g.style.family.to_ascii_lowercase();
            ["mono", "courier", "consol", "code"]
                .iter()
                .any(|name| family.contains(name))
        });
        let said: String = next.clusters.iter().map(|g| g.text.as_str()).collect();
        let marker = crate::roles::list_marker(said.trim_start()).is_some() || numbered(&said);
        if even && same_size && aligned && full && (shared || long) && !marker && !code {
            let last = this
                .clusters
                .iter()
                .rev()
                .find(|g| !g.blank)
                .and_then(|g| g.text.chars().last());
            let first = said.trim_start().chars().next();
            let unspaced =
                |c: Option<char>| c.is_some_and(|c| ('\u{0E00}'..='\u{0EFF}').contains(&c));
            let hyphen = last.is_some_and(|c| matches!(c, '-' | '\u{2010}'));
            let spaced = this.clusters.last().is_some_and(|g| g.blank);
            lines[at].end = if hyphen || (!spaced && unspaced(last) && unspaced(first)) {
                End::Wrap
            } else {
                End::WrapWithSpace
            };
        }
    }
}

pub(crate) fn infer_ends(lines: &mut [Line]) {
    let mut gaps: Vec<f64> = lines
        .windows(2)
        .map(|pair| pair[1].baseline - pair[0].baseline)
        .filter(|gap| *gap > 0.0)
        .collect();
    gaps.sort_by(f64::total_cmp);
    let usual = gaps.get(gaps.len() / 2).copied();
    let left = lines
        .iter()
        .map(|line| line.rect.x0)
        .fold(f64::INFINITY, f64::min);
    let right = lines
        .iter()
        .map(|line| line.rect.x1)
        .fold(f64::NEG_INFINITY, f64::max);
    for at in 0..lines.len().saturating_sub(1) {
        let gap = lines[at + 1].baseline - lines[at].baseline;
        let em = lines[at].em.max(1.0);
        let wide_gap = usual.is_some_and(|usual| gap > usual * 1.4) || gap > em * 2.0 || gap < 0.0;
        let short = lines[at].rect.x1 < right - 4.0 * em;
        let indented = lines[at + 1].rect.x0 > left + 1.0 * em;
        lines[at].end = if wide_gap || (short && indented) {
            End::Paragraph
        } else {
            End::WrapWithSpace
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_closed_without_a_space_start_items() {
        assert!(numbered("4.ເພິ່ນຕົ່ມ"));
        assert!(numbered("12) item"));
        assert!(numbered("໒.ບົດ"));
        assert!(!numbered("4.5 kg"));
        assert!(!numbered("2026 was"));
        assert!(!numbered("word"));
    }

    #[test]
    fn family_names_lose_subset_tags_and_styles() {
        assert_eq!(clean_family("ABCDEF+Saysettha OT"), "Saysettha OT");
        assert_eq!(clean_family("TimesNewRomanPSMT"), "TimesNewRoman");
        assert_eq!(clean_family("Arial,Bold"), "Arial");
        assert_eq!(clean_family("Phetsarath-Bold"), "Phetsarath");
        assert_eq!(clean_family("abc+X"), "abc+X");
    }

    #[test]
    fn device_colours() {
        assert_eq!(
            rgb(&Color::DeviceGray(0.0), &ColorSpace::DeviceGray),
            [0, 0, 0]
        );
        assert_eq!(
            rgb(&Color::DeviceRgb(1.0, 0.0, 0.0), &ColorSpace::DeviceRgb),
            [255, 0, 0]
        );
        assert_eq!(
            rgb(
                &Color::DeviceCmyk(0.0, 0.0, 0.0, 1.0),
                &ColorSpace::DeviceCmyk
            ),
            [0, 0, 0]
        );
    }
}
