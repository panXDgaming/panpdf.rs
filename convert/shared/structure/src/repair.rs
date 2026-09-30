use crate::model::Rect;
use crate::text::{End, Glyphs, Line, TextBlock};

fn rect_of(clusters: &[Glyphs]) -> Rect {
    clusters
        .iter()
        .skip(1)
        .fold(clusters[0].rect, |rect, glyphs| rect.union(&glyphs.rect))
}

fn line_from(template: &Line, clusters: Vec<Glyphs>) -> Line {
    Line {
        rect: rect_of(&clusters),
        clusters,
        baseline: template.baseline,
        em: template.em,
        end: template.end,
        horizontal: template.horizontal,
    }
}

#[must_use]
pub fn inside_page(blocks: Vec<TextBlock>, page: &Rect) -> Vec<TextBlock> {
    let outside = |r: &Rect| {
        r.x1 < page.x0 - 1.0 || r.x0 > page.x1 + 1.0 || r.y1 < page.y0 - 1.0 || r.y0 > page.y1 + 1.0
    };
    blocks
        .into_iter()
        .filter_map(|mut block| {
            block.lines = block
                .lines
                .into_iter()
                .filter_map(|line| {
                    let kept: Vec<Glyphs> = line
                        .clusters
                        .iter()
                        .filter(|g| !outside(&g.rect))
                        .cloned()
                        .collect();
                    (!kept.is_empty()).then(|| line_from(&line, kept))
                })
                .collect();
            (!block.lines.is_empty()).then_some(block)
        })
        .collect()
}

fn cut_at_gutters(line: &Line) -> Vec<Line> {
    cut_at(line, 1.5)
}

#[must_use]
pub fn cut_at(line: &Line, ems: f64) -> Vec<Line> {
    if !line.horizontal || line.clusters.len() < 2 {
        return vec![line.clone()];
    }
    let em = line.em.max(1.0);
    let mut pieces: Vec<Vec<Glyphs>> = vec![Vec::new()];
    let mut last_ink: Option<f64> = None;
    for glyphs in &line.clusters {
        if !glyphs.blank && !glyphs.text.is_empty() {
            if let Some(right) = last_ink
                && glyphs.rect.x0 - right > ems * em
            {
                pieces.push(Vec::new());
            }
            last_ink = Some(glyphs.rect.x1);
        }
        if let Some(piece) = pieces.last_mut() {
            piece.push(glyphs.clone());
        }
    }
    pieces
        .into_iter()
        .filter(|piece| piece.iter().any(|g| !g.blank && !g.text.is_empty()))
        .map(|piece| line_from(line, piece))
        .collect()
}

#[must_use]
pub fn split_columns(blocks: Vec<TextBlock>) -> Vec<TextBlock> {
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        let cut: Vec<Vec<Line>> = block.lines.iter().map(cut_at_gutters).collect();
        let gaps: Vec<(f64, f64)> = cut
            .iter()
            .filter(|pieces| pieces.len() > 1)
            .map(|pieces| (pieces[0].rect.x1, pieces[1].rect.x0))
            .collect();
        let shared = gaps.iter().enumerate().any(|(i, a)| {
            gaps.iter()
                .skip(i + 1)
                .any(|b| a.1.min(b.1) - a.0.max(b.0) > 0.0)
        });
        if !shared {
            out.push(block);
            continue;
        }
        let mut columns: Vec<(f64, f64, Vec<Line>)> = Vec::new();
        for pieces in cut {
            for piece in pieces {
                let (x0, x1) = (piece.rect.x0, piece.rect.x1);
                let best = columns
                    .iter()
                    .enumerate()
                    .map(|(at, (a, b, _))| (at, x1.min(*b) - x0.max(*a)))
                    .filter(|(_, overlap)| *overlap > 0.0)
                    .max_by(|p, q| p.1.total_cmp(&q.1))
                    .map(|(at, _)| at);
                match best {
                    Some(at) => {
                        let column = &mut columns[at];
                        column.0 = column.0.min(x0);
                        column.1 = column.1.max(x1);
                        column.2.push(piece);
                    }
                    None => columns.push((x0, x1, vec![piece])),
                }
            }
        }
        columns.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, _, mut lines) in columns {
            crate::text::infer_ends(&mut lines);
            if let Some(last) = lines.last_mut() {
                last.end = End::Paragraph;
            }
            out.push(TextBlock {
                lines,
                read: false,
                block: block.block,
            });
        }
    }
    out
}

#[must_use]
pub fn rejoin_shifted(mut blocks: Vec<TextBlock>) -> Vec<TextBlock> {
    let places: Vec<(usize, usize)> = blocks
        .iter()
        .enumerate()
        .flat_map(|(b, block)| (0..block.lines.len()).map(move |l| (b, l)))
        .collect();
    let mut moves: Vec<((usize, usize), (usize, usize))> = Vec::new();
    for &(b, l) in &places {
        let small = &blocks[b].lines[l];
        let visible: usize = small
            .clusters
            .iter()
            .filter(|g| !g.blank)
            .map(|g| g.text.chars().count())
            .sum();
        if !small.horizontal || visible == 0 || visible > 12 {
            continue;
        }
        let host = places
            .iter()
            .copied()
            .filter(|&(hb, hl)| (hb, hl) != (b, l))
            .filter_map(|(hb, hl)| {
                let line = &blocks[hb].lines[hl];
                let em = line.em.max(1.0);
                if !line.horizontal
                    || small.em > line.em * 1.05
                    || line.clusters.len() <= small.clusters.len()
                {
                    return None;
                }
                let shift = (small.baseline - line.baseline).abs();
                let (_, middle) = small.rect.center();
                let within_band =
                    middle > line.rect.y0 - 0.3 * em && middle < line.rect.y1 + 0.3 * em;
                let within_span = small.rect.x0 >= line.rect.x0 - 0.5 * em
                    && small.rect.x0 <= line.rect.x1 + 1.5 * em;
                let clear = small.clusters.iter().filter(|g| !g.blank).all(|g| {
                    line.clusters
                        .iter()
                        .filter(|h| !h.blank)
                        .all(|h| g.rect.x1.min(h.rect.x1) - g.rect.x0.max(h.rect.x0) < 0.2 * em)
                });
                (shift > 0.1 * em && shift < 0.7 * em && within_band && within_span && clear)
                    .then_some(((hb, hl), shift / em))
            })
            .min_by(|p, q| p.1.total_cmp(&q.1))
            .map(|(place, _)| place);
        if let Some(host) = host
            && !moves.iter().any(|(from, _)| *from == host)
        {
            moves.push(((b, l), host));
        }
    }
    if moves.is_empty() {
        return blocks;
    }
    for ((b, l), (hb, hl)) in &moves {
        let moved = std::mem::take(&mut blocks[*b].lines[*l].clusters);
        let host = &mut blocks[*hb].lines[*hl];
        host.clusters.extend(moved);
        host.clusters
            .sort_by(|p, q| p.rect.center().0.total_cmp(&q.rect.center().0));
        host.rect = rect_of(&host.clusters);
        let (em, baseline) = (host.em, host.baseline);
        for glyphs in &mut host.clusters {
            if glyphs.baseline < baseline - 0.2 * em && glyphs.style.size < em * 0.95 {
                glyphs.style.baseline = crate::model::Shift::Up;
            } else if glyphs.baseline > baseline + 0.1 * em && glyphs.style.size < em * 0.95 {
                glyphs.style.baseline = crate::model::Shift::Down;
            }
        }
    }
    for block in &mut blocks {
        block.lines.retain(|line| !line.clusters.is_empty());
    }
    blocks.retain(|block| !block.lines.is_empty());
    blocks
}
