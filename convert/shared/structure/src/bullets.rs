use pdf_paint::{PaintAtomKind, PathSegment};
use pdf_session::PageView;

use crate::geometry::Shown;
use crate::model::Rect;
use crate::text::{Glyphs, TextBlock, rgb};

const MOST_SIDE: f64 = 8.0;
const LEAST_SIDE: f64 = 1.5;

struct Marker {
    rect: Rect,
    character: char,
}

fn markers(view: &PageView, shown: &Shown) -> Vec<Marker> {
    let mut found = Vec::new();
    for atom in &view.graph.atoms {
        let PaintAtomKind::Path(paint) = &atom.kind else {
            continue;
        };
        let Some(bounds) = atom.kind.user_bounds() else {
            continue;
        };
        let rect = shown.rect(bounds);
        let (w, h) = (rect.width(), rect.height());
        if !(LEAST_SIDE..=MOST_SIDE).contains(&w)
            || !(LEAST_SIDE..=MOST_SIDE).contains(&h)
            || w > 1.6 * h
            || h > 1.6 * w
        {
            continue;
        }
        let curved = paint
            .path
            .segments
            .iter()
            .any(|s| matches!(s, PathSegment::CubicTo { .. }));
        let fill = rgb(
            &paint.state.fill_color.value,
            &paint.state.fill_color_space.value,
        );
        let filled = paint.fill.is_some() && fill.iter().any(|c| *c < 245);
        let character = match (filled, paint.stroke, curved) {
            (true, _, true) => '•',
            (true, _, false) => '▪',
            (false, true, true) => '◦',
            _ => continue,
        };
        found.push(Marker { rect, character });
    }
    found
}

pub fn mark(view: &PageView, shown: &Shown, blocks: &mut [TextBlock]) {
    let mut markers = markers(view, shown);
    if markers.is_empty() || markers.len() > 400 {
        return;
    }
    for block in blocks.iter_mut() {
        for line in &mut block.lines {
            if !line.horizontal {
                continue;
            }
            let Some(first) = line.clusters.iter().find(|g| !g.blank) else {
                continue;
            };
            if first
                .text
                .trim_start()
                .starts_with(['•', '◦', '▪', '●', '○', '■', '-', '–'])
            {
                continue;
            }
            let em = line.em.max(1.0);
            let band = (line.baseline - 0.9 * em, line.baseline + 0.1 * em);
            let chosen = markers
                .iter()
                .enumerate()
                .filter(|(_, marker)| {
                    let (_, y) = marker.rect.center();
                    let gap = line.rect.x0 - marker.rect.x1;
                    y >= band.0
                        && y <= band.1
                        && gap >= 0.0
                        && gap <= 2.5 * em
                        && marker.rect.height() <= 0.8 * em
                })
                .min_by(|a, b| {
                    (line.rect.x0 - a.1.rect.x1).total_cmp(&(line.rect.x0 - b.1.rect.x1))
                })
                .map(|(at, _)| at);
            let Some(at) = chosen else {
                continue;
            };
            let marker = markers.swap_remove(at);
            let style = first.style.clone();
            let space = Rect::new(marker.rect.x1, line.rect.y0, line.rect.x0, line.rect.y1);
            line.clusters.insert(
                0,
                Glyphs {
                    text: " ".to_owned(),
                    rect: space,
                    baseline: line.baseline,
                    style: style.clone(),
                    blank: true,
                },
            );
            line.clusters.insert(
                0,
                Glyphs {
                    text: marker.character.to_string(),
                    rect: marker.rect,
                    baseline: line.baseline,
                    style,
                    blank: false,
                },
            );
            line.rect = line.rect.union(&marker.rect);
        }
    }
}
