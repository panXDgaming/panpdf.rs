#![forbid(unsafe_code)]

pub mod actual_text;
pub mod bullets;
pub mod bytes_tool;
pub mod geometry;
pub mod held_fonts;
pub mod model;
pub mod order;
#[cfg(feature = "pictures")]
pub mod pictures;
pub mod repair;
pub mod roles;
pub mod rules;
pub mod script;
pub mod tables;
pub mod text;

use std::sync::Arc;

use pdf_bytes::{ByteStore, SourceId};
pub use pdf_content::FontProvider;
use pdf_session::PageView;

use geometry::Shown;
use model::{Align, Block, Cell, Document, Page, Paragraph, Rect, Run, Table, TableRow};
use script::Script;
use text::{End, Glyphs, Line};

#[derive(Clone, Debug, Default)]
pub struct Output {
    pub main: Vec<u8>,
    pub attachments: Vec<(String, Vec<u8>)>,
    pub notes: Vec<String>,
    pub dropped: String,
}

impl Document {
    #[must_use]
    pub fn running_heads(&self) -> String {
        let mut out = String::new();
        for page in &self.pages {
            for paragraph in page.header.iter().chain(&page.footer) {
                out.push_str(&paragraph.text());
                out.push('\n');
            }
        }
        out
    }
}

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub pages: Vec<usize>,
    pub password: Vec<u8>,
    pub attachments: String,
}

pub fn read(
    pdf: Vec<u8>,
    request: &Request,
    fonts: Option<Arc<dyn FontProvider>>,
    options: &Options,
    progress: &mut dyn FnMut(usize, usize) -> bool,
    sink: &mut dyn FnMut(&mut Page),
) -> Result<Document, String> {
    let reader = Reader::open(pdf, &request.password, fonts)?;
    if let Some(&page) = request
        .pages
        .iter()
        .find(|&&page| page >= reader.page_count())
    {
        return Err(format!(
            "page {} was asked for and the file has {} pages",
            page + 1,
            reader.page_count()
        ));
    }
    read_document(&reader, &request.pages, options, progress, sink)
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub pictures: bool,
    pub tables: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            pictures: cfg!(feature = "pictures"),
            tables: true,
        }
    }
}

#[derive(Debug)]
pub struct ReadEverywhere {
    inner: Arc<dyn FontProvider>,
}

impl ReadEverywhere {
    #[must_use]
    pub fn new(inner: Arc<dyn FontProvider>) -> Self {
        Self { inner }
    }
}

impl FontProvider for ReadEverywhere {
    fn primary_face(
        &self,
        request: &pdf_content::FontRequest,
    ) -> Option<pdf_content::SubstitutedFace> {
        self.inner.primary_face(request)
    }

    fn fallback_face(
        &self,
        request: &pdf_content::FontRequest,
        character: char,
    ) -> Option<pdf_content::SubstitutedFace> {
        self.inner.fallback_face(request, character)
    }

    fn description(&self) -> String {
        self.inner.description()
    }

    fn decipher_regions(&self, _page: pdf_syntax::Reference) -> Vec<[f64; 4]> {
        vec![[-1.0e7, -1.0e7, 1.0e7, 1.0e7]]
    }
}

pub struct Reader {
    source: ByteStore,
    credential: Vec<u8>,
    fonts: Option<Arc<dyn FontProvider>>,
    pages: usize,
}

impl Reader {
    pub fn open(
        bytes: Vec<u8>,
        password: &[u8],
        fonts: Option<Arc<dyn FontProvider>>,
    ) -> Result<Self, String> {
        let source = ByteStore::owning(SourceId::next_document(), bytes);
        let pages = pdf_content::count_pages_recovering(
            &source,
            pdf_content::PageContentLimits::default(),
            pdf_content::RecoverLimits::default(),
            password,
        )
        .map_err(|error| format!("the file cannot be read as a PDF: {error}"))?
        .into_parts()
        .0;
        let fonts =
            fonts.map(|inner| Arc::new(ReadEverywhere::new(inner)) as Arc<dyn FontProvider>);
        Ok(Self {
            source,
            credential: password.to_vec(),
            fonts,
            pages,
        })
    }

    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages
    }

    pub fn view(&self, index: usize) -> Result<PageView, String> {
        pdf_session::interpret_page_for_display(
            &self.source,
            index,
            &self.credential,
            None,
            self.fonts.clone(),
        )
        .map_err(|error| error.to_string())
    }

    #[must_use]
    pub fn read_page(&self, index: usize, options: &Options) -> Page {
        match self.view(index) {
            Ok(view) => assemble(index, &view, options, Some(&self.source)),
            Err(why) => Page {
                index,
                refused: Some(why),
                ..Page::default()
            },
        }
    }
}

pub fn read_document(
    reader: &Reader,
    pages: &[usize],
    options: &Options,
    progress: &mut dyn FnMut(usize, usize) -> bool,
    sink: &mut dyn FnMut(&mut Page),
) -> Result<Document, String> {
    let all: Vec<usize>;
    let pages = if pages.is_empty() {
        all = (0..reader.page_count()).collect();
        &all
    } else {
        pages
    };
    let mut document = Document::default();
    for (done, &index) in pages.iter().enumerate() {
        let mut page = reader.read_page(index, options);
        sink(&mut page);
        document.pages.push(page);
        if !progress(done + 1, pages.len()) {
            return Err(CANCELLED.to_owned());
        }
    }
    finish(&mut document);
    Ok(document)
}

pub const CANCELLED: &str = "cancelled";

pub fn finish(document: &mut Document) {
    roles::take_running_heads(document);
    document.body_size = roles::body_size(document);
    let body = document.body_size;
    for page in &mut document.pages {
        let left = text_left(&page.blocks);
        for block in &mut page.blocks {
            match block {
                Block::Paragraph(paragraph) => roles::mark_lists(paragraph, left, body),
                Block::Table(table) => {
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            let left = cell
                                .paragraphs
                                .iter()
                                .map(|p| p.frame.x0)
                                .fold(f64::INFINITY, f64::min);
                            for paragraph in &mut cell.paragraphs {
                                roles::mark_lists(paragraph, left, body);
                            }
                        }
                    }
                }
                Block::Picture(_) => {}
            }
        }
    }
    roles::mark_headings(document);
}

fn text_left(blocks: &[Block]) -> f64 {
    let mut lefts: Vec<f64> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph.frame.x0),
            _ => None,
        })
        .collect();
    lefts.sort_by(f64::total_cmp);
    lefts.first().copied().unwrap_or(0.0)
}

type CellPlace = (usize, (usize, usize));

enum Unit {
    Lines(Vec<Line>),
    Table(usize),
    #[cfg_attr(
        not(feature = "pictures"),
        expect(dead_code, reason = "pictures are read only with the feature")
    )]
    Picture(model::Picture),
}

fn line_of(template: &Line, clusters: Vec<Glyphs>, end: End) -> Line {
    let rect = clusters
        .iter()
        .skip(1)
        .fold(clusters[0].rect, |rect, glyphs| rect.union(&glyphs.rect));
    Line {
        clusters,
        rect,
        baseline: template.baseline,
        em: template.em,
        end,
        horizontal: template.horizontal,
    }
}

fn assemble(index: usize, view: &PageView, options: &Options, file: Option<&ByteStore>) -> Page {
    let shown = Shown::of(&view.program.geometry);
    let (width, height) = shown.size();
    let mut page = Page {
        index,
        width,
        height,
        ..Page::default()
    };
    let mut blocks = text::read_blocks(view, &shown, &actual_text::read(view, file));
    blocks.extend(text::group_blocks(view, &shown));
    let annotated = text::annotation_blocks(view, &shown, &blocks);
    blocks.extend(annotated);
    bullets::mark(view, &shown, &mut blocks);
    for block in blocks.iter_mut().filter(|block| block.read) {
        text::rejoin_wrapped(&mut block.lines);
    }
    let debug = |stage: &str, blocks: &[text::TextBlock]| {
        if std::env::var_os("CONVERT_DEBUG").is_some() {
            for block in blocks {
                for line in &block.lines {
                    let text: String = line.clusters.iter().map(|g| g.text.as_str()).collect();
                    eprintln!(
                        "{stage} b{} {:?} {:?} {:?} read={}",
                        block.block, line.rect, text, line.end, block.read
                    );
                }
            }
        }
    };
    debug("read", &blocks);
    let blocks = repair::inside_page(blocks, &shown.page());
    debug("inside", &blocks);
    let blocks = repair::split_columns(blocks);
    debug("split", &blocks);
    let blocks = repair::rejoin_shifted(blocks);
    debug("rejoin", &blocks);
    let unread = blocks.iter().filter(|block| !block.read).count();
    if unread > 0 {
        page.notes.push(format!(
            "{unread} text blocks read without the block reader"
        ));
    }

    let mut grids = Vec::new();
    if options.tables {
        let (rules, fills) = rules::collect(&view.graph, &shown);
        if rules.len() <= 4000 {
            grids = tables::find(&rules, &fills);
        } else {
            page.notes.push(format!(
                "{} ruling lines: tables not looked for",
                rules.len()
            ));
        }
        let page_area = width * height;
        grids.retain(|grid| {
            let mut hit: Vec<(usize, usize)> = Vec::new();
            for block in &blocks {
                for line in &block.lines {
                    for glyphs in line.clusters.iter().filter(|g| !g.blank) {
                        if let Some(cell) = grid.cell_at(glyphs.rect.center())
                            && !hit.contains(&cell)
                        {
                            hit.push(cell);
                        }
                    }
                }
            }
            let frame_like =
                grid.frame.area() > 0.85 * page_area && (grid.rows() <= 2 || grid.columns() <= 1);
            hit.len() >= 2 && !frame_like
        });
        let mut pieces = Vec::new();
        for block in &blocks {
            for line in &block.lines {
                if grids
                    .iter()
                    .any(|grid| grid.frame.contains(line.rect.center()))
                {
                    continue;
                }
                for piece in repair::cut_at(line, 0.8) {
                    let chars = piece
                        .clusters
                        .iter()
                        .map(|g| g.text.trim().chars().count())
                        .sum();
                    if chars > 0 {
                        pieces.push(tables::Piece {
                            rect: piece.rect,
                            baseline: piece.baseline,
                            em: piece.em,
                            chars,
                        });
                    }
                }
            }
        }
        grids.extend(tables::find_stream(&pieces));
    }

    let mut cell_lines: Vec<Vec<((usize, usize), Line)>> = vec![Vec::new(); grids.len()];
    let mut units: Vec<(Rect, Unit)> = Vec::new();
    let mut text_rects: Vec<Rect> = Vec::new();
    for block in blocks {
        let mut outside: Vec<Line> = Vec::new();
        for line in block.lines {
            text_rects.push(line.rect);
            let mut pieces: Vec<(Option<CellPlace>, Vec<Glyphs>)> = Vec::new();
            for glyphs in line.clusters.iter().cloned() {
                let (x, y) = glyphs.rect.center();
                let place = grids
                    .iter()
                    .enumerate()
                    .find_map(|(g, grid)| grid.cell_at((x, y)).map(|cell| (g, cell)));
                match pieces.last_mut() {
                    Some((last, piece)) if *last == place || glyphs.blank => piece.push(glyphs),
                    _ => pieces.push((place, vec![glyphs])),
                }
            }
            let count = pieces.len();
            for (at, (place, clusters)) in pieces.into_iter().enumerate() {
                let end = if at + 1 == count {
                    line.end
                } else {
                    End::Paragraph
                };
                let piece = line_of(&line, clusters, end);
                match place {
                    Some((grid, cell)) => {
                        if !outside.is_empty() {
                            flush_lines(&mut outside, &mut units);
                        }
                        cell_lines[grid].push((cell, piece));
                    }
                    None => outside.push(piece),
                }
            }
        }
        flush_lines(&mut outside, &mut units);
    }
    for (at, grid) in grids.iter().enumerate() {
        units.push((grid.frame, Unit::Table(at)));
    }
    #[cfg(feature = "pictures")]
    if options.pictures {
        let (pictures, notes) = pictures::read(view, &shown, &text_rects);
        page.notes.extend(notes);
        for picture in pictures {
            units.push((picture.frame, Unit::Picture(picture)));
        }
        let frames: Vec<Rect> = grids.iter().map(|grid| grid.frame).collect();
        let (drawings, notes) = pictures::drawings(view, &shown, &text_rects, &frames);
        page.notes.extend(notes);
        for picture in drawings {
            units.push((picture.frame, Unit::Picture(picture)));
        }
    }
    #[cfg(not(feature = "pictures"))]
    let _ = &text_rects;

    let (backgrounds, units): (Vec<_>, Vec<_>) = units
        .into_iter()
        .partition(|(_, unit)| matches!(unit, Unit::Picture(picture) if picture.background));
    for (_, unit) in backgrounds {
        if let Unit::Picture(picture) = unit {
            page.blocks.push(Block::Picture(picture));
        }
    }
    let ems: Vec<f64> = units
        .iter()
        .filter_map(|(_, unit)| match unit {
            Unit::Lines(lines) => lines.first().map(|line| line.em),
            _ => None,
        })
        .collect();
    let gutter = median(&ems).unwrap_or(10.0).max(6.0) * 1.2;
    let rects: Vec<Rect> = units.iter().map(|(rect, _)| *rect).collect();
    let order = order::reading_order(&rects, gutter);
    let column = text_extent(&units);
    let mut slots: Vec<Option<Unit>> = units.into_iter().map(|(_, unit)| Some(unit)).collect();
    for at in order {
        let Some(unit) = slots[at].take() else {
            continue;
        };
        match unit {
            Unit::Lines(lines) => {
                for paragraph in paragraphs(&lines, column) {
                    page.blocks.push(Block::Paragraph(paragraph));
                }
            }
            Unit::Table(grid) => {
                let lines = std::mem::take(&mut cell_lines[grid]);
                page.blocks.push(Block::Table(table(&grids[grid], lines)));
            }
            Unit::Picture(picture) => page.blocks.push(Block::Picture(picture)),
        }
    }
    space_between(&mut page.blocks);
    page
}

fn flush_lines(outside: &mut Vec<Line>, units: &mut Vec<(Rect, Unit)>) {
    if outside.is_empty() {
        return;
    }
    let lines = std::mem::take(outside);
    let rect = lines
        .iter()
        .skip(1)
        .fold(lines[0].rect, |rect, line| rect.union(&line.rect));
    units.push((rect, Unit::Lines(lines)));
}

fn median(values: &[f64]) -> Option<f64> {
    let mut values: Vec<f64> = values.iter().copied().filter(|v| *v > 0.0).collect();
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}

fn text_extent(units: &[(Rect, Unit)]) -> (f64, f64) {
    let mut left = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    for (rect, unit) in units {
        if matches!(unit, Unit::Lines(_)) {
            left = left.min(rect.x0);
            right = right.max(rect.x1);
        }
    }
    if left.is_finite() {
        (left, right)
    } else {
        (0.0, 0.0)
    }
}

fn joins_without_space(before: &str, after: &str) -> bool {
    let last = before
        .chars()
        .rev()
        .find(|c| !c.is_whitespace())
        .map(Script::of);
    let first = after.chars().find(|c| !c.is_whitespace()).map(Script::of);
    matches!((last, first), (Some(a), Some(b)) if a.joins_words() && b.joins_words())
}

fn push_text(runs: &mut Vec<Run>, text: &str, style: &model::Style) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.style == *style => last.text.push_str(text),
        _ => runs.push(Run {
            text: text.to_owned(),
            style: style.clone(),
        }),
    }
}

fn paragraphs(lines: &[Line], column: (f64, f64)) -> Vec<Paragraph> {
    let mut out = Vec::new();
    let mut start = 0;
    for (at, line) in lines.iter().enumerate() {
        if line.end == End::Paragraph || at + 1 == lines.len() {
            out.push(paragraph(&lines[start..=at], column));
            start = at + 1;
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    Rtl,
    Ltr,
    Neutral,
}

fn way_of(text: &str) -> Way {
    let mut way = Way::Neutral;
    for c in text.chars() {
        let script = Script::of(c);
        if script.is_right_to_left() && !c.is_numeric() {
            return Way::Rtl;
        }
        if c.is_alphanumeric() {
            way = Way::Ltr;
        }
    }
    way
}

fn logical_order<S>(pieces: Vec<(String, S)>) -> Vec<(String, S)> {
    let ways: Vec<Way> = pieces.iter().map(|(text, _)| way_of(text)).collect();
    let rtl = ways.iter().filter(|w| **w == Way::Rtl).count();
    let ltr = ways.iter().filter(|w| **w == Way::Ltr).count();
    if rtl == 0 || rtl < ltr {
        return pieces;
    }
    let mut reversed: Vec<((String, S), Way)> = pieces.into_iter().zip(ways).rev().collect();
    let mut at = 0;
    while at < reversed.len() {
        if reversed[at].1 != Way::Ltr {
            at += 1;
            continue;
        }
        let mut end = at;
        let mut last_ltr = at;
        while end < reversed.len() && reversed[end].1 != Way::Rtl {
            if reversed[end].1 == Way::Ltr {
                last_ltr = end;
            }
            end += 1;
        }
        reversed[at..=last_ltr].reverse();
        at = last_ltr + 1;
    }
    reversed.into_iter().map(|(piece, _)| piece).collect()
}

fn paragraph(lines: &[Line], column: (f64, f64)) -> Paragraph {
    let mut runs: Vec<Run> = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let mut pieces: Vec<(String, &model::Style)> = Vec::with_capacity(line.clusters.len());
        let mut previous: Option<&Glyphs> = None;
        for glyphs in &line.clusters {
            if let Some(before) = previous
                && line.horizontal
                && !before.blank
                && !glyphs.blank
            {
                let gap = glyphs.rect.x0 - before.rect.x1;
                let em = before.style.size.max(glyphs.style.size).max(1.0);
                if gap > 0.3 * em && !before.text.ends_with(char::is_whitespace) {
                    pieces.push((" ".to_owned(), &before.style));
                }
            }
            pieces.push((glyphs.text.clone(), &glyphs.style));
            previous = Some(glyphs);
        }
        for (text, style) in logical_order(pieces) {
            push_text(&mut runs, &text, style);
        }
        if at + 1 < lines.len() {
            let next = &lines[at + 1];
            let before: String = line
                .clusters
                .iter()
                .rev()
                .take(2)
                .map(|g| g.text.as_str())
                .collect();
            let after: String = next
                .clusters
                .iter()
                .take(2)
                .map(|g| g.text.as_str())
                .collect();
            let ends_blank = line.clusters.last().is_some_and(|g| g.blank);
            let space = match line.end {
                End::Wrap => false,
                End::WrapWithSpace | End::Paragraph => {
                    !joins_without_space(&before, &after) && !ends_blank
                }
            };
            let hyphen =
                before.ends_with('-') && after.chars().next().is_some_and(char::is_lowercase);
            if hyphen {
                if let Some(last) = runs.last_mut() {
                    last.text.pop();
                }
            } else if space && let Some(last) = line.clusters.last() {
                push_text(&mut runs, " ", &last.style);
            }
        }
    }
    tidy_spaces(&mut runs);
    let frame = lines
        .iter()
        .skip(1)
        .fold(lines[0].rect, |rect, line| rect.union(&line.rect));
    let pitch = if lines.len() > 1 {
        (lines[lines.len() - 1].baseline - lines[0].baseline) / (lines.len() - 1) as f64
    } else {
        0.0
    };
    let (align, first_indent) = alignment(lines, column);
    Paragraph {
        runs,
        frame,
        align,
        first_indent,
        lines: lines.len(),
        pitch,
        ..Paragraph::default()
    }
}

fn tidy_spaces(runs: &mut Vec<Run>) {
    let mut last_space = true;
    for run in runs.iter_mut() {
        let mut text = String::with_capacity(run.text.len());
        for c in run.text.chars() {
            let space = c == ' '
                || c == '\u{a0}'
                || c == '\t'
                || ('\u{2000}'..='\u{200a}').contains(&c)
                || c == '\u{3000}';
            if space {
                if !last_space {
                    text.push(' ');
                }
                last_space = true;
            } else {
                text.push(c);
                last_space = false;
            }
        }
        run.text = text;
    }
    if let Some(last) = runs.iter_mut().rev().find(|run| !run.text.is_empty()) {
        let trimmed = last.text.trim_end_matches(' ').len();
        last.text.truncate(trimmed);
    }
    runs.retain(|run| !run.text.is_empty());
}

fn spread(values: impl Iterator<Item = f64>) -> f64 {
    let (lo, hi) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    if lo.is_finite() { hi - lo } else { 0.0 }
}

fn alignment(lines: &[Line], (left, right): (f64, f64)) -> (Align, f64) {
    if lines.len() == 1 {
        let rect = lines[0].rect;
        let width = right - left;
        let center = f64::midpoint(rect.x0, rect.x1);
        if width > 0.0
            && rect.width() < 0.8 * width
            && (center - f64::midpoint(left, right)).abs() < 4.0
            && rect.x0 - left > 12.0
        {
            return (Align::Center, 0.0);
        }
        let em = lines[0].em.max(1.0);
        if width > 0.0
            && (right - rect.x1).abs() < (0.5 * em).max(3.0)
            && rect.x0 - left > 0.3 * width
        {
            return (Align::Right, 0.0);
        }
        return (Align::Left, 0.0);
    }
    let rest_left = lines[1..]
        .iter()
        .map(|l| l.rect.x0)
        .fold(f64::INFINITY, f64::min);
    let first_indent = lines[0].rect.x0 - rest_left;
    let lefts = spread(lines[1..].iter().map(|l| l.rect.x0));
    let rights = spread(lines[..lines.len() - 1].iter().map(|l| l.rect.x1));
    let centers = spread(lines.iter().map(|l| f64::midpoint(l.rect.x0, l.rect.x1)));
    let em = lines[0].em.max(1.0);
    let align = if lines.len() >= 3 && lefts <= 0.2 * em && rights <= 0.2 * em {
        Align::Justify
    } else if centers <= 0.25 * em && lefts > 0.5 * em {
        Align::Center
    } else if rights <= 0.2 * em && lefts > 0.5 * em {
        Align::Right
    } else {
        Align::Left
    };
    let first_indent = if matches!(align, Align::Left | Align::Justify) {
        first_indent
    } else {
        0.0
    };
    (align, first_indent)
}

fn table(grid: &tables::Grid, mut lines: Vec<((usize, usize), Line)>) -> Table {
    lines.sort_by(|a, b| {
        (a.0, a.1.baseline, a.1.rect.x0)
            .partial_cmp(&(b.0, b.1.baseline, b.1.rect.x0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let columns: Vec<f64> = grid.xs.windows(2).map(|w| w[1] - w[0]).collect();
    let mut rows = Vec::with_capacity(grid.rows());
    for r in 0..grid.rows() {
        let mut cells = Vec::with_capacity(grid.columns());
        for c in 0..grid.columns() {
            if grid.owner[r][c] != (r, c) {
                cells.push(Cell {
                    covered: true,
                    span: (1, 1),
                    ..Cell::default()
                });
                continue;
            }
            let span = grid.span(r, c);
            let mine: Vec<Line> = lines
                .iter()
                .filter(|(cell, _)| *cell == (r, c))
                .map(|(_, line)| line.clone())
                .collect();
            let x0 = grid.xs[c];
            let x1 = grid.xs[(c + span.0).min(grid.columns())];
            let mut paragraphs = paragraphs_in_cell(&mine, (x0 + 2.0, x1 - 2.0));
            space_between_paragraphs(&mut paragraphs);
            cells.push(Cell {
                paragraphs,
                span,
                covered: false,
                fill: grid.fills[r][c],
            });
        }
        rows.push(TableRow {
            height: grid.ys[r + 1] - grid.ys[r],
            cells,
        });
    }
    Table {
        frame: grid.frame,
        columns,
        rows,
    }
}

fn paragraphs_in_cell(lines: &[Line], column: (f64, f64)) -> Vec<Paragraph> {
    if lines.is_empty() {
        return Vec::new();
    }
    paragraphs(lines, column)
}

fn space_between_paragraphs(paragraphs: &mut [Paragraph]) {
    for at in 1..paragraphs.len() {
        let gap = paragraphs[at].frame.y0 - paragraphs[at - 1].frame.y1;
        paragraphs[at].space_before = gap.clamp(0.0, 48.0);
    }
}

fn space_between(blocks: &mut [Block]) {
    for at in 1..blocks.len() {
        let before = blocks[at - 1].frame();
        if let Block::Paragraph(paragraph) = &mut blocks[at] {
            let overlap = paragraph.frame.x1.min(before.x1) - paragraph.frame.x0.max(before.x0);
            let gap = paragraph.frame.y0 - before.y1;
            if overlap > 0.0 && gap > 0.0 {
                paragraph.space_before = gap.min(48.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_to_left_lines_read_in_logical_order() {
        let drawn = [
            "١", "٢", "٣", " ", "ر", "ا", "ب", "ت", "خ", "ا", " ", "ا", "ب", "ح", "ر", "م",
        ];
        let pieces: Vec<(String, ())> = drawn.iter().map(|s| ((*s).to_owned(), ())).collect();
        let text: String = logical_order(pieces).into_iter().map(|(t, ())| t).collect();
        assert_eq!(text, "مرحبا اختبار ١٢٣");
        let latin: Vec<(String, ())> = ["a", "b"].iter().map(|s| ((*s).to_owned(), ())).collect();
        let text: String = logical_order(latin).into_iter().map(|(t, ())| t).collect();
        assert_eq!(text, "ab");
    }
}
