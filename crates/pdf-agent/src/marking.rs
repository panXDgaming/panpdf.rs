use std::collections::HashMap;

use pdf_cli::TextClusterBox;
use pdf_edit::{Command, PenBlend, PenStep, PenStroke};
use pdf_session::PageView;

use crate::desk;
use crate::finding::{PageWork, Search, matches_in};

pub const YELLOW: [f64; 3] = [1.0, 0.92, 0.23];

pub const MARKER_OPACITY: f64 = 0.4;

pub const MOST_MARKS: usize = 400;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum How {
    Highlight,
    Underline,
    StrikeThrough,
}

impl How {
    #[must_use]
    pub fn of(word: &str) -> Option<Self> {
        match word.trim() {
            "highlight" => Some(Self::Highlight),
            "underline" => Some(Self::Underline),
            "strike_through" | "strike" | "strikethrough" => Some(Self::StrikeThrough),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Highlight => "highlight",
            Self::Underline => "underline",
            Self::StrikeThrough => "strike_through",
        }
    }

    #[must_use]
    pub const fn done(self) -> &'static str {
        match self {
            Self::Highlight => "highlighted",
            Self::Underline => "underlined",
            Self::StrikeThrough => "struck through",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marking {
    pub how: How,
    pub colour: [f64; 3],
}

impl Marking {
    #[must_use]
    pub const fn new(how: How, colour: Option<[f64; 3]>) -> Self {
        let colour = match colour {
            Some(colour) => colour,
            None => match how {
                How::Highlight => YELLOW,
                How::Underline | How::StrikeThrough => [0.0; 3],
            },
        };
        Self { how, colour }
    }
}

fn boxes_by_place(clusters: &[TextClusterBox]) -> HashMap<(usize, usize), [f64; 4]> {
    clusters
        .iter()
        .filter_map(|cluster| {
            cluster
                .box_pixels
                .map(|boxed| ((cluster.line, cluster.index_in_line), boxed))
        })
        .collect()
}

fn joined(one: [f64; 4], other: [f64; 4]) -> [f64; 4] {
    [
        one[0].min(other[0]),
        one[1].min(other[1]),
        one[2].max(other[2]),
        one[3].max(other[3]),
    ]
}

fn stroke_across(
    [left, top, right, bottom]: [f64; 4],
    em: f64,
    marking: Marking,
) -> (Vec<PenStep>, PenStroke) {
    let height = bottom - top;
    let middle = f64::midpoint(top, bottom);
    let (at, width, opacity, blend) = match marking.how {
        How::Highlight => (middle, height.max(em), MARKER_OPACITY, PenBlend::Multiply),
        How::Underline => (
            bottom + 0.08 * em,
            (em * 0.05).max(0.5),
            1.0,
            PenBlend::Normal,
        ),
        How::StrikeThrough => (middle, (em * 0.05).max(0.5), 1.0, PenBlend::Normal),
    };
    (
        vec![PenStep::Move((left, at)), PenStep::Line((right, at))],
        PenStroke {
            colour: marking.colour,
            width,
            opacity,
            blend,
            round_ends: false,
        },
    )
}

#[must_use]
pub fn marks_on(
    view: &PageView,
    page: usize,
    clusters: &[TextClusterBox],
    (search, marking): (&Search, Marking),
) -> PageWork {
    let mut work = PageWork {
        page,
        ..PageWork::default()
    };
    let Ok(device) = desk::device(view) else {
        work.left
            .push(format!("page {} has no size to mark on", page + 1));
        return work;
    };
    let scale = device
        .matrix
        .a
        .mul_add(device.matrix.d, -(device.matrix.b * device.matrix.c))
        .abs()
        .sqrt();
    let places = boxes_by_place(clusters);
    for (index, semantic) in view.index.blocks.iter().enumerate() {
        let Some((block, parts)) = desk::read(view, page, index) else {
            continue;
        };
        let matches = matches_in(&parts.reading, search);
        for found in &matches.found {
            let mut rows: Vec<[f64; 4]> = Vec::new();
            for row in found.from.0..=found.to.0 {
                let Some(line) = semantic.lines.get(row) else {
                    continue;
                };
                let stops = parts
                    .reading
                    .lines
                    .get(row)
                    .map_or(0, |read| read.clusters.len());
                let first = if row == found.from.0 { found.from.1 } else { 0 };
                let last = if row == found.to.0 { found.to.1 } else { stops };
                let boxed = (first..last)
                    .filter_map(|stop| places.get(&(*line, stop)).copied())
                    .reduce(joined);
                rows.extend(boxed);
            }
            if rows.is_empty() {
                work.left.push(format!(
                    "a match in {} has no place on the page to mark",
                    block.name()
                ));
                continue;
            }
            work.found += 1;
            let em = block.size.max(1.0);
            for boxed in rows {
                let (steps, stroke) = stroke_across(boxed, em, marking);
                let Ok(steps) = desk::steps_in_user_space_with(&device.matrix, &steps) else {
                    continue;
                };
                work.commands.push(Command::DrawPath {
                    page_index: page,
                    steps,
                    closed: false,
                    stroke: Some(PenStroke {
                        width: stroke.width / scale.max(f64::MIN_POSITIVE),
                        ..stroke
                    }),
                    fill: None,
                });
            }
        }
    }
    work
}

#[cfg(test)]
mod tests;
