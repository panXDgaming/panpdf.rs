use std::rc::Rc;

use crate::fonts::FontBook;
use crate::model::{
    Align, Block, Border, Cell, Document, FIELD_PAGE, FIELD_PAGES, Group, Inline, PageSetup,
    Paragraph, Section, Sides, Table, TableWidth, VAlign, VMerge,
};
use crate::page::{Item, LaidPage};
use crate::paragraph::lay_out;

#[derive(Clone, Debug, Default)]
pub struct Unit {
    pub height: f32,
    pub items: Vec<Item>,
    pub reach: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub keep_with_next: bool,
    pub page_break: bool,
    pub repeat: Option<Rc<Vec<Unit>>>,
    pub bands: Vec<Band>,
    pub notes: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Band {
    pub x: f32,
    pub width: f32,
    pub fill: Option<crate::model::Rgb>,
    pub left: Option<Border>,
    pub right: Option<Border>,
    pub top: Option<(Border, f32)>,
    pub bottom: Option<(Border, f32)>,
    pub pad_top: f32,
    pub pad_bottom: f32,
}

fn moved(items: &[Item], dx: f32, dy: f32) -> impl Iterator<Item = Item> + '_ {
    items.iter().map(move |item| item.clone().moved(dx, dy))
}

fn item_reach(item: &Item) -> f32 {
    match item {
        Item::Glyphs(run) => run.x + run.width,
        Item::Rect { x, width, .. }
        | Item::Image { x, width, .. }
        | Item::Link { x, width, .. } => x + width,
        Item::Line { x1, x2, .. } => x1.max(*x2),
    }
}

pub fn units_of(blocks: &[Block], width: f32, book: &FontBook) -> Vec<Unit> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph(p) => paragraph_units(p, width, book, &mut out),
            Block::Table(t) => table_units(t, width, book, &mut out),
            Block::Rule {
                width: thickness,
                colour,
                space_before,
                space_after,
            } => out.push(Unit {
                height: *thickness,
                items: vec![Item::Rect {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height: *thickness,
                    fill: Some(*colour),
                    stroke: None,
                }],
                reach: 0.0,
                space_before: *space_before,
                space_after: *space_after,
                ..Unit::default()
            }),
            Block::PageBreak | Block::SectionBreak(_) => out.push(Unit {
                page_break: true,
                ..Unit::default()
            }),
            Block::Group(group) => group_units(group, width, book, &mut out),
            Block::Columns { count, gap, blocks } => {
                column_units(blocks, *count, *gap, width, book, &mut out);
            }
        }
    }
    out
}

fn paragraph_units(p: &Paragraph, width: f32, book: &FontBook, out: &mut Vec<Unit>) {
    if p.page_break_before {
        out.push(Unit {
            page_break: true,
            ..Unit::default()
        });
    }
    let lines = lay_out(p, width, book);
    let count = lines.len();
    for (index, line) in lines.into_iter().enumerate() {
        let reach = line.items.iter().map(item_reach).fold(0.0, f32::max);
        out.push(Unit {
            height: line.height,
            items: line.items,
            reach,
            space_before: if index == 0 { p.space_before } else { 0.0 },
            space_after: if index + 1 == count {
                p.space_after
            } else {
                0.0
            },
            keep_with_next: (index + 1 < count && (index == 0 || index + 2 == count))
                || (index + 1 == count && p.keep_with_next),
            notes: line.notes,
            ..Unit::default()
        });
    }
}

fn group_units(g: &Group, width: f32, book: &FontBook, out: &mut Vec<Unit>) {
    let bl = g.borders.left.map_or(0.0, |b| b.width);
    let br = g.borders.right.map_or(0.0, |b| b.width);
    let inner_left = g.margin_left + bl + g.padding.left;
    let inner_width = (width - inner_left - g.margin_right - br - g.padding.right).max(1.0);
    let mut inner = units_of(&g.blocks, inner_width, book);
    if inner.is_empty() {
        inner.push(Unit::default());
    }
    let count = inner.len();
    for (index, mut unit) in inner.into_iter().enumerate() {
        unit.items = moved(&unit.items, inner_left, 0.0).collect();
        for band in &mut unit.bands {
            band.x += inner_left;
        }
        unit.reach += inner_left;
        let first = index == 0;
        let last = index + 1 == count;
        unit.bands.insert(
            0,
            Band {
                x: g.margin_left,
                width: width - g.margin_left - g.margin_right,
                fill: g.background,
                left: g.borders.left,
                right: g.borders.right,
                top: if first {
                    g.borders.top.map(|b| (b, 0.0))
                } else {
                    None
                },
                bottom: if last {
                    g.borders.bottom.map(|b| (b, 0.0))
                } else {
                    None
                },
                pad_top: if first {
                    g.padding.top + g.borders.top.map_or(0.0, |b| b.width)
                } else {
                    0.0
                },
                pad_bottom: if last {
                    g.padding.bottom + g.borders.bottom.map_or(0.0, |b| b.width)
                } else {
                    0.0
                },
            },
        );
        if first {
            unit.space_before = g.space_before;
        }
        if last {
            unit.space_after = g.space_after;
        }
        out.push(unit);
    }
}

fn column_units(
    blocks: &[Block],
    count: usize,
    gap: f32,
    width: f32,
    book: &FontBook,
    out: &mut Vec<Unit>,
) {
    let count = count.clamp(1, 12);
    #[allow(clippy::cast_precision_loss)]
    let n = count as f32;
    let column = ((width - gap * (n - 1.0)) / n).max(10.0);
    let units = units_of(blocks, column, book);
    let (_, total, _) = stack(&units, true);
    if count == 1 || units.iter().any(|u| u.page_break) || total / n > 600.0 {
        out.extend(units_of(blocks, width, book));
        return;
    }
    let target = total / n;
    let mut columns: Vec<Vec<Unit>> = vec![Vec::new()];
    let mut height = 0.0_f32;
    for unit in units {
        let h = unit_height(&unit) + unit.space_before;
        if height + h / 2.0 > target
            && columns.len() < count
            && !columns.last().is_some_and(Vec::is_empty)
        {
            columns.push(Vec::new());
            height = 0.0;
        }
        height += h;
        if let Some(last) = columns.last_mut() {
            last.push(unit);
        }
    }
    let mut unit = Unit::default();
    for (index, col) in columns.iter().enumerate() {
        let (items, h, _) = stack(col, true);
        #[allow(clippy::cast_precision_loss)]
        let x = index as f32 * (column + gap);
        unit.items.extend(moved(&items, x, 0.0));
        unit.height = unit.height.max(h);
        unit.notes
            .extend(col.iter().flat_map(|u| u.notes.iter().copied()));
    }
    unit.space_before = columns
        .first()
        .and_then(|c| c.first())
        .map_or(0.0, |u| u.space_before);
    unit.space_after = columns
        .iter()
        .filter_map(|c| c.last())
        .map(|u| u.space_after)
        .fold(0.0, f32::max);
    unit.reach = width;
    out.push(unit);
}

fn stack(units: &[Unit], collapse: bool) -> (Vec<Item>, f32, f32) {
    let mut items = Vec::new();
    let mut y = 0.0_f32;
    let mut after = 0.0_f32;
    let mut reach = 0.0_f32;
    for (index, unit) in units.iter().enumerate() {
        if unit.page_break {
            continue;
        }
        let gap = if index == 0 {
            unit.space_before
        } else if collapse {
            after.max(unit.space_before)
        } else {
            after + unit.space_before
        };
        let top = y + gap;
        place_bands(unit, top, gap, &mut items);
        items.extend(moved(&unit.items, 0.0, top + unit_pad_top(unit)));
        y = top + unit_height(unit);
        after = unit.space_after;
        reach = reach.max(unit.reach);
    }
    (items, y + after, reach)
}

fn unit_pad_top(unit: &Unit) -> f32 {
    unit.bands.iter().map(|b| b.pad_top).sum()
}

fn unit_height(unit: &Unit) -> f32 {
    unit.height
        + unit
            .bands
            .iter()
            .map(|b| b.pad_top + b.pad_bottom)
            .sum::<f32>()
}

fn place_bands(unit: &Unit, top: f32, gap: f32, items: &mut Vec<Item>) {
    let height = unit_height(unit);
    for band in &unit.bands {
        let (y, h) = if band.top.is_some() || band.pad_top > 0.0 {
            (top, height)
        } else {
            (top - gap, height + gap)
        };
        if let Some(fill) = band.fill {
            items.push(Item::Rect {
                x: band.x,
                y,
                width: band.width,
                height: h,
                fill: Some(fill),
                stroke: None,
            });
        }
        let line = |x1, y1, x2, y2, b: Border| Item::Line {
            x1,
            y1,
            x2,
            y2,
            width: b.width,
            colour: b.colour,
        };
        let right = band.x + band.width;
        if let Some(b) = band.left {
            items.push(line(
                band.x + b.width / 2.0,
                y,
                band.x + b.width / 2.0,
                y + h,
                b,
            ));
        }
        if let Some(b) = band.right {
            items.push(line(
                right - b.width / 2.0,
                y,
                right - b.width / 2.0,
                y + h,
                b,
            ));
        }
        if let Some((b, _)) = band.top {
            items.push(line(band.x, y + b.width / 2.0, right, y + b.width / 2.0, b));
        }
        if let Some((b, _)) = band.bottom {
            items.push(line(
                band.x,
                y + h - b.width / 2.0,
                right,
                y + h - b.width / 2.0,
                b,
            ));
        }
    }
}

fn content_widths(blocks: &[Block], book: &FontBook) -> (f32, f32) {
    let mut most = 0.0_f32;
    let mut least = 0.0_f32;
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                let mut left = p.clone();
                left.align = if p.is_rtl() {
                    Align::Right
                } else {
                    Align::Left
                };
                let p = &left;
                let wide = lay_out(p, 100_000.0, book);
                for line in &wide {
                    most = most.max(line.items.iter().map(item_reach).fold(0.0, f32::max));
                }
                let narrow = lay_out(p, 1.0, book);
                for line in &narrow {
                    let right = line.items.iter().map(item_reach).fold(0.0, f32::max);
                    least = least.max(right);
                }
            }
            Block::Table(t) => {
                let (lo, hi) = table_widths_hint(t, book);
                least = least.max(lo);
                most = most.max(hi);
            }
            Block::Group(g) => {
                let (hi, lo) = content_widths(&g.blocks, book);
                let extra = g.margin_left + g.margin_right + g.padding.left + g.padding.right;
                most = most.max(hi + extra);
                least = least.max(lo + extra);
            }
            _ => {}
        }
    }
    (most, least.min(most.max(least)))
}

fn table_widths_hint(t: &Table, book: &FontBook) -> (f32, f32) {
    if let TableWidth::Fixed(w) = t.width {
        return (w, w);
    }
    if !t.columns.is_empty() {
        let sum: f32 = t.columns.iter().sum();
        return (sum.min(200.0), sum);
    }
    let mut lo = 0.0_f32;
    let mut hi = 0.0_f32;
    for row in &t.rows {
        let mut row_lo = 0.0;
        let mut row_hi = 0.0;
        for cell in &row.cells {
            let (m, l) = content_widths(&cell.blocks, book);
            let pad = cell_padding(t, cell);
            row_hi += m + pad.left + pad.right;
            row_lo += l + pad.left + pad.right;
        }
        lo = lo.max(row_lo);
        hi = hi.max(row_hi);
    }
    (lo, hi)
}

fn cell_padding(t: &Table, cell: &Cell) -> Sides<f32> {
    cell.padding.unwrap_or(t.padding)
}

struct Placed<'a> {
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    cell: &'a Cell,
}

fn grid(t: &Table) -> (Vec<Placed<'_>>, usize) {
    let mut placed: Vec<Placed<'_>> = Vec::new();
    let mut taken: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    let mut columns = 0;
    for (r, row) in t.rows.iter().enumerate() {
        let mut c = 0;
        for cell in &row.cells {
            while taken.contains(&(r, c)) {
                c += 1;
            }
            let cols = cell.span.max(1);
            if cell.vmerge == VMerge::Continue {
                if let Some(above) = placed
                    .iter_mut()
                    .rev()
                    .find(|p| p.col == c && p.row + p.rows == r)
                {
                    above.rows += 1;
                }
                c += cols;
                columns = columns.max(c);
                continue;
            }
            let rows = cell.row_span.max(1).min(t.rows.len() - r);
            for dr in 1..rows {
                for dc in 0..cols {
                    taken.insert((r + dr, c + dc));
                }
            }
            placed.push(Placed {
                row: r,
                col: c,
                rows,
                cols,
                cell,
            });
            c += cols;
            columns = columns.max(c);
        }
    }
    (placed, columns)
}

#[allow(clippy::too_many_lines)]
fn table_units(t: &Table, avail: f32, book: &FontBook, out: &mut Vec<Unit>) {
    if t.rows.is_empty() {
        return;
    }
    let (placed, columns) = grid(t);
    if columns == 0 {
        return;
    }
    let avail = (avail - t.indent).max(10.0);
    #[allow(clippy::cast_precision_loss)]
    let n = columns as f32;
    let target = match t.width {
        TableWidth::Fixed(w) => Some(w.min(avail)),
        TableWidth::Percent(p) => Some((avail * p).min(avail)),
        _ => None,
    };
    let mut widths: Vec<f32> = if t.columns.len() == columns && t.width != TableWidth::Auto {
        let mut w = t.columns.clone();
        let sum: f32 = w.iter().sum();
        let want = target.unwrap_or(sum).min(avail);
        if sum > 0.0 && (sum - want).abs() > 0.5 {
            for x in &mut w {
                *x *= want / sum;
            }
        }
        w
    } else {
        let mut most = vec![0.0_f32; columns];
        let mut least = vec![0.0_f32; columns];
        let mut asked = vec![None::<f32>; columns];
        for p in &placed {
            if p.cols != 1 {
                continue;
            }
            let (m, l) = content_widths(&p.cell.blocks, book);
            let pad = cell_padding(t, p.cell);
            let extra = pad.left + pad.right + 1.0;
            most[p.col] = most[p.col].max(m + extra);
            least[p.col] = least[p.col].max(l + extra);
            if let Some(w) = p.cell.width {
                asked[p.col] = Some(asked[p.col].unwrap_or(0.0).max(w));
            }
        }
        for c in 0..columns {
            if let Some(w) = asked[c] {
                most[c] = w.max(least[c]);
                least[c] = least[c].max(w.min(most[c]));
            }
            if most[c] <= 0.0 {
                most[c] = 12.0;
                least[c] = least[c].max(6.0);
            }
        }
        for p in &placed {
            if p.cols < 2 {
                continue;
            }
            let (m, _) = content_widths(&p.cell.blocks, book);
            let pad = cell_padding(t, p.cell);
            let need = m + pad.left + pad.right + 1.0;
            let have: f32 = most[p.col..p.col + p.cols].iter().sum();
            if need > have {
                #[allow(clippy::cast_precision_loss)]
                let add = (need - have) / p.cols as f32;
                for x in &mut most[p.col..p.col + p.cols] {
                    *x += add;
                }
            }
        }
        let sum_most: f32 = most.iter().sum();
        let sum_least: f32 = least.iter().sum();
        let room = target.unwrap_or_else(|| sum_most.min(avail));
        if sum_most <= room {
            let extra = room - sum_most;
            most.iter()
                .map(|m| {
                    m + if sum_most > 0.0 {
                        extra * m / sum_most
                    } else {
                        extra / n
                    }
                })
                .collect()
        } else if sum_least >= room {
            least.iter().map(|l| l * room / sum_least).collect()
        } else {
            let spread = sum_most - sum_least;
            most.iter()
                .zip(&least)
                .map(|(m, l)| l + (room - sum_least) * (m - l) / spread.max(0.001))
                .collect()
        }
    };
    if widths.len() < columns {
        widths.resize(columns, avail / n);
    }
    let total: f32 = widths.iter().sum();
    let x0 = t.indent
        + match t.align {
            Align::Center => ((avail - total) / 2.0).max(0.0),
            Align::Right => (avail - total).max(0.0),
            _ => 0.0,
        };
    let col_x: Vec<f32> = std::iter::once(0.0)
        .chain(widths.iter().scan(0.0, |acc, w| {
            *acc += w;
            Some(*acc)
        }))
        .collect();

    struct Content {
        items: Vec<Item>,
        height: f32,
        notes: Vec<usize>,
    }
    let contents: Vec<Content> = placed
        .iter()
        .map(|p| {
            let pad = cell_padding(t, p.cell);
            let inner = (col_x[p.col + p.cols] - col_x[p.col] - pad.left - pad.right).max(1.0);
            let units = units_of(&p.cell.blocks, inner, book);
            let notes = units.iter().flat_map(|u| u.notes.iter().copied()).collect();
            let (items, height, _) = stack(&units, false);
            Content {
                items,
                height,
                notes,
            }
        })
        .collect();

    let mut heights: Vec<f32> = t
        .rows
        .iter()
        .map(|r| r.exact_height.unwrap_or(r.min_height))
        .collect();
    for (p, content) in placed.iter().zip(&contents) {
        if p.rows == 1 && t.rows[p.row].exact_height.is_none() {
            let pad = cell_padding(t, p.cell);
            heights[p.row] = heights[p.row].max(content.height + pad.top + pad.bottom);
        }
    }
    for (p, content) in placed.iter().zip(&contents) {
        if p.rows > 1 {
            let pad = cell_padding(t, p.cell);
            let need = content.height + pad.top + pad.bottom;
            let have: f32 = heights[p.row..p.row + p.rows].iter().sum();
            if need > have {
                heights[p.row + p.rows - 1] += need - have;
            }
        }
    }

    let mut rows: Vec<Unit> = (0..t.rows.len())
        .map(|r| Unit {
            height: heights[r],
            reach: x0 + total,
            ..Unit::default()
        })
        .collect();
    for (p, content) in placed.iter().zip(contents) {
        let pad = cell_padding(t, p.cell);
        let x = x0 + col_x[p.col];
        let w = col_x[p.col + p.cols] - col_x[p.col];
        let h: f32 = heights[p.row..p.row + p.rows].iter().sum();
        rows[p.row].notes.extend(content.notes.iter().copied());
        let items = &mut rows[p.row].items;
        if let Some(fill) = p.cell.background {
            items.insert(
                0,
                Item::Rect {
                    x,
                    y: 0.0,
                    width: w,
                    height: h,
                    fill: Some(fill),
                    stroke: None,
                },
            );
        }
        let free = h - pad.top - pad.bottom - content.height;
        let dy = pad.top
            + match p.cell.valign {
                VAlign::Top => 0.0,
                VAlign::Middle => (free / 2.0).max(0.0),
                VAlign::Bottom => free.max(0.0),
            };
        items.extend(moved(&content.items, x + pad.left, dy));
        let b = p.cell.borders;
        let line = |x1, y1, x2, y2, b: Border| Item::Line {
            x1,
            y1,
            x2,
            y2,
            width: b.width,
            colour: b.colour,
        };
        if let Some(b) = b.top {
            items.push(line(x, 0.0, x + w, 0.0, b));
        }
        if let Some(b) = b.bottom {
            items.push(line(x, h, x + w, h, b));
        }
        if let Some(b) = b.left {
            items.push(line(x, 0.0, x, h, b));
        }
        if let Some(b) = b.right {
            items.push(line(x + w, 0.0, x + w, h, b));
        }
    }
    if let Some(b) = t.outer_border {
        let last = rows.len() - 1;
        for (r, unit) in rows.iter_mut().enumerate() {
            let h = unit.height;
            let line = |x1, y1, x2, y2| Item::Line {
                x1,
                y1,
                x2,
                y2,
                width: b.width,
                colour: b.colour,
            };
            unit.items.push(line(x0, 0.0, x0, h));
            unit.items.push(line(x0 + total, 0.0, x0 + total, h));
            if r == 0 {
                unit.items.push(line(x0, 0.0, x0 + total, 0.0));
            }
            if r == last {
                unit.items.push(line(x0, h, x0 + total, h));
            }
        }
    }
    for p in &placed {
        for r in p.row..p.row + p.rows - 1 {
            rows[r].keep_with_next = true;
        }
    }
    let mut header_count = t.rows.iter().take_while(|r| r.header).count();
    if header_count > 0 {
        for p in &placed {
            if p.row < header_count {
                header_count = header_count.max(p.row + p.rows);
            }
        }
    }
    let header: Option<Rc<Vec<Unit>>> = (header_count > 0 && header_count < rows.len())
        .then(|| Rc::new(rows[..header_count].to_vec()));
    let count = rows.len();
    for (r, mut unit) in rows.into_iter().enumerate() {
        if r < header_count {
            unit.keep_with_next = true;
        } else {
            unit.repeat.clone_from(&header);
        }
        if r == 0 {
            unit.space_before = t.space_before;
        }
        if r + 1 == count {
            unit.space_after = t.space_after;
        }
        out.push(unit);
    }
}

fn with_fields(blocks: &[Block], page: usize, count: usize) -> Vec<Block> {
    let fill = |text: &str| {
        text.replace(FIELD_PAGE, &page.to_string())
            .replace(FIELD_PAGES, &count.to_string())
    };
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(p) => {
                let mut p = p.clone();
                for inline in &mut p.inlines {
                    if let Inline::Text { text, .. } = inline
                        && text.contains('\u{E000}')
                    {
                        *text = fill(text);
                    }
                }
                Block::Paragraph(p)
            }
            Block::Table(t) => {
                let mut t = t.clone();
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        cell.blocks = with_fields(&cell.blocks, page, count);
                    }
                }
                Block::Table(t)
            }
            Block::Group(g) => {
                let mut g = g.clone();
                g.blocks = with_fields(&g.blocks, page, count);
                Block::Group(g)
            }
            other => other.clone(),
        })
        .collect()
}

struct Filling {
    setup: PageSetup,
    section: usize,
    in_section: usize,
    items: Vec<Item>,
    notes: Vec<usize>,
}

const NOTE_SEPARATOR: f32 = 10.0;

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn paginate(doc: &Document, book: &FontBook) -> Vec<LaidPage> {
    let mut pages: Vec<Filling> = Vec::new();
    let mut laid_notes: std::collections::HashMap<(usize, u32), (Vec<Item>, f32)> =
        std::collections::HashMap::new();
    for (s, section) in doc.sections.iter().enumerate() {
        let setup = section.setup;
        let width = (setup.width - setup.margin.left - setup.margin.right).max(20.0);
        let columns = section.columns.max(1);
        #[allow(clippy::cast_precision_loss)]
        let n = columns as f32;
        let column = ((width - section.column_gap * (n - 1.0)) / n).max(20.0);
        let head_room = |blocks: &[Block]| {
            if blocks.is_empty() {
                return 0.0;
            }
            let units = units_of(&with_fields(blocks, 99, 99), width, book);
            stack(&units, doc.collapse_margins).1
        };
        let header_height =
            head_room(&section.header).max(section.first_header.as_deref().map_or(0.0, head_room));
        let footer_height =
            head_room(&section.footer).max(section.first_footer.as_deref().map_or(0.0, head_room));
        let top = setup
            .margin
            .top
            .max(setup.header_distance + header_height + 4.0);
        let bottom = setup.height
            - setup
                .margin
                .bottom
                .max(setup.footer_distance + footer_height + 4.0);
        let units = units_of(&section.blocks, column, book);
        let mut note_height = |note: usize| -> f32 {
            let key = (note, width.to_bits());
            if let Some((_, h)) = laid_notes.get(&key) {
                return *h;
            }
            let blocks = doc.notes.get(note).map_or(&[][..], Vec::as_slice);
            let (items, h, _) = stack(&units_of(blocks, width, book), doc.collapse_margins);
            laid_notes.insert(key, (items, h));
            h
        };
        let new_page = |pages: &mut Vec<Filling>| {
            let in_section = pages.iter().filter(|p| p.section == s).count();
            pages.push(Filling {
                setup,
                section: s,
                in_section,
                items: Vec::new(),
                notes: Vec::new(),
            });
        };
        new_page(&mut pages);
        let mut col = 0_usize;
        let mut y = top;
        let mut after = 0.0_f32;
        let mut first_on_page = true;
        let mut first_in_section = true;
        let mut notes_room = 0.0_f32;
        let mut index = 0;
        while index < units.len() {
            let unit = &units[index];
            #[allow(clippy::cast_precision_loss)]
            let x0 = setup.margin.left + col as f32 * (column + section.column_gap);
            if unit.page_break {
                if !first_on_page || !first_in_section {
                    new_page(&mut pages);
                    col = 0;
                    y = top;
                    notes_room = 0.0;
                    first_on_page = true;
                }
                after = 0.0;
                index += 1;
                continue;
            }
            let gap = if first_on_page {
                if first_in_section {
                    unit.space_before
                } else {
                    0.0
                }
            } else if doc.collapse_margins {
                after.max(unit.space_before)
            } else {
                after + unit.space_before
            };
            let mut brought: f32 = unit.notes.iter().map(|n| note_height(*n)).sum();
            if brought > 0.0 && notes_room == 0.0 {
                brought += NOTE_SEPARATOR;
            }
            let floor = bottom - notes_room - brought;
            let mut need = gap + unit_height(unit);
            let mut last = index;
            while units[last].keep_with_next && last + 1 < units.len() && need < bottom - top {
                last += 1;
                if units[last].page_break {
                    break;
                }
                need += units[last].space_before.max(units[last - 1].space_after)
                    + unit_height(&units[last]);
            }
            let fits_alone = y + gap + unit_height(unit) <= floor + 0.01;
            let fits_group = y + need <= floor + 0.01 || need > bottom - top;
            if !first_on_page && (!fits_alone || !fits_group) {
                if col + 1 < columns {
                    col += 1;
                } else {
                    new_page(&mut pages);
                    col = 0;
                    notes_room = 0.0;
                }
                y = top;
                first_on_page = true;
                first_in_section = false;
                after = 0.0;
                if let Some(header) = &unit.repeat {
                    #[allow(clippy::cast_precision_loss)]
                    let x = setup.margin.left + col as f32 * (column + section.column_gap);
                    if let Some(page) = pages.last_mut() {
                        for h in header.iter() {
                            let mut local = Vec::new();
                            place_bands(h, y, 0.0, &mut local);
                            page.items.extend(moved(&local, x, 0.0));
                            page.items.extend(moved(&h.items, x, y));
                            y += unit_height(h);
                        }
                    }
                    first_on_page = false;
                }
                continue;
            }
            let top_y = y + gap;
            if let Some(page) = pages.last_mut() {
                let mut local = Vec::new();
                place_bands(unit, top_y, gap, &mut local);
                page.items.extend(moved(&local, x0, 0.0));
                page.items
                    .extend(moved(&unit.items, x0, top_y + unit_pad_top(unit)));
                page.notes.extend(unit.notes.iter().copied());
            }
            notes_room += brought;
            y = top_y + unit_height(unit);
            after = unit.space_after;
            first_on_page = false;
            first_in_section = false;
            index += 1;
        }
    }
    let count = pages.len();
    let mut out = Vec::with_capacity(count);
    for (number, page) in pages.into_iter().enumerate() {
        let section: &Section = &doc.sections[page.section];
        let setup = page.setup;
        let width = (setup.width - setup.margin.left - setup.margin.right).max(20.0);
        let mut items = Vec::new();
        let first = page.in_section == 0;
        let header = if first {
            section.first_header.as_deref().unwrap_or(&section.header)
        } else {
            &section.header
        };
        let footer = if first {
            section.first_footer.as_deref().unwrap_or(&section.footer)
        } else {
            &section.footer
        };
        let mut footer_top = setup.height - setup.margin.bottom;
        if !header.is_empty() {
            let units = units_of(&with_fields(header, number + 1, count), width, book);
            let (h_items, _, _) = stack(&units, doc.collapse_margins);
            items.extend(moved(&h_items, setup.margin.left, setup.header_distance));
        }
        if !footer.is_empty() {
            let units = units_of(&with_fields(footer, number + 1, count), width, book);
            let (f_items, height, _) = stack(&units, doc.collapse_margins);
            let y = setup.height - setup.footer_distance - height;
            footer_top = footer_top.min(y - 4.0);
            items.extend(moved(&f_items, setup.margin.left, y));
        }
        items.extend(page.items);
        if !page.notes.is_empty() {
            let laid: Vec<&(Vec<Item>, f32)> = page
                .notes
                .iter()
                .filter_map(|n| laid_notes.get(&(*n, width.to_bits())))
                .collect();
            let total: f32 = laid.iter().map(|(_, h)| *h).sum();
            let mut y = footer_top - total;
            items.push(Item::Line {
                x1: setup.margin.left,
                y1: y - NOTE_SEPARATOR / 2.0,
                x2: setup.margin.left + width / 3.0,
                y2: y - NOTE_SEPARATOR / 2.0,
                width: 0.5,
                colour: [0, 0, 0],
            });
            for (note_items, h) in laid {
                items.extend(moved(note_items, setup.margin.left, y));
                y += h;
            }
        }
        out.push(LaidPage {
            width: setup.width,
            height: setup.height,
            items,
        });
    }
    out
}
