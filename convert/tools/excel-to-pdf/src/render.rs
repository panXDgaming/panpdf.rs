use std::collections::{BTreeSet, HashMap};

use convert_drawingml::{IDENTITY, NoInherit, Scene, Xfrm};
use convert_office_read::Package;
use convert_pdf_canvas::families::symbol_text;
use convert_pdf_canvas::text::starts_right_to_left;
use convert_pdf_canvas::{
    Canvas, Direction, FontBook, ImageInfo, Line, Page, Rgb, Span, layout, layout_directed,
};

use crate::numfmt;
use crate::xlsx::{
    Area, Cell, Drawing, Edge, HAlign, LineStyle, Place, Sheet, VAlign, Value, Workbook, Xf,
};

pub type Images = HashMap<String, Option<ImageInfo>>;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub fit: Option<Fit>,
    pub paper: Option<(f32, f32)>,
    pub landscape: Option<bool>,
    pub grid: bool,
    pub file_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Width,
    Page,
    None,
}

fn paper(code: u32) -> Option<(f32, f32)> {
    Some(match code {
        1 | 2 => (612.0, 792.0),
        3 => (792.0, 1224.0),
        5 => (612.0, 1008.0),
        7 => (522.0, 756.0),
        8 => (842.0, 1191.0),
        9 | 10 => (595.0, 842.0),
        11 => (420.0, 595.0),
        12 => (729.0, 1032.0),
        13 => (516.0, 729.0),
        _ => return None,
    })
}

const A4: (f32, f32) = (595.0, 842.0);
const PAD: f32 = 2.0;

struct Grid {
    widths: HashMap<u32, f32>,
    heights: HashMap<u32, f32>,
    default_width: f32,
    default_height: f32,
}

impl Grid {
    fn width(&self, c: u32) -> f32 {
        self.widths.get(&c).copied().unwrap_or(self.default_width)
    }

    fn height(&self, r: u32) -> f32 {
        self.heights.get(&r).copied().unwrap_or(self.default_height)
    }
}

fn argb(c: u32) -> Rgb {
    Rgb::from_u8((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

struct Shown {
    runs: Vec<(String, Option<crate::xlsx::Font>)>,
    color: Option<u32>,
    number: bool,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Number,
    Bool,
    Error,
}

fn shown(cell: &Cell, xf: &Xf, book: &Workbook) -> Option<Shown> {
    let value = cell.value.as_ref()?;
    let code = book
        .styles
        .num_fmts
        .get(&xf.num_fmt)
        .map(String::as_str)
        .or_else(|| numfmt::builtin(xf.num_fmt))
        .unwrap_or("General");
    Some(match value {
        Value::Number(v) => {
            let f = numfmt::format(code, *v, book.date1904);
            Shown {
                runs: vec![(f.text, None)],
                color: f.color,
                number: true,
                kind: Kind::Number,
            }
        }
        Value::Text(runs) => {
            if runs.len() == 1 && code.contains('@') {
                let f = numfmt::format_text(code, &runs[0].text);
                Shown {
                    runs: vec![(f.text, runs[0].font.clone())],
                    color: f.color,
                    number: false,
                    kind: Kind::Text,
                }
            } else {
                Shown {
                    runs: runs
                        .iter()
                        .map(|r| (r.text.clone(), r.font.clone()))
                        .collect(),
                    color: None,
                    number: false,
                    kind: Kind::Text,
                }
            }
        }
        Value::Bool(b) => Shown {
            runs: vec![(if *b { "TRUE" } else { "FALSE" }.to_owned(), None)],
            color: None,
            number: false,
            kind: Kind::Bool,
        },
        Value::Error(e) => Shown {
            runs: vec![(e.clone(), None)],
            color: None,
            number: false,
            kind: Kind::Error,
        },
    })
}

fn spans_of(shown: &Shown, xf: &Xf, book: &Workbook, wrap: bool) -> Vec<Span> {
    let base = book.styles.font(xf.font);
    shown
        .runs
        .iter()
        .map(|(text, font)| {
            let f = font.as_ref().unwrap_or(&base);
            let text = if wrap {
                text.clone()
            } else {
                text.replace(['\n', '\r'], " ")
            };
            let color = shown
                .color
                .or(f.color)
                .map_or(Rgb::BLACK, |c| argb(c & 0x00FF_FFFF));
            Span {
                text: symbol_text(&f.name, &text).unwrap_or(text),
                family: f.name.clone(),
                size: f.size,
                bold: f.bold,
                italic: f.italic,
                underline: f.underline,
                strike: f.strike,
                color,
            }
        })
        .collect()
}

fn drawing_cells(sheet: &Sheet, drawing: &Drawing) -> (u32, u32, u32, u32) {
    let col_w = |c: u32| {
        sheet.cols.get(&c).map_or_else(
            || sheet.default_col_width.unwrap_or(9.14) * 5.25,
            |&(w, hidden)| if hidden { 0.0 } else { w * 5.25 },
        )
    };
    let row_h = |r: u32| {
        sheet.rows.get(&r).map_or_else(
            || sheet.default_row_height.unwrap_or(15.0),
            |&(h, hidden)| if hidden { 0.0 } else { h.unwrap_or(15.0) },
        )
    };
    let walk = |first: u32, mut length: f32, size: &dyn Fn(u32) -> f32, last: u32| {
        let mut i = first;
        while i < last {
            let s = size(i);
            if length <= s {
                break;
            }
            length -= s;
            i += 1;
        }
        i
    };
    match drawing.place {
        Place::Cells(a, b) => (a.row, a.col, b.row, b.col),
        Place::Cell(a, w, h) => (
            a.row,
            a.col,
            walk(a.row, a.row_off + h, &row_h, 1_048_575),
            walk(a.col, a.col_off + w, &col_w, 16_383),
        ),
        Place::Absolute(x, y, w, h) => {
            let (r0, c0) = (walk(0, y, &row_h, 1_048_575), walk(0, x, &col_w, 16_383));
            (
                r0,
                c0,
                walk(0, y + h, &row_h, 1_048_575),
                walk(0, x + w, &col_w, 16_383),
            )
        }
    }
}

fn print_areas(sheet: &Sheet, book: &Workbook) -> Vec<Area> {
    if let Some(areas) = &sheet.print_area {
        return areas.clone();
    }
    let mut used: Option<Area> = None;
    let grow = |used: &mut Option<Area>, r: u32, c: u32| {
        let a = used.get_or_insert(Area {
            r0: r,
            c0: c,
            r1: r,
            c1: c,
        });
        a.r0 = a.r0.min(r);
        a.c0 = a.c0.min(c);
        a.r1 = a.r1.max(r);
        a.c1 = a.c1.max(c);
    };
    for (&(r, c), cell) in &sheet.cells {
        let xf = book.styles.xf(cell.style);
        let has_text = cell.value.as_ref().is_some_and(
            |v| !matches!(v, Value::Text(runs) if runs.iter().all(|r| r.text.is_empty())),
        );
        let border = [
            xf.border.left,
            xf.border.right,
            xf.border.top,
            xf.border.bottom,
        ]
        .iter()
        .any(|e| e.style != LineStyle::None);
        if has_text || xf.fill.is_some() || border {
            grow(&mut used, r, c);
        }
    }
    for m in &sheet.merges {
        if used.is_some() {
            grow(&mut used, m.r0, m.c0);
            grow(&mut used, m.r1, m.c1);
        }
    }
    for drawing in &sheet.drawings {
        let (r0, c0, r1, c1) = drawing_cells(sheet, drawing);
        grow(&mut used, r0, c0);
        grow(&mut used, r1, c1);
    }
    used.into_iter().collect()
}

fn natural_width(
    canvas: &mut Canvas,
    fonts: &mut FontBook,
    spans: &[Span],
    direction: Direction,
) -> f32 {
    layout_directed(canvas, fonts, spans, None, direction)
        .iter()
        .map(|l| l.width)
        .fold(0.0, f32::max)
}

struct Placed {
    lines: Vec<Line>,
    height: f32,
}

fn place(
    canvas: &mut Canvas,
    fonts: &mut FontBook,
    spans: &[Span],
    width: Option<f32>,
    direction: Direction,
) -> Placed {
    let lines = layout_directed(canvas, fonts, spans, width, direction);
    let height = lines.iter().map(Line::height).sum();
    Placed { lines, height }
}

fn cell_direction(xf: &Xf, sheet: &Sheet, kind: Kind, spans: &[Span]) -> Direction {
    if kind != Kind::Text && xf.reading_order != 2 {
        return Direction::Ltr;
    }
    match xf.reading_order {
        1 => Direction::Ltr,
        2 => Direction::Rtl,
        _ => {
            let strong = spans.iter().any(|s| {
                s.text.chars().any(|c| {
                    matches!(
                        convert_pdf_canvas::bidi::class(c),
                        convert_pdf_canvas::bidi::Class::L
                            | convert_pdf_canvas::bidi::Class::R
                            | convert_pdf_canvas::bidi::Class::AL
                    )
                })
            });
            if !strong && sheet.right_to_left {
                Direction::Rtl
            } else {
                Direction::Auto
            }
        }
    }
}

fn reads_right_to_left(direction: Direction, spans: &[Span]) -> bool {
    match direction {
        Direction::Ltr => false,
        Direction::Rtl => true,
        Direction::Auto => {
            let text: String = spans.iter().map(|s| s.text.as_str()).collect();
            starts_right_to_left(&text)
        }
    }
}

fn default_font_px(
    canvas: &mut Canvas,
    fonts: &mut FontBook,
    font: &crate::xlsx::Font,
) -> Option<(f32, f32)> {
    let pick = fonts.face(canvas, &font.name, font.bold, font.italic)?;
    let px = font.size * 96.0 / 72.0;
    let digit = ('0'..='9')
        .map(|d| canvas.measure(pick.font, d.encode_utf8(&mut [0; 4]), px))
        .fold(0.0, f32::max)
        .round();
    let m = canvas.font_metrics(pick.font);
    let em = f32::from(m.units_per_em.max(16));
    let (above, below) = if m.win_ascent + m.win_descent > 0 {
        (m.win_ascent, m.win_descent)
    } else {
        (m.ascent, -m.descent)
    };
    let line = ((above + below) as f32 / em * px).ceil() + 2.0;
    (digit >= 3.0 && line >= 8.0).then_some((digit, line))
}

fn build_grid(
    sheet: &Sheet,
    book: &Workbook,
    canvas: &mut Canvas,
    fonts: &mut FontBook,
    area: &Area,
) -> Grid {
    let base_font = book.styles.font(0);
    let (digit_px, standard_px) = default_font_px(canvas, fonts, &base_font).unwrap_or((
        (7.0 * base_font.size / 11.0).max(5.0),
        20.0 * base_font.size / 11.0,
    ));
    let to_pt = |chars: f32| (chars * digit_px).round() * 0.75;
    let default_width = sheet.default_col_width.map_or_else(
        || {
            let base = sheet.base_col_width.unwrap_or(8.0);
            let px = (base * digit_px + 5.0).ceil();
            (px / 8.0).ceil() * 8.0 * 0.75
        },
        to_pt,
    );
    let mut widths = HashMap::new();
    for (&c, &(w, hidden)) in &sheet.cols {
        widths.insert(c, if hidden { 0.0 } else { to_pt(w) });
    }
    let standard = standard_px * 0.75;
    let default_height = sheet.default_row_height.unwrap_or(standard);
    let mut heights = HashMap::new();
    for (&r, &(h, hidden)) in &sheet.rows {
        if hidden {
            heights.insert(r, 0.0);
        } else if let Some(h) = h {
            heights.insert(r, h);
        }
    }
    let mut grid = Grid {
        widths,
        heights,
        default_width,
        default_height,
    };
    let merged: BTreeSet<(u32, u32)> = sheet
        .merges
        .iter()
        .filter(|m| m.r0 != m.r1 || m.c0 != m.c1)
        .flat_map(|m| (m.r0..=m.r1).flat_map(move |r| (m.c0..=m.c1).map(move |c| (r, c))))
        .collect();
    let mut needed: HashMap<u32, f32> = HashMap::new();
    for (&(r, c), cell) in sheet.cells.range((area.r0, 0)..=(area.r1, u32::MAX)) {
        if sheet
            .rows
            .get(&r)
            .is_some_and(|(h, hidden)| h.is_some() || *hidden)
            || merged.contains(&(r, c))
            || !area.contains(r, c)
        {
            continue;
        }
        let xf = book.styles.xf(cell.style);
        let Some(shown) = shown(cell, &xf, book) else {
            continue;
        };
        if xf.rotation != 0 {
            continue;
        }
        let spans = spans_of(&shown, &xf, book, xf.wrap);
        let h = if xf.wrap {
            let width = (grid.width(c) - 2.0 * PAD).max(1.0);
            let direction = cell_direction(&xf, sheet, shown.kind, &spans);
            place(canvas, fonts, &spans, Some(width), direction).height + 2.0
        } else {
            spans.iter().map(|s| s.size).fold(0.0, f32::max) * standard / base_font.size
        };
        let entry = needed.entry(r).or_insert(0.0);
        *entry = entry.max(h);
    }
    for (r, h) in needed {
        if h > grid.default_height {
            grid.heights.insert(r, h);
        }
    }
    grid
}

fn cut(
    indices: &[u32],
    size: impl Fn(u32) -> f32,
    room: f32,
    manual: &BTreeSet<u32>,
) -> Vec<Vec<u32>> {
    let mut pages: Vec<Vec<u32>> = Vec::new();
    let mut current: Vec<u32> = Vec::new();
    let mut used = 0.0;
    for &i in indices {
        let s = size(i);
        let forced = manual.contains(&i) && !current.is_empty();
        if forced || (used + s > room + 0.01 && !current.is_empty()) {
            pages.push(std::mem::take(&mut current));
            used = 0.0;
        }
        current.push(i);
        used += s;
    }
    if !current.is_empty() {
        pages.push(current);
    }
    pages
}

struct Tile {
    rows: Vec<u32>,
    cols: Vec<u32>,
    row0: u32,
    col0: u32,
}

pub struct Printing<'a, 'b> {
    pub canvas: &'a mut Canvas,
    pub fonts: &'a mut FontBook,
    pub images: &'a mut Images,
    pub package: &'a Package<'b>,
    pub book: &'a Workbook,
    pub options: &'a Options,
    pub notes: &'a mut Vec<String>,
}

pub fn sheet_pages(p: &mut Printing<'_, '_>, sheet: &Sheet) -> usize {
    if sheet.chart_sheet {
        return chart_sheet_page(p, sheet);
    }
    cell_pages(p, sheet)
}

#[allow(clippy::too_many_lines)]
fn cell_pages(p: &mut Printing<'_, '_>, sheet: &Sheet) -> usize {
    let Printing {
        canvas,
        fonts,
        images,
        package,
        book,
        options,
        notes,
    } = p;
    let (book, options, package) = (*book, *options, *package);
    let areas = print_areas(sheet, book);
    let mut pages = 0;
    for area in &areas {
        let grid = build_grid(sheet, book, canvas, fonts, area);
        let setup = &sheet.setup;
        let rows: Vec<u32> = (area.r0..=area.r1)
            .filter(|&r| grid.height(r) > 0.0)
            .collect();
        let cols: Vec<u32> = (area.c0..=area.c1.min(area.c0 + 16_383))
            .filter(|&c| grid.width(c) > 0.0)
            .collect();
        if rows.is_empty() || cols.is_empty() {
            continue;
        }
        let total_w: f32 = cols.iter().map(|&c| grid.width(c)).sum();
        let total_h: f32 = rows.iter().map(|&r| grid.height(r)).sum();
        let margins = setup
            .margins
            .unwrap_or([0.7, 0.7, 0.75, 0.75, 0.3, 0.3])
            .map(|m| m * 72.0);
        let paper_size = options
            .paper
            .or_else(|| setup.paper.and_then(paper))
            .unwrap_or(A4);
        let use_file = options.fit.is_none() && setup.present;
        let fit = options.fit.unwrap_or(if use_file {
            if setup.fit_to_page {
                Fit::Page
            } else {
                Fit::None
            }
        } else {
            Fit::Width
        });
        let room = |landscape: bool| {
            let (w, h) = if landscape {
                (paper_size.1, paper_size.0)
            } else {
                paper_size
            };
            (w - margins[0] - margins[1], h - margins[2] - margins[3])
        };
        let scale_for = |landscape: bool| -> f32 {
            let (pw, ph) = room(landscape);
            match fit {
                Fit::None => {
                    setup
                        .scale
                        .filter(|_| use_file)
                        .unwrap_or(100.0)
                        .clamp(10.0, 400.0)
                        / 100.0
                }
                Fit::Width => (pw / total_w).min(1.0),
                Fit::Page => {
                    let (fw, fh) = if use_file {
                        (setup.fit_width, setup.fit_height)
                    } else {
                        (1, 1)
                    };
                    let mut s: f32 = 1.0;
                    if fw > 0 {
                        s = s.min(fw as f32 * pw / total_w);
                    }
                    if fh > 0 {
                        s = s.min(fh as f32 * ph / total_h);
                    }
                    s.max(0.1)
                }
            }
        };
        let landscape = options
            .landscape
            .or(setup.landscape.filter(|_| use_file))
            .unwrap_or_else(|| {
                !use_file && fit == Fit::Width && scale_for(true) > scale_for(false) * 1.25
            });
        let scale = scale_for(landscape);
        let (page_w, page_h) = if landscape {
            (paper_size.1, paper_size.0)
        } else {
            paper_size
        };
        let (pw, ph) = room(landscape);
        let title_rows: Vec<u32> = sheet
            .print_titles_rows
            .map(|(a, b)| (a..=b).filter(|&r| grid.height(r) > 0.0).collect())
            .unwrap_or_default();
        let title_cols: Vec<u32> = sheet
            .print_titles_cols
            .map(|(a, b)| (a..=b).filter(|&c| grid.width(c) > 0.0).collect())
            .unwrap_or_default();
        let title_h: f32 = title_rows.iter().map(|&r| grid.height(r)).sum();
        let title_w: f32 = title_cols.iter().map(|&c| grid.width(c)).sum();
        let row_breaks: BTreeSet<u32> = sheet.row_breaks.iter().copied().collect();
        let col_breaks: BTreeSet<u32> = sheet.col_breaks.iter().copied().collect();
        let row_pages = cut(&rows, |r| grid.height(r), ph / scale - title_h, &row_breaks);
        let col_pages = cut(&cols, |c| grid.width(c), pw / scale - title_w, &col_breaks);
        let mut tiles = Vec::new();
        let tile = |rp: &Vec<u32>, cp: &Vec<u32>| {
            let mut rows = Vec::new();
            if rp
                .first()
                .is_some_and(|f| title_rows.first().is_some_and(|t| f > t))
            {
                rows.extend(title_rows.iter().filter(|r| !rp.contains(r)));
            }
            rows.extend(rp);
            let mut cols = Vec::new();
            if cp
                .first()
                .is_some_and(|f| title_cols.first().is_some_and(|t| f > t))
            {
                cols.extend(title_cols.iter().filter(|c| !cp.contains(c)));
            }
            cols.extend(cp);
            Tile {
                rows,
                cols,
                row0: rp.first().copied().unwrap_or(0),
                col0: cp.first().copied().unwrap_or(0),
            }
        };
        if setup.over_then_down {
            for rp in &row_pages {
                for cp in &col_pages {
                    tiles.push(tile(rp, cp));
                }
            }
        } else {
            for cp in &col_pages {
                for rp in &row_pages {
                    tiles.push(tile(rp, cp));
                }
            }
        }
        let title_set: BTreeSet<u32> = title_rows.iter().copied().collect();
        let covers: Vec<(u32, u32, u32, u32)> = sheet
            .drawings
            .iter()
            .map(|d| drawing_cells(sheet, d))
            .collect();
        tiles.retain(|t| {
            covers.iter().any(|&(r0, c0, r1, c1)| {
                t.rows
                    .iter()
                    .any(|r| !title_set.contains(r) && (r0..=r1).contains(r))
                    && t.cols.iter().any(|c| (c0..=c1).contains(c))
            }) || t.rows.iter().filter(|r| !title_set.contains(r)).any(|&r| {
                t.cols.iter().any(|&c| {
                    sheet.cells.get(&(r, c)).is_some_and(|cell| {
                        let xf = book.styles.xf(cell.style);
                        cell.value.is_some()
                            || xf.fill.is_some()
                            || [
                                xf.border.left,
                                xf.border.right,
                                xf.border.top,
                                xf.border.bottom,
                            ]
                            .iter()
                            .any(|e| e.style != LineStyle::None)
                    }) || merge_of(sheet, r, c).is_some()
                })
            })
        });
        let count = tiles.len();
        for (number, tile) in tiles.iter().enumerate() {
            let id = canvas.add_page(page_w, page_h);
            let used_w: f32 = tile.cols.iter().map(|&c| grid.width(c)).sum::<f32>() * scale;
            let used_h: f32 = tile.rows.iter().map(|&r| grid.height(r)).sum::<f32>() * scale;
            let ox = margins[0]
                + if setup.center_h {
                    ((pw - used_w) / 2.0).max(0.0)
                } else if sheet.right_to_left {
                    (pw - used_w).max(0.0)
                } else {
                    0.0
                };
            let top = page_h
                - margins[2]
                - if setup.center_v {
                    ((ph - used_h) / 2.0).max(0.0)
                } else {
                    0.0
                };
            draw_tile(
                canvas,
                fonts,
                book,
                sheet,
                &grid,
                tile,
                ox,
                top,
                scale,
                options.grid || setup.grid_lines,
            );
            draw_drawings(
                &mut Printing {
                    canvas,
                    fonts,
                    images,
                    package,
                    book,
                    options,
                    notes,
                },
                sheet,
                &grid,
                tile,
                [scale, 0.0, 0.0, scale, ox, top],
            );
            let fields = Fields {
                page: setup.first_page_number.unwrap_or(1) as usize + number,
                pages: count,
                sheet: &sheet.name,
                file: &options.file_name,
            };
            let base = book.styles.font(0);
            let (header, footer) = header_footer(setup, number + 1);
            if !header.is_empty() {
                draw_header_footer(
                    canvas,
                    fonts,
                    id,
                    header,
                    &fields,
                    &base.name,
                    base.size,
                    page_h - margins[4],
                    true,
                    margins,
                );
            }
            if !footer.is_empty() {
                draw_header_footer(
                    canvas, fonts, id, footer, &fields, &base.name, base.size, margins[5], false,
                    margins,
                );
            }
            pages += 1;
        }
        if scale < 0.5 {
            notes.push(format!(
                "sheet '{}' printed at {:.0}% to fit the page",
                sheet.name,
                scale * 100.0
            ));
        }
    }
    pages
}

fn header_footer(setup: &crate::xlsx::PageSetup, n: usize) -> (&str, &str) {
    if setup.different_first && n == 1 {
        (&setup.first_header, &setup.first_footer)
    } else if setup.different_odd_even && n.is_multiple_of(2) {
        (&setup.even_header, &setup.even_footer)
    } else {
        (&setup.header, &setup.footer)
    }
}

struct Fields<'a> {
    page: usize,
    pages: usize,
    sheet: &'a str,
    file: &'a str,
}

#[allow(clippy::too_many_arguments)]
fn draw_header_footer(
    canvas: &mut Canvas,
    fonts: &mut FontBook,
    page: convert_pdf_canvas::PageId,
    text: &str,
    fields: &Fields<'_>,
    family: &str,
    size: f32,
    y: f32,
    header: bool,
    margins: [f32; 6],
) {
    let fresh = || Span::new("", family, size);
    let mut parts: [Vec<Span>; 3] = Default::default();
    let mut current = 1;
    let mut style = fresh();
    let push = |parts: &mut [Vec<Span>; 3], current: usize, style: &Span, text: &str| {
        let part = &mut parts[current];
        match part.last_mut() {
            Some(last)
                if last.family == style.family
                    && last.size == style.size
                    && last.bold == style.bold
                    && last.italic == style.italic
                    && last.underline == style.underline
                    && last.strike == style.strike
                    && last.color == style.color =>
            {
                last.text.push_str(text);
            }
            _ => {
                let mut span = style.clone();
                span.text = text.to_owned();
                part.push(span);
            }
        }
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            push(&mut parts, current, &style, c.encode_utf8(&mut [0; 4]));
            continue;
        }
        match chars.next() {
            Some(section @ ('L' | 'C' | 'R')) => {
                current = match section {
                    'L' => 0,
                    'C' => 1,
                    _ => 2,
                };
                style = fresh();
            }
            Some('P') => push(&mut parts, current, &style, &fields.page.to_string()),
            Some('N') => push(&mut parts, current, &style, &fields.pages.to_string()),
            Some('A') => push(&mut parts, current, &style, fields.sheet),
            Some('F') => push(&mut parts, current, &style, fields.file),
            Some('&') => push(&mut parts, current, &style, "&"),
            Some('B') => style.bold = !style.bold,
            Some('I') => style.italic = !style.italic,
            Some('U' | 'E') => style.underline = !style.underline,
            Some('S') => style.strike = !style.strike,
            Some('"') => {
                let mut code = String::new();
                for d in chars.by_ref() {
                    if d == '"' {
                        break;
                    }
                    code.push(d);
                }
                let (face, kind) = code.split_once(',').unwrap_or((&code, ""));
                let face = face.trim();
                style.family = if face.is_empty() || face == "-" {
                    family.to_owned()
                } else {
                    face.to_owned()
                };
                let kind = kind.to_ascii_lowercase();
                style.bold = kind.contains("bold");
                style.italic = kind.contains("italic") || kind.contains("oblique");
            }
            Some('K') => {
                let code: String = (0..6).filter_map(|_| chars.next()).collect();
                if let Ok(rgb) = u32::from_str_radix(&code, 16) {
                    style.color = argb(rgb);
                }
            }
            Some(d) if d.is_ascii_digit() => {
                let mut number = d.to_string();
                while let Some(n) = chars.peek().copied().filter(char::is_ascii_digit) {
                    number.push(n);
                    chars.next();
                }
                style.size = number.parse().unwrap_or(size);
            }
            _ => {}
        }
    }
    let page_w = canvas.page(page).width;
    for (k, spans) in parts.iter().enumerate() {
        if spans.iter().all(|s| s.text.trim().is_empty()) {
            continue;
        }
        let lines = layout(canvas, fonts, spans, None);
        let mut baseline = if header {
            y - lines.first().map_or(size, |l| l.ascent)
        } else {
            y
        };
        if !header {
            let total: f32 = lines.iter().map(Line::height).sum();
            baseline = y + total - lines.first().map_or(size, |l| l.ascent);
        }
        for line in &lines {
            let x = match k {
                0 => margins[0],
                1 => (page_w - line.width) / 2.0,
                _ => page_w - margins[1] - line.width,
            };
            line.draw(canvas.page(page), x, baseline, 0.0);
            baseline -= line.height();
        }
    }
}

fn merge_of(sheet: &Sheet, r: u32, c: u32) -> Option<&Area> {
    sheet
        .merges
        .iter()
        .find(|m| m.contains(r, c) && (m.r0 != m.r1 || m.c0 != m.c1))
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn draw_tile(
    canvas: &mut Canvas,
    fonts: &mut FontBook,
    book: &Workbook,
    sheet: &Sheet,
    grid: &Grid,
    tile: &Tile,
    ox: f32,
    top: f32,
    scale: f32,
    grid_lines: bool,
) {
    let mut col_x: HashMap<u32, f32> = HashMap::new();
    let mut x = 0.0;
    for &c in &tile.cols {
        col_x.insert(c, x);
        x += grid.width(c);
    }
    let tile_w = x;
    let rtl_sheet = sheet.right_to_left;
    if rtl_sheet {
        for &c in &tile.cols {
            if let Some(x) = col_x.get_mut(&c) {
                *x = tile_w - *x - grid.width(c);
            }
        }
    }
    let mut row_y: HashMap<u32, f32> = HashMap::new();
    let mut y = 0.0;
    for &r in &tile.rows {
        row_y.insert(r, y);
        y += grid.height(r);
    }
    let tile_h = y;
    let page_id = convert_pdf_canvas::PageId(canvas.page_count() - 1);
    {
        let page = canvas.page(page_id);
        page.save();
        page.transform([scale, 0.0, 0.0, scale, ox, top]);
        page.rect(0.0, -tile_h, tile_w, tile_h).clip();
    }
    let rows: BTreeSet<u32> = tile.rows.iter().copied().collect();
    let cols: BTreeSet<u32> = tile.cols.iter().copied().collect();
    let span_box = |m: &Area| -> Option<(f32, f32, f32, f32)> {
        let rs: Vec<u32> = (m.r0..=m.r1).filter(|r| rows.contains(r)).collect();
        let cs: Vec<u32> = (m.c0..=m.c1).filter(|c| cols.contains(c)).collect();
        let r_first = *rs.first()?;
        let x0 = cs.iter().map(|c| col_x[c]).reduce(f32::min)?;
        let y0 = row_y[&r_first];
        let w: f32 = cs.iter().map(|&c| grid.width(c)).sum();
        let h: f32 = rs.iter().map(|&r| grid.height(r)).sum();
        Some((x0, y0, w, h))
    };
    let whole_box = |m: &Area| -> Option<(f32, f32, f32, f32)> {
        let (x0, y0, _, _) = span_box(m)?;
        let c_first = (m.c0..=m.c1).find(|c| cols.contains(c))?;
        let r_first = (m.r0..=m.r1).find(|r| rows.contains(r))?;
        let before_w: f32 = (m.c0..c_first).map(|c| grid.width(c)).sum();
        let before_h: f32 = (m.r0..r_first).map(|r| grid.height(r)).sum();
        let w: f32 = (m.c0..=m.c1).map(|c| grid.width(c)).sum();
        let h: f32 = (m.r0..=m.r1).map(|r| grid.height(r)).sum();
        if rtl_sheet {
            let c_last = (m.c0..=m.c1).rev().find(|c| cols.contains(c))?;
            let after_w: f32 = (c_last + 1..=m.c1).map(|c| grid.width(c)).sum();
            return Some((x0 - after_w, y0 - before_h, w, h));
        }
        Some((x0 - before_w, y0 - before_h, w, h))
    };
    let style_at = |r: u32, c: u32| -> Xf {
        sheet
            .cells
            .get(&(r, c))
            .map_or_else(Xf::default, |cell| book.styles.xf(cell.style))
    };
    let mut filled_merges: BTreeSet<(u32, u32)> = BTreeSet::new();
    for &r in &tile.rows {
        for &c in &tile.cols {
            let (bx, by, bw, bh, xf) = if let Some(m) = merge_of(sheet, r, c) {
                if !filled_merges.insert((m.r0, m.c0)) {
                    continue;
                }
                let Some((bx, by, bw, bh)) = span_box(m) else {
                    continue;
                };
                (bx, by, bw, bh, style_at(m.r0, m.c0))
            } else {
                let Some(cell) = sheet.cells.get(&(r, c)) else {
                    continue;
                };
                (
                    col_x[&c],
                    row_y[&r],
                    grid.width(c),
                    grid.height(r),
                    book.styles.xf(cell.style),
                )
            };
            if let Some(fill) = xf.fill {
                canvas
                    .page(page_id)
                    .fill_rect(bx, -(by + bh), bw, bh, argb(fill & 0x00FF_FFFF));
            }
        }
    }
    if grid_lines {
        let light = Rgb::from_u8(0xD0, 0xD0, 0xD0);
        let page = canvas.page(page_id);
        for &c in &tile.cols {
            page.stroke_line(col_x[&c], 0.0, col_x[&c], -tile_h, 0.25, light);
        }
        page.stroke_line(tile_w, 0.0, tile_w, -tile_h, 0.25, light);
        for &r in &tile.rows {
            page.stroke_line(0.0, -row_y[&r], tile_w, -row_y[&r], 0.25, light);
        }
        page.stroke_line(0.0, -tile_h, tile_w, -tile_h, 0.25, light);
    }
    let mut drawn_merges: BTreeSet<(u32, u32)> = BTreeSet::new();
    for &r in &tile.rows {
        for &c in &tile.cols {
            let merge = merge_of(sheet, r, c);
            let (anchor, bx, by, bw, bh) = if let Some(m) = merge {
                if !drawn_merges.insert((m.r0, m.c0)) {
                    continue;
                }
                let Some((bx, by, bw, bh)) = whole_box(m) else {
                    continue;
                };
                ((m.r0, m.c0), bx, by, bw, bh)
            } else {
                ((r, c), col_x[&c], row_y[&r], grid.width(c), grid.height(r))
            };
            let Some(cell) = sheet.cells.get(&anchor) else {
                continue;
            };
            let xf = book.styles.xf(cell.style);
            let Some(mut shown) = shown(cell, &xf, book) else {
                continue;
            };
            if shown.kind == Kind::Error
                && let Some(how) = sheet.setup.errors.as_deref()
            {
                let text = match how {
                    "blank" => "",
                    "dash" => "--",
                    "NA" => "#N/A",
                    _ => shown.runs[0].0.as_str(),
                }
                .to_owned();
                shown.runs = vec![(text, None)];
            }
            if shown.runs.iter().all(|(t, _)| t.is_empty()) {
                continue;
            }
            let wrap = xf.wrap || matches!(xf.h_align, HAlign::Justify | HAlign::Distributed);
            let indent = xf.indent as f32 * 9.0;
            let inner = (bw - 2.0 * PAD - indent).max(1.0);
            let mut spans = spans_of(&shown, &xf, book, wrap);
            let direction = cell_direction(&xf, sheet, shown.kind, &spans);
            let h_align = match xf.h_align {
                HAlign::General => match shown.kind {
                    Kind::Number => HAlign::Right,
                    Kind::Bool | Kind::Error => HAlign::Center,
                    Kind::Text if reads_right_to_left(direction, &spans) => HAlign::Right,
                    Kind::Text => HAlign::Left,
                },
                HAlign::CenterContinuous => HAlign::Center,
                other => other,
            };
            if xf.shrink && !wrap {
                let natural = natural_width(canvas, fonts, &spans, direction);
                if natural > inner {
                    let k = (inner / natural).max(0.1);
                    for s in &mut spans {
                        s.size *= k;
                    }
                }
            }
            if xf.rotation != 0 && xf.rotation != 255 {
                draw_rotated(canvas, fonts, page_id, &spans, xf.rotation, bx, by, bw, bh);
                continue;
            }
            let mut placed = place(canvas, fonts, &spans, wrap.then_some(inner), direction);
            let widest = placed.lines.iter().map(|l| l.width).fold(0.0, f32::max);
            if shown.number && !wrap && widest > inner + 0.5 && widest <= inner * 1.15 {
                let k = inner / widest;
                for s in &mut spans {
                    s.size *= k;
                }
                placed = place(canvas, fonts, &spans, None, direction);
            }
            let widest = placed.lines.iter().map(|l| l.width).fold(0.0, f32::max);
            if shown.number && !wrap && widest > inner + 0.5 {
                let hash = natural_width(
                    canvas,
                    fonts,
                    &[Span {
                        text: "#".into(),
                        ..spans[0].clone()
                    }],
                    Direction::Ltr,
                )
                .max(1.0);
                let n = ((inner / hash).floor() as usize).max(1);
                let s = Span {
                    text: "#".repeat(n),
                    ..spans[0].clone()
                };
                placed = place(canvas, fonts, &[s], None, Direction::Ltr);
            }
            let (mut left, mut right) = (bx, bx + bw);
            let widest = placed.lines.iter().map(|l| l.width).fold(0.0, f32::max);
            if !wrap && merge.is_none() && !shown.number && widest > inner {
                let empty = |cc: u32| {
                    sheet.cells.get(&(r, cc)).is_none_or(|x| x.value.is_none())
                        && merge_of(sheet, r, cc).is_none()
                };
                let need = widest + 2.0 * PAD + indent;
                let grow_right = matches!(h_align, HAlign::Left | HAlign::Fill | HAlign::Justify)
                    || h_align == HAlign::Center;
                let grow_left = matches!(h_align, HAlign::Right) || h_align == HAlign::Center;
                let position = tile.cols.iter().position(|&x| x == c).unwrap_or(0);
                let after = tile.cols[position + 1..].iter().copied();
                let before = tile.cols[..position].iter().rev().copied();
                let (on_right, on_left): (Vec<u32>, Vec<u32>) = if rtl_sheet {
                    (before.collect(), after.collect())
                } else {
                    (after.collect(), before.collect())
                };
                if grow_right {
                    for &cc in &on_right {
                        if right - left >= need || !empty(cc) {
                            break;
                        }
                        right += grid.width(cc);
                    }
                }
                if grow_left {
                    for &cc in &on_left {
                        if right - left >= need || !empty(cc) {
                            break;
                        }
                        left -= grid.width(cc);
                    }
                }
            }
            let total_h = placed.height;
            let first_ascent = placed.lines.first().map_or(0.0, |l| l.ascent);
            let last = placed
                .lines
                .last()
                .map_or((0.0, 0.0), |l| (l.descent, l.height()));
            let nominal_descent =
                (spans.iter().map(|s| s.size).fold(0.0, f32::max) * 0.24).min(last.0.max(0.1));
            let mut baseline = match xf.v_align {
                VAlign::Top | VAlign::Justify | VAlign::Distributed => by + 1.0 + first_ascent,
                VAlign::Center => by + (bh - total_h) / 2.0 + first_ascent,
                VAlign::Bottom => by + bh - 1.0 - nominal_descent - (total_h - last.1),
            };
            let page = canvas.page(page_id);
            page.save();
            page.rect(
                left,
                -(by + bh + 2.0),
                right - left,
                bh + 2.0 + bh.min(12.0),
            )
            .clip();
            for line in &placed.lines {
                let avail = right - left - 2.0 * PAD - indent;
                let spacing = if matches!(h_align, HAlign::Justify | HAlign::Distributed)
                    && !line.last
                    && line.spaces > 0
                {
                    ((avail - line.width) / line.spaces as f32).max(0.0)
                } else {
                    0.0
                };
                let lx = match h_align {
                    HAlign::Right => right - PAD - indent - line.width,
                    HAlign::Center => left + (right - left - line.width) / 2.0,
                    HAlign::Justify | HAlign::Distributed if line.rtl => {
                        right - PAD - indent - line.width - spacing * line.spaces as f32
                    }
                    _ => left + PAD + indent,
                };
                line.draw(page, lx, -baseline, spacing);
                baseline += line.height();
            }
            page.restore();
        }
    }
    for &r in &tile.rows {
        for &c in &tile.cols {
            let Some(cell) = sheet.cells.get(&(r, c)) else {
                continue;
            };
            let border = book.styles.xf(cell.style).border;
            let merge = merge_of(sheet, r, c);
            let (x0, y0, x1, y1) = (
                col_x[&c],
                row_y[&r],
                col_x[&c] + grid.width(c),
                row_y[&r] + grid.height(r),
            );
            let outer = |dr: i32, dc: i32| {
                merge.is_none_or(|m| {
                    let (nr, nc) = (r as i64 + i64::from(dr), c as i64 + i64::from(dc));
                    !(nr >= i64::from(m.r0)
                        && nr <= i64::from(m.r1)
                        && nc >= i64::from(m.c0)
                        && nc <= i64::from(m.c1))
                })
            };
            let page = canvas.page(page_id);
            let (on_left, on_right, left_out, right_out) = if rtl_sheet {
                (border.right, border.left, outer(0, 1), outer(0, -1))
            } else {
                (border.left, border.right, outer(0, -1), outer(0, 1))
            };
            if left_out {
                edge(page, on_left, x0, -y0, x0, -y1);
            }
            if right_out {
                edge(page, on_right, x1, -y0, x1, -y1);
            }
            if outer(-1, 0) {
                edge(page, border.top, x0, -y0, x1, -y0);
            }
            if outer(1, 0) {
                edge(page, border.bottom, x0, -y1, x1, -y1);
            }
        }
    }
    canvas.page(page_id).restore();
}

fn drawing_box(place: Place, grid: &Grid) -> (f32, f32, f32, f32) {
    let col_at = |c: u32| (0..c).map(|k| grid.width(k)).sum::<f32>();
    let row_at = |r: u32| (0..r).map(|k| grid.height(k)).sum::<f32>();
    match place {
        Place::Cells(a, b) => {
            let x0 = col_at(a.col) + a.col_off.min(grid.width(a.col));
            let y0 = row_at(a.row) + a.row_off.min(grid.height(a.row));
            let x1 = col_at(b.col) + b.col_off.min(grid.width(b.col));
            let y1 = row_at(b.row) + b.row_off.min(grid.height(b.row));
            (x0, y0, x1 - x0, y1 - y0)
        }
        Place::Cell(a, w, h) => (col_at(a.col) + a.col_off, row_at(a.row) + a.row_off, w, h),
        Place::Absolute(x, y, w, h) => (x, y, w, h),
    }
}

fn drawing_xfrm(d: &Drawing, x: f32, y: f32, w: f32, h: f32) -> Xfrm {
    let own = match d.element.local() {
        "grpSp" => d.element.path(&["grpSpPr", "xfrm"]),
        "graphicFrame" => d.element.child("xfrm"),
        _ => d.element.path(&["spPr", "xfrm"]),
    };
    let turn = Xfrm::turn_of(own).rot.rem_euclid(180.0);
    let (w, h, x, y) = if (45.0..135.0).contains(&turn) {
        (h, w, x + (w - h) / 2.0, y + (h - w) / 2.0)
    } else {
        (w, h, x, y)
    };
    Xfrm {
        x,
        y,
        w,
        h,
        ..Xfrm::default()
    }
}

fn draw_list(
    p: &mut Printing<'_, '_>,
    page: convert_pdf_canvas::PageId,
    base: convert_drawingml::Matrix,
    drawings: &[(&Drawing, Xfrm)],
) {
    let map = HashMap::new();
    let date1904 = p.book.date1904;
    let format = move |code: &str, v: f64| numfmt::format(code, v, date1904).text;
    let book = p.book;
    let hidden = move |formula: &str| hidden_cells(book, formula);
    let mut scene = Scene {
        canvas: &mut *p.canvas,
        fonts: &mut *p.fonts,
        package: p.package,
        images: &mut *p.images,
        theme: &p.book.theme,
        map: &map,
        page,
        base,
        notes: &mut *p.notes,
        default_size: 11.0,
        default_text: None,
        format: &format,
        hidden: &hidden,
    };
    for (d, place) in drawings {
        scene.element(
            &d.element,
            &d.part,
            IDENTITY,
            true,
            &NoInherit,
            Some(*place),
        );
    }
}

fn hidden_cells(book: &Workbook, formula: &str) -> Option<Vec<bool>> {
    let mut out = Vec::new();
    let text = formula.trim();
    let text = text
        .strip_prefix('(')
        .and_then(|t| t.strip_suffix(')'))
        .unwrap_or(text);
    for part in text.split(',') {
        let (name, range) = part.trim().rsplit_once('!')?;
        let name = name.trim();
        let name = name
            .strip_prefix('\'')
            .and_then(|n| n.strip_suffix('\''))
            .map_or_else(|| name.to_owned(), |n| n.replace("''", "'"));
        let sheet = book.sheets.iter().find(|s| s.name == name)?;
        let a = crate::xlsx::area(range)?;
        let cells = u64::from(a.r1 - a.r0 + 1) * u64::from(a.c1 - a.c0 + 1);
        if cells > 1_000_000 {
            return None;
        }
        for r in a.r0..=a.r1 {
            for c in a.c0..=a.c1 {
                let row = sheet.rows.get(&r).is_some_and(|x| x.1);
                let col = sheet.cols.get(&c).is_some_and(|x| x.1);
                out.push(row || col);
            }
        }
    }
    Some(out)
}

fn draw_drawings(
    p: &mut Printing<'_, '_>,
    sheet: &Sheet,
    grid: &Grid,
    tile: &Tile,
    transform: [f32; 6],
) {
    if sheet.drawings.is_empty() {
        return;
    }
    let col_at = |c: u32| (0..c).map(|k| grid.width(k)).sum::<f32>();
    let row_at = |r: u32| (0..r).map(|k| grid.height(k)).sum::<f32>();
    let mut own_x = 0.0;
    for &c in tile.cols.iter().take_while(|&&c| c != tile.col0) {
        own_x += grid.width(c);
    }
    let mut own_y = 0.0;
    for &r in tile.rows.iter().take_while(|&&r| r != tile.row0) {
        own_y += grid.height(r);
    }
    let tile_w: f32 = tile.cols.iter().map(|&c| grid.width(c)).sum();
    let tile_h: f32 = tile.rows.iter().map(|&r| grid.height(r)).sum();
    let (shift_x, shift_y) = (own_x - col_at(tile.col0), own_y - row_at(tile.row0));
    let list: Vec<(&Drawing, Xfrm)> = sheet
        .drawings
        .iter()
        .filter_map(|d| {
            let (x, y, w, h) = drawing_box(d.place, grid);
            let (tx, ty) = (x + shift_x, y + shift_y);
            if w <= 0.0
                || h <= 0.0
                || tx >= tile_w
                || ty >= tile_h
                || tx + w <= own_x
                || ty + h <= own_y
            {
                return None;
            }
            let x = if sheet.right_to_left {
                tile_w - x - w - 2.0 * shift_x
            } else {
                x
            };
            Some((d, drawing_xfrm(d, x, y, w, h)))
        })
        .collect();
    if list.is_empty() {
        return;
    }
    let page_id = convert_pdf_canvas::PageId(p.canvas.page_count() - 1);
    {
        let page = p.canvas.page(page_id);
        page.save();
        page.transform(transform);
        if sheet.right_to_left {
            page.rect(0.0, -tile_h, tile_w - own_x, tile_h - own_y)
                .clip();
        } else {
            page.rect(own_x, -tile_h, tile_w - own_x, tile_h - own_y)
                .clip();
        }
    }
    draw_list(p, page_id, [1.0, 0.0, 0.0, -1.0, shift_x, -shift_y], &list);
    p.canvas.page(page_id).restore();
}

fn chart_sheet_page(p: &mut Printing<'_, '_>, sheet: &Sheet) -> usize {
    if sheet.drawings.is_empty() {
        return 0;
    }
    let setup = &sheet.setup;
    let paper_size = p
        .options
        .paper
        .or_else(|| setup.paper.and_then(paper))
        .unwrap_or(A4);
    let landscape = p.options.landscape.or(setup.landscape).unwrap_or(true);
    let (page_w, page_h) = if landscape {
        (paper_size.1, paper_size.0)
    } else {
        paper_size
    };
    let margins = setup
        .margins
        .unwrap_or([0.7, 0.7, 0.75, 0.75, 0.3, 0.3])
        .map(|m| m * 72.0);
    let grid = Grid {
        widths: HashMap::new(),
        heights: HashMap::new(),
        default_width: 48.0,
        default_height: 15.0,
    };
    let boxes: Vec<(f32, f32, f32, f32)> = sheet
        .drawings
        .iter()
        .map(|d| drawing_box(d.place, &grid))
        .collect();
    let x0 = boxes.iter().map(|b| b.0).fold(f32::MAX, f32::min);
    let y0 = boxes.iter().map(|b| b.1).fold(f32::MAX, f32::min);
    let x1 = boxes.iter().map(|b| b.0 + b.2).fold(f32::MIN, f32::max);
    let y1 = boxes.iter().map(|b| b.1 + b.3).fold(f32::MIN, f32::max);
    let room_w = page_w - margins[0] - margins[1];
    let room_h = page_h - margins[2] - margins[3];
    if x1 - x0 <= 0.0 || y1 - y0 <= 0.0 || room_w <= 0.0 || room_h <= 0.0 {
        return 0;
    }
    let (sx, sy) = (room_w / (x1 - x0), room_h / (y1 - y0));
    let list: Vec<(&Drawing, Xfrm)> = sheet
        .drawings
        .iter()
        .zip(&boxes)
        .map(|(d, b)| {
            (
                d,
                drawing_xfrm(d, (b.0 - x0) * sx, (b.1 - y0) * sy, b.2 * sx, b.3 * sy),
            )
        })
        .collect();
    let id = p.canvas.add_page(page_w, page_h);
    draw_list(
        p,
        id,
        [1.0, 0.0, 0.0, -1.0, margins[0], page_h - margins[2]],
        &list,
    );
    let fields = Fields {
        page: setup.first_page_number.unwrap_or(1) as usize,
        pages: 1,
        sheet: &sheet.name,
        file: &p.options.file_name,
    };
    let base = p.book.styles.font(0);
    let (header, footer) = header_footer(setup, 1);
    for (text, y, header) in [
        (header, page_h - margins[4], true),
        (footer, margins[5], false),
    ] {
        if !text.is_empty() {
            draw_header_footer(
                p.canvas, p.fonts, id, text, &fields, &base.name, base.size, y, header, margins,
            );
        }
    }
    1
}

#[allow(clippy::too_many_arguments)]
fn draw_rotated(
    canvas: &mut Canvas,
    fonts: &mut FontBook,
    page_id: convert_pdf_canvas::PageId,
    spans: &[Span],
    rotation: i32,
    bx: f32,
    by: f32,
    bw: f32,
    bh: f32,
) {
    let degrees = if rotation <= 90 {
        rotation as f32
    } else {
        -((rotation - 90) as f32)
    };
    let lines = layout(canvas, fonts, spans, None);
    let Some(line) = lines.first() else { return };
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (ax, ay) = if degrees >= 0.0 {
        (
            bx + PAD + line.descent * sin + (bw - line.width * cos).max(0.0) / 2.0,
            -(by + bh - PAD),
        )
    } else {
        (
            bx + PAD + (bw - line.width * cos).max(0.0) / 2.0,
            -(by + PAD),
        )
    };
    let page = canvas.page(page_id);
    page.save();
    page.transform([cos, sin, -sin, cos, ax, ay]);
    let baseline = if degrees >= 0.0 {
        line.descent
    } else {
        -line.ascent
    };
    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
    page.begin_actual_text(&text);
    line.draw(page, 0.0, baseline, 0.0);
    page.end_actual_text();
    page.restore();
}

fn edge(page: &mut Page, e: Edge, x0: f32, y0: f32, x1: f32, y1: f32) {
    let (width, dash): (f32, &[f32]) = match e.style {
        LineStyle::None => return,
        LineStyle::Hair => (0.25, &[]),
        LineStyle::Thin => (0.5, &[]),
        LineStyle::Medium => (1.0, &[]),
        LineStyle::Thick => (1.5, &[]),
        LineStyle::Double => (0.5, &[]),
        LineStyle::Dotted => (0.5, &[0.5, 1.0]),
        LineStyle::Dashed => (0.5, &[3.0, 1.5]),
        LineStyle::MediumDashed => (1.0, &[4.0, 2.0]),
        LineStyle::DashDot => (0.5, &[3.0, 1.0, 1.0, 1.0]),
        LineStyle::MediumDashDot | LineStyle::SlantDashDot => (1.0, &[4.0, 1.5, 1.5, 1.5]),
        LineStyle::DashDotDot => (0.5, &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
        LineStyle::MediumDashDotDot => (1.0, &[4.0, 1.5, 1.5, 1.5, 1.5, 1.5]),
    };
    let color = argb(e.color & 0x00FF_FFFF);
    page.save().set_stroke(color).set_line_width(width);
    if !dash.is_empty() {
        page.set_dash(dash, 0.0);
    }
    if e.style == LineStyle::Double {
        let (dx, dy) = if (x0 - x1).abs() < 1e-3 {
            (0.75, 0.0)
        } else {
            (0.0, 0.75)
        };
        page.move_to(x0 - dx, y0 - dy)
            .line_to(x1 - dx, y1 - dy)
            .stroke();
        page.move_to(x0 + dx, y0 + dy)
            .line_to(x1 + dx, y1 + dy)
            .stroke();
    } else {
        page.move_to(x0, y0).line_to(x1, y1).stroke();
    }
    page.restore();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xlsx::Marker;

    #[test]
    fn drawing_cover() {
        let mut sheet = Sheet {
            default_row_height: Some(20.0),
            ..Sheet::default()
        };
        sheet.cols.insert(0, (10.0, false));
        let at = |row, col| Marker {
            row,
            col,
            ..Marker::default()
        };
        let picture = |place| Drawing {
            place,
            element: convert_office_read::Element::default(),
            part: String::new(),
        };
        let p = picture(Place::Cell(at(2, 1), 100.0, 50.0));
        assert_eq!(drawing_cells(&sheet, &p), (2, 1, 4, 3));
        let p = picture(Place::Cells(at(0, 4), at(3, 5)));
        assert_eq!(drawing_cells(&sheet, &p), (0, 4, 3, 5));
        let p = picture(Place::Absolute(0.0, 0.0, 60.0, 30.0));
        assert_eq!(drawing_cells(&sheet, &p), (0, 0, 1, 1));
    }
}
