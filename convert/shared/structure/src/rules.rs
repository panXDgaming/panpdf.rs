use pdf_paint::{Matrix, PaintAtomKind, PaintGraph, PathSegment, Point};

use crate::geometry::Shown;
use crate::text::rgb;

const MOST_THICKNESS: f64 = 2.5;
const LEAST_LENGTH: f64 = 3.0;
const LEAN: f64 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    pub horizontal: bool,
    pub at: f64,
    pub from: f64,
    pub to: f64,
}

impl Rule {
    #[must_use]
    pub fn length(&self) -> f64 {
        self.to - self.from
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fill {
    pub rect: crate::model::Rect,
    pub color: [u8; 3],
}

fn transform(matrix: Matrix, point: Point) -> Point {
    matrix.transform(point)
}

fn is_white(color: [u8; 3]) -> bool {
    color.iter().all(|c| *c >= 245)
}

#[must_use]
pub fn collect(graph: &PaintGraph, shown: &Shown) -> (Vec<Rule>, Vec<Fill>) {
    let mut rules = Vec::new();
    let mut fills = Vec::new();
    for atom in &graph.atoms {
        let PaintAtomKind::Path(paint) = &atom.kind else {
            continue;
        };
        let ctm = paint.state.ctm.value;
        let scale = (ctm.a * ctm.d - ctm.b * ctm.c).abs().sqrt();
        let stroke_width = paint.state.line_width.value * scale;
        let stroke_color = rgb(
            &paint.state.stroke_color.value,
            &paint.state.stroke_color_space.value,
        );
        let fill_color = rgb(
            &paint.state.fill_color.value,
            &paint.state.fill_color_space.value,
        );
        let stroked =
            paint.stroke && stroke_width <= MOST_THICKNESS * 2.0 && !is_white(stroke_color);
        let filled = paint.fill.is_some();
        for polygon in polygons(&paint.path, ctm) {
            let shown_points: Vec<(f64, f64)> = polygon
                .points
                .iter()
                .map(|p| shown.point(p.x, p.y))
                .collect();
            if stroked {
                let count = shown_points.len();
                let edges = if polygon.closed {
                    count
                } else {
                    count.saturating_sub(1)
                };
                for at in 0..edges {
                    let a = shown_points[at];
                    let b = shown_points[(at + 1) % count];
                    if let Some(rule) = rule_of_segment(a, b) {
                        rules.push(rule);
                    }
                }
            }
            if filled && let Some(rect) = axis_rectangle(&shown_points) {
                let thin = rect.width().min(rect.height());
                let long = rect.width().max(rect.height());
                if thin <= MOST_THICKNESS && long >= LEAST_LENGTH {
                    if !is_white(fill_color) {
                        rules.push(if rect.width() >= rect.height() {
                            Rule {
                                horizontal: true,
                                at: (rect.y0 + rect.y1) / 2.0,
                                from: rect.x0,
                                to: rect.x1,
                            }
                        } else {
                            Rule {
                                horizontal: false,
                                at: (rect.x0 + rect.x1) / 2.0,
                                from: rect.y0,
                                to: rect.y1,
                            }
                        });
                    }
                } else if thin > MOST_THICKNESS {
                    fills.push(Fill {
                        rect,
                        color: fill_color,
                    });
                }
            }
        }
    }
    (merge(rules), fills)
}

struct Polygon {
    points: Vec<Point>,
    closed: bool,
}

fn polygons(path: &pdf_paint::Path, ctm: Matrix) -> Vec<Polygon> {
    let mut out = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let mut curved = false;
    let flush =
        |current: &mut Vec<Point>, curved: &mut bool, closed: bool, out: &mut Vec<Polygon>| {
            if !*curved && current.len() >= 2 {
                out.push(Polygon {
                    points: std::mem::take(current),
                    closed,
                });
            }
            current.clear();
            *curved = false;
        };
    for segment in &path.segments {
        match segment {
            PathSegment::MoveTo { point, .. } => {
                flush(&mut current, &mut curved, false, &mut out);
                current.push(transform(ctm, *point));
            }
            PathSegment::LineTo { point, .. } => current.push(transform(ctm, *point)),
            PathSegment::CubicTo { end, .. } => {
                curved = true;
                current.push(transform(ctm, *end));
            }
            PathSegment::ClosePath { .. } => {
                let start = current.first().copied();
                flush(&mut current, &mut curved, true, &mut out);
                if let Some(start) = start {
                    current.push(start);
                }
            }
            PathSegment::Rectangle {
                origin,
                width,
                height,
                ..
            } => {
                flush(&mut current, &mut curved, false, &mut out);
                let corners = [
                    *origin,
                    Point {
                        x: origin.x + width,
                        y: origin.y,
                    },
                    Point {
                        x: origin.x + width,
                        y: origin.y + height,
                    },
                    Point {
                        x: origin.x,
                        y: origin.y + height,
                    },
                ];
                out.push(Polygon {
                    points: corners.iter().map(|p| transform(ctm, *p)).collect(),
                    closed: true,
                });
                current.push(transform(ctm, *origin));
            }
        }
    }
    flush(&mut current, &mut curved, false, &mut out);
    out
}

fn rule_of_segment(a: (f64, f64), b: (f64, f64)) -> Option<Rule> {
    let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    if dx >= LEAST_LENGTH && dy <= LEAN {
        Some(Rule {
            horizontal: true,
            at: (a.1 + b.1) / 2.0,
            from: a.0.min(b.0),
            to: a.0.max(b.0),
        })
    } else if dy >= LEAST_LENGTH && dx <= LEAN {
        Some(Rule {
            horizontal: false,
            at: (a.0 + b.0) / 2.0,
            from: a.1.min(b.1),
            to: a.1.max(b.1),
        })
    } else {
        None
    }
}

fn axis_rectangle(points: &[(f64, f64)]) -> Option<crate::model::Rect> {
    let mut points = points.to_vec();
    if points.len() == 5
        && (points[0].0 - points[4].0).abs() < 0.01
        && (points[0].1 - points[4].1).abs() < 0.01
    {
        points.pop();
    }
    if points.len() != 4 {
        return None;
    }
    let xs: Vec<f64> = points.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
    let (x0, x1) = (
        xs.iter().copied().fold(f64::INFINITY, f64::min),
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    let (y0, y1) = (
        ys.iter().copied().fold(f64::INFINITY, f64::min),
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    let on_edge = points.iter().all(|p| {
        ((p.0 - x0).abs() < 0.5 || (p.0 - x1).abs() < 0.5)
            && ((p.1 - y0).abs() < 0.5 || (p.1 - y1).abs() < 0.5)
    });
    on_edge.then(|| crate::model::Rect::new(x0, y0, x1, y1))
}

#[must_use]
pub fn merge(mut rules: Vec<Rule>) -> Vec<Rule> {
    rules.sort_by(|a, b| {
        (a.horizontal, a.at, a.from)
            .partial_cmp(&(b.horizontal, b.at, b.from))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out: Vec<Rule> = Vec::with_capacity(rules.len());
    for rule in rules {
        if let Some(last) = out.last_mut()
            && last.horizontal == rule.horizontal
            && (last.at - rule.at).abs() <= 1.0
            && rule.from <= last.to + 1.5
        {
            last.to = last.to.max(rule.to);
            continue;
        }
        out.push(rule);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_become_rules_only_when_level_and_long() {
        assert!(rule_of_segment((0.0, 10.0), (100.0, 10.3)).is_some_and(|r| r.horizontal));
        assert!(rule_of_segment((5.0, 0.0), (5.0, 50.0)).is_some_and(|r| !r.horizontal));
        assert!(rule_of_segment((0.0, 0.0), (50.0, 50.0)).is_none());
        assert!(rule_of_segment((0.0, 0.0), (1.0, 0.0)).is_none());
    }

    #[test]
    fn touching_rules_merge() {
        let merged = merge(vec![
            Rule {
                horizontal: true,
                at: 10.0,
                from: 0.0,
                to: 50.0,
            },
            Rule {
                horizontal: true,
                at: 10.2,
                from: 50.5,
                to: 90.0,
            },
            Rule {
                horizontal: true,
                at: 30.0,
                from: 0.0,
                to: 90.0,
            },
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].to, 90.0);
    }

    #[test]
    fn rectangles_are_recognised() {
        let rect = axis_rectangle(&[(0.0, 0.0), (10.0, 0.0), (10.0, 1.0), (0.0, 1.0)]);
        assert_eq!(rect, Some(crate::model::Rect::new(0.0, 0.0, 10.0, 1.0)));
        assert!(axis_rectangle(&[(0.0, 0.0), (10.0, 3.0), (10.0, 1.0), (0.0, 1.0)]).is_none());
    }
}
