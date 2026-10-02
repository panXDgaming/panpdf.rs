use crate::model::Rect;
use crate::rules::{Fill, Rule};

const TOUCH: f64 = 2.0;
const SNAP: f64 = 2.5;
const COVER: f64 = 0.6;

#[derive(Clone, Debug)]
pub struct Grid {
    pub frame: Rect,
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    pub owner: Vec<Vec<(usize, usize)>>,
    pub fills: Vec<Vec<Option<[u8; 3]>>>,
}

impl Grid {
    #[must_use]
    pub fn rows(&self) -> usize {
        self.ys.len() - 1
    }

    #[must_use]
    pub fn columns(&self) -> usize {
        self.xs.len() - 1
    }

    #[must_use]
    pub fn cell_at(&self, (x, y): (f64, f64)) -> Option<(usize, usize)> {
        if !self.frame.contains((x, y)) {
            return None;
        }
        let column = self.xs.windows(2).position(|w| x >= w[0] && x <= w[1])?;
        let row = self.ys.windows(2).position(|w| y >= w[0] && y <= w[1])?;
        Some(self.owner[row][column])
    }

    #[must_use]
    pub fn span(&self, row: usize, column: usize) -> (usize, usize) {
        let mut columns = 0;
        let mut rows = 0;
        for (r, owners) in self.owner.iter().enumerate() {
            for (c, owner) in owners.iter().enumerate() {
                if *owner == (row, column) {
                    columns = columns.max(c + 1 - column);
                    rows = rows.max(r + 1 - row);
                }
            }
        }
        (columns.max(1), rows.max(1))
    }
}

struct Sets {
    parent: Vec<usize>,
}

impl Sets {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut a: usize) -> usize {
        while self.parent[a] != a {
            self.parent[a] = self.parent[self.parent[a]];
            a = self.parent[a];
        }
        a
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[b.max(a)] = a.min(b);
        }
    }
}

fn crosses(h: &Rule, v: &Rule) -> bool {
    v.at >= h.from - TOUCH && v.at <= h.to + TOUCH && h.at >= v.from - TOUCH && h.at <= v.to + TOUCH
}

fn snap(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    let mut out: Vec<(f64, usize)> = Vec::new();
    for value in values {
        match out.last_mut() {
            Some((sum, count))
                if value - *sum / f64::from(u32::try_from(*count).unwrap_or(1)) <= SNAP =>
            {
                *sum += value;
                *count += 1;
            }
            _ => out.push((value, 1)),
        }
    }
    out.into_iter()
        .map(|(sum, count)| sum / f64::from(u32::try_from(count).unwrap_or(1)))
        .collect()
}

fn covered(rules: &[&Rule], at: f64, from: f64, to: f64) -> f64 {
    let mut pieces: Vec<(f64, f64)> = rules
        .iter()
        .filter(|rule| (rule.at - at).abs() <= SNAP)
        .map(|rule| (rule.from.max(from), rule.to.min(to)))
        .filter(|(a, b)| b > a)
        .collect();
    pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut total = 0.0;
    let mut reach = from;
    for (a, b) in pieces {
        let a = a.max(reach);
        if b > a {
            total += b - a;
            reach = b;
        }
    }
    total
}

#[must_use]
pub fn find(rules: &[Rule], fills: &[Fill]) -> Vec<Grid> {
    let mut sets = Sets::new(rules.len());
    for (i, a) in rules.iter().enumerate() {
        for (j, b) in rules.iter().enumerate().skip(i + 1) {
            let meet = match (a.horizontal, b.horizontal) {
                (true, false) => crosses(a, b),
                (false, true) => crosses(b, a),
                _ => false,
            };
            if meet {
                sets.join(i, j);
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    for i in 0..rules.len() {
        let root = sets.find(i);
        match roots.iter().position(|r| *r == root) {
            Some(at) => groups[at].push(i),
            None => {
                roots.push(root);
                groups.push(vec![i]);
            }
        }
    }
    let mut grids = Vec::new();
    for group in groups {
        let members: Vec<&Rule> = group.iter().map(|i| &rules[*i]).collect();
        let horizontal: Vec<&Rule> = members.iter().copied().filter(|r| r.horizontal).collect();
        let vertical: Vec<&Rule> = members.iter().copied().filter(|r| !r.horizontal).collect();
        if horizontal.len() < 2 || vertical.len() < 2 {
            continue;
        }
        let x0 = members
            .iter()
            .map(|r| if r.horizontal { r.from } else { r.at })
            .fold(f64::INFINITY, f64::min);
        let x1 = members
            .iter()
            .map(|r| if r.horizontal { r.to } else { r.at })
            .fold(f64::NEG_INFINITY, f64::max);
        let y0 = members
            .iter()
            .map(|r| if r.horizontal { r.at } else { r.from })
            .fold(f64::INFINITY, f64::min);
        let y1 = members
            .iter()
            .map(|r| if r.horizontal { r.at } else { r.to })
            .fold(f64::NEG_INFINITY, f64::max);
        let frame = Rect::new(x0, y0, x1, y1);
        let mut xs: Vec<f64> = vertical.iter().map(|r| r.at).collect();
        xs.extend([x0, x1]);
        let xs = snap(xs);
        let mut ys: Vec<f64> = horizontal.iter().map(|r| r.at).collect();
        ys.extend([y0, y1]);
        let ys = snap(ys);
        if xs.len() < 3 || ys.len() < 3 {
            continue;
        }
        let rows = ys.len() - 1;
        let columns = xs.len() - 1;
        let mut cells = Sets::new(rows * columns);
        for r in 0..rows {
            for c in 0..columns {
                if c + 1 < columns {
                    let length = ys[r + 1] - ys[r];
                    if covered(&vertical, xs[c + 1], ys[r], ys[r + 1]) < COVER * length {
                        cells.join(r * columns + c, r * columns + c + 1);
                    }
                }
                if r + 1 < rows {
                    let length = xs[c + 1] - xs[c];
                    if covered(&horizontal, ys[r + 1], xs[c], xs[c + 1]) < COVER * length {
                        cells.join(r * columns + c, (r + 1) * columns + c);
                    }
                }
            }
        }
        let mut owner = vec![vec![(0, 0); columns]; rows];
        let mut boxes: Vec<(usize, [usize; 4])> = Vec::new();
        for r in 0..rows {
            for c in 0..columns {
                let root = cells.find(r * columns + c);
                match boxes.iter_mut().find(|(id, _)| *id == root) {
                    Some((_, b)) => {
                        b[0] = b[0].min(r);
                        b[1] = b[1].min(c);
                        b[2] = b[2].max(r);
                        b[3] = b[3].max(c);
                    }
                    None => boxes.push((root, [r, c, r, c])),
                }
            }
        }
        let mut claimed = vec![vec![false; columns]; rows];
        for (_, [r0, c0, r1, c1]) in &boxes {
            for (r, row) in claimed.iter_mut().enumerate().take(r1 + 1).skip(*r0) {
                for (c, slot) in row.iter_mut().enumerate().take(c1 + 1).skip(*c0) {
                    if !*slot {
                        *slot = true;
                        owner[r][c] = (*r0, *c0);
                    }
                }
            }
        }
        let fills = (0..rows)
            .map(|r| {
                (0..columns)
                    .map(|c| {
                        let cell = Rect::new(xs[c], ys[r], xs[c + 1], ys[r + 1]);
                        fills
                            .iter()
                            .filter(|fill| fill.color.iter().any(|v| *v < 245))
                            .find(|fill| fill.rect.overlap(&cell) >= 0.8 * cell.area())
                            .map(|fill| fill.color)
                    })
                    .collect()
            })
            .collect();
        grids.push(Grid {
            frame,
            xs,
            ys,
            owner,
            fills,
        });
    }
    grids
}

#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub rect: Rect,
    pub baseline: f64,
    pub em: f64,
    pub chars: usize,
}

#[must_use]
pub fn find_stream(pieces: &[Piece]) -> Vec<Grid> {
    let mut sorted: Vec<Piece> = pieces.to_vec();
    sorted.sort_by(|a, b| {
        a.baseline
            .total_cmp(&b.baseline)
            .then(a.rect.x0.total_cmp(&b.rect.x0))
    });
    let mut rows: Vec<Vec<Piece>> = Vec::new();
    for piece in sorted {
        match rows.last_mut() {
            Some(row)
                if (piece.baseline - row[0].baseline).abs()
                    <= 0.4 * piece.em.max(row[0].em).max(1.0) =>
            {
                row.push(piece);
            }
            _ => rows.push(vec![piece]),
        }
    }
    for row in &mut rows {
        row.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    }
    let mut grids = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        let mut end = start;
        while end < rows.len() && rows[end].len() >= 2 {
            if end > start {
                let above = rect_of(&rows[end - 1]);
                let here = rect_of(&rows[end]);
                let em = rows[end][0].em.max(1.0);
                if here.y0 - above.y1 > 2.5 * em {
                    break;
                }
            }
            end += 1;
        }
        if end - start >= 3
            && let Some(grid) = stream_grid(&rows[start..end])
        {
            grids.push(grid);
            start = end;
            continue;
        }
        start = end.max(start + 1);
    }
    grids
}

fn rect_of(pieces: &[Piece]) -> Rect {
    pieces
        .iter()
        .skip(1)
        .fold(pieces[0].rect, |r, p| r.union(&p.rect))
}

fn stream_grid(rows: &[Vec<Piece>]) -> Option<Grid> {
    let all: Vec<&Piece> = rows.iter().flatten().collect();
    let chars: usize = all.iter().map(|p| p.chars).sum();
    let longest = all.iter().map(|p| p.chars).max().unwrap_or(0);
    if chars > 25 * all.len() || longest > 60 {
        return None;
    }
    let em = all.iter().map(|p| p.em).fold(0.0, f64::max).max(1.0);
    let mut spans: Vec<(f64, f64)> = all.iter().map(|p| (p.rect.x0, p.rect.x1)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut columns: Vec<(f64, f64)> = Vec::new();
    for (a, b) in spans {
        match columns.last_mut() {
            Some(last) if a <= last.1 + 0.8 * em => last.1 = last.1.max(b),
            _ => columns.push((a, b)),
        }
    }
    if columns.len() < 2 {
        return None;
    }
    let column_of = |p: &Piece| {
        columns
            .iter()
            .position(|(a, b)| p.rect.x0 >= *a - 0.1 && p.rect.x1 <= *b + 0.1)
    };
    let spread = rows
        .iter()
        .filter(|row| {
            let mut seen: Vec<usize> = row.iter().filter_map(&column_of).collect();
            seen.sort_unstable();
            seen.dedup();
            seen.len() >= 2
        })
        .count();
    if spread * 10 < rows.len() * 6 {
        return None;
    }
    let frame = rect_of(&all.iter().map(|p| **p).collect::<Vec<_>>());
    let mut xs = vec![frame.x0];
    for pair in columns.windows(2) {
        xs.push(f64::midpoint(pair[0].1, pair[1].0));
    }
    xs.push(frame.x1);
    let boxes: Vec<Rect> = rows.iter().map(|row| rect_of(row)).collect();
    let mut ys = vec![frame.y0];
    for pair in boxes.windows(2) {
        ys.push(f64::midpoint(pair[0].y1, pair[1].y0));
    }
    ys.push(frame.y1);
    let (r, c) = (ys.len() - 1, xs.len() - 1);
    Some(Grid {
        frame,
        xs,
        ys,
        owner: (0..r)
            .map(|row| (0..c).map(|column| (row, column)).collect())
            .collect(),
        fills: vec![vec![None; c]; r],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(at: f64, from: f64, to: f64) -> Rule {
        Rule {
            horizontal: true,
            at,
            from,
            to,
        }
    }

    fn v(at: f64, from: f64, to: f64) -> Rule {
        Rule {
            horizontal: false,
            at,
            from,
            to,
        }
    }

    #[test]
    fn a_full_grid_of_two_by_three() {
        let rules = vec![
            h(0.0, 0.0, 300.0),
            h(20.0, 0.0, 300.0),
            h(40.0, 0.0, 300.0),
            v(0.0, 0.0, 40.0),
            v(100.0, 0.0, 40.0),
            v(200.0, 0.0, 40.0),
            v(300.0, 0.0, 40.0),
        ];
        let grids = find(&rules, &[]);
        assert_eq!(grids.len(), 1);
        let grid = &grids[0];
        assert_eq!((grid.rows(), grid.columns()), (2, 3));
        assert_eq!(grid.cell_at((150.0, 30.0)), Some((1, 1)));
        assert_eq!(grid.span(0, 0), (1, 1));
    }

    #[test]
    fn a_missing_rule_merges_a_header() {
        let rules = vec![
            h(0.0, 0.0, 300.0),
            h(20.0, 0.0, 300.0),
            h(40.0, 0.0, 300.0),
            v(0.0, 0.0, 40.0),
            v(100.0, 20.0, 40.0),
            v(200.0, 0.0, 40.0),
            v(300.0, 0.0, 40.0),
        ];
        let grid = &find(&rules, &[])[0];
        assert_eq!(grid.span(0, 0), (2, 1));
        assert_eq!(grid.cell_at((150.0, 10.0)), Some((0, 0)));
        assert_eq!(grid.cell_at((150.0, 30.0)), Some((1, 1)));
    }

    fn piece(x0: f64, x1: f64, baseline: f64, chars: usize) -> Piece {
        Piece {
            rect: Rect::new(x0, baseline - 9.0, x1, baseline + 2.0),
            baseline,
            em: 10.0,
            chars,
        }
    }

    #[test]
    fn aligned_short_cells_make_a_table() {
        let mut pieces = Vec::new();
        for row in 0..4 {
            let y = 100.0 + 14.0 * f64::from(row);
            pieces.push(piece(50.0, 90.0, y, 5));
            pieces.push(piece(150.0, 175.0, y, 3));
            pieces.push(piece(250.0, 280.0, y, 4));
        }
        let grids = find_stream(&pieces);
        assert_eq!(grids.len(), 1);
        assert_eq!((grids[0].rows(), grids[0].columns()), (4, 3));
    }

    #[test]
    fn two_columns_of_prose_are_not_a_table() {
        let mut pieces = Vec::new();
        for row in 0..6 {
            let y = 100.0 + 14.0 * f64::from(row);
            pieces.push(piece(50.0, 280.0, y, 55));
            pieces.push(piece(300.0, 530.0, y, 58));
        }
        assert!(find_stream(&pieces).is_empty());
    }

    #[test]
    fn a_box_is_not_a_table() {
        let rules = vec![
            h(0.0, 0.0, 100.0),
            h(50.0, 0.0, 100.0),
            v(0.0, 0.0, 50.0),
            v(100.0, 0.0, 50.0),
        ];
        assert!(find(&rules, &[]).is_empty());
    }

    #[test]
    fn separate_tables_stay_separate() {
        let mut rules = Vec::new();
        for top in [0.0, 100.0] {
            rules.extend([
                h(top, 0.0, 200.0),
                h(top + 20.0, 0.0, 200.0),
                h(top + 40.0, 0.0, 200.0),
                v(0.0, top, top + 40.0),
                v(100.0, top, top + 40.0),
                v(200.0, top, top + 40.0),
            ]);
        }
        assert_eq!(find(&rules, &[]).len(), 2);
    }
}
