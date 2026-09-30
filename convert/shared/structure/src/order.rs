use crate::model::Rect;

#[must_use]
pub fn reading_order(rects: &[Rect], gutter: f64) -> Vec<usize> {
    let mut out = Vec::with_capacity(rects.len());
    let ids: Vec<usize> = (0..rects.len()).collect();
    cut(rects, ids, gutter, &mut out, 0);
    out
}

fn gaps(rects: &[Rect], ids: &[usize], vertical_cut: bool) -> Vec<(f64, f64)> {
    let mut spans: Vec<(f64, f64)> = ids
        .iter()
        .map(|&i| {
            let r = &rects[i];
            if vertical_cut {
                (r.x0, r.x1)
            } else {
                (r.y0, r.y1)
            }
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut reach = f64::NEG_INFINITY;
    for (start, end) in spans {
        if reach > f64::NEG_INFINITY && start > reach {
            out.push((reach, start));
        }
        reach = reach.max(end);
    }
    out
}

fn split(rects: &[Rect], ids: &[usize], vertical_cut: bool, at: &[(f64, f64)]) -> Vec<Vec<usize>> {
    let mut parts = vec![Vec::new(); at.len() + 1];
    for &i in ids {
        let r = &rects[i];
        let start = if vertical_cut { r.x0 } else { r.y0 };
        let part = at.iter().take_while(|gap| start >= gap.1).count();
        parts[part].push(i);
    }
    parts.retain(|part| !part.is_empty());
    parts
}

fn extent(rects: &[Rect], ids: &[usize]) -> Rect {
    ids.iter()
        .skip(1)
        .fold(rects[ids[0]], |acc, &i| acc.union(&rects[i]))
}

fn cut(rects: &[Rect], ids: Vec<usize>, gutter: f64, out: &mut Vec<usize>, depth: usize) {
    if ids.len() <= 1 || depth > 64 {
        let mut ids = ids;
        ids.sort_by(|&a, &b| {
            (rects[a].y0, rects[a].x0)
                .partial_cmp(&(rects[b].y0, rects[b].x0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.extend(ids);
        return;
    }
    let region = extent(rects, &ids);
    let columns: Vec<(f64, f64)> = gaps(rects, &ids, true)
        .into_iter()
        .filter(|(a, b)| b - a >= gutter)
        .collect();
    if !columns.is_empty() {
        let parts = split(rects, &ids, true, &columns);
        let tall = parts
            .iter()
            .all(|part| extent(rects, part).height() >= 0.35 * region.height());
        if tall && parts.len() > 1 {
            for part in parts {
                cut(rects, part, gutter, out, depth + 1);
            }
            return;
        }
    }
    let rows = gaps(rects, &ids, false);
    if !rows.is_empty() {
        let bands = split(rects, &ids, false, &rows);
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut previous: Vec<(f64, f64)> = Vec::new();
        for band in bands {
            let gutters: Vec<(f64, f64)> = gaps(rects, &band, true)
                .into_iter()
                .filter(|(a, b)| b - a >= gutter)
                .collect();
            let shared = gutters.iter().any(|(a, b)| {
                previous
                    .iter()
                    .any(|(c, d)| b.min(*d) - a.max(*c) >= gutter)
            });
            match groups.last_mut() {
                Some(group) if shared => group.extend(band),
                _ => groups.push(band),
            }
            previous = gutters;
        }
        if groups.len() > 1 {
            for group in groups {
                cut(rects, group, gutter, out, depth + 1);
            }
            return;
        }
    }
    let any_columns = gaps(rects, &ids, true);
    if !any_columns.is_empty() {
        let parts = split(rects, &ids, true, &any_columns);
        if parts.len() > 1 {
            for part in parts {
                cut(rects, part, gutter, out, depth + 1);
            }
            return;
        }
    }
    cut(rects, ids, gutter, out, usize::MAX - 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect::new(x0, y0, x1, y1)
    }

    #[test]
    fn single_column_top_to_bottom() {
        let rects = [
            r(0.0, 50.0, 100.0, 60.0),
            r(0.0, 0.0, 100.0, 10.0),
            r(0.0, 20.0, 100.0, 40.0),
        ];
        assert_eq!(reading_order(&rects, 12.0), vec![1, 2, 0]);
    }

    #[test]
    fn two_columns_under_a_title_are_not_interleaved() {
        let rects = [
            r(0.0, 0.0, 300.0, 20.0),
            r(0.0, 30.0, 140.0, 100.0),
            r(160.0, 30.0, 300.0, 100.0),
            r(0.0, 110.0, 140.0, 200.0),
            r(160.0, 110.0, 300.0, 200.0),
        ];
        assert_eq!(reading_order(&rects, 12.0), vec![0, 1, 3, 2, 4]);
    }

    #[test]
    fn a_label_beside_its_value_reads_left_then_right() {
        let rects = [r(200.0, 0.0, 300.0, 10.0), r(0.0, 0.0, 100.0, 10.0)];
        assert_eq!(reading_order(&rects, 12.0), vec![1, 0]);
    }
}
