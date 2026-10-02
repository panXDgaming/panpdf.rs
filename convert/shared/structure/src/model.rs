#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    #[must_use]
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    #[must_use]
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    #[must_use]
    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    #[must_use]
    pub fn center(&self) -> (f64, f64) {
        ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    #[must_use]
    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        self.x0 <= x && x <= self.x1 && self.y0 <= y && y <= self.y1
    }

    #[must_use]
    pub fn overlap(&self, other: &Self) -> f64 {
        let w = self.x1.min(other.x1) - self.x0.max(other.x0);
        let h = self.y1.min(other.y1) - self.y0.max(other.y0);
        if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
    }

    #[must_use]
    pub fn area(&self) -> f64 {
        self.width().max(0.0) * self.height().max(0.0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Document {
    pub pages: Vec<Page>,
    pub body_size: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    pub index: usize,
    pub width: f64,
    pub height: f64,
    pub blocks: Vec<Block>,
    pub header: Vec<Paragraph>,
    pub footer: Vec<Paragraph>,
    pub refused: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Picture(Picture),
}

impl Block {
    #[must_use]
    pub fn frame(&self) -> Rect {
        match self {
            Self::Paragraph(paragraph) => paragraph.frame,
            Self::Table(table) => table.frame,
            Self::Picture(picture) => picture.frame,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Body,
    Heading(u8),
    ListItem(ListItem),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub marker: String,
    pub kind: ListKind,
    pub level: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Decimal(u32),
    LowerLetter(u32),
    UpperLetter(u32),
    Kept,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug, Default)]
pub struct Paragraph {
    pub role: Role,
    pub runs: Vec<Run>,
    pub frame: Rect,
    pub align: Align,
    pub first_indent: f64,
    pub lines: usize,
    pub pitch: f64,
    pub space_before: f64,
}

impl Paragraph {
    #[must_use]
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }

    #[must_use]
    pub fn text_with_marker(&self) -> String {
        match &self.role {
            Role::ListItem(item) if item.kind != ListKind::Kept => {
                format!("{} {}", item.marker, self.text())
            }
            _ => self.text(),
        }
    }

    #[must_use]
    pub fn size(&self) -> f64 {
        let mut best = (0.0, 0_usize);
        let mut sizes: Vec<(f64, usize)> = Vec::new();
        for run in &self.runs {
            let count = run.text.chars().filter(|c| !c.is_whitespace()).count();
            let size = (run.style.size * 2.0).round() / 2.0;
            match sizes.iter_mut().find(|(s, _)| (*s - size).abs() < 0.01) {
                Some(entry) => entry.1 += count,
                None => sizes.push((size, count)),
            }
        }
        for (size, count) in sizes {
            if count > best.1 || (count == best.1 && size > best.0) {
                best = (size, count);
            }
        }
        best.0
    }

    #[must_use]
    pub fn all_bold(&self) -> bool {
        let mut any = false;
        for run in &self.runs {
            if run.text.chars().any(|c| !c.is_whitespace()) {
                if !run.style.bold {
                    return false;
                }
                any = true;
            }
        }
        any
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub family: String,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: [u8; 3],
    pub legacy: bool,
    pub baseline: Shift,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shift {
    #[default]
    None,
    Up,
    Down,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub frame: Rect,
    pub columns: Vec<f64>,
    pub rows: Vec<TableRow>,
}

#[derive(Clone, Debug, Default)]
pub struct TableRow {
    pub height: f64,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub paragraphs: Vec<Paragraph>,
    pub span: (usize, usize),
    pub covered: bool,
    pub fill: Option<[u8; 3]>,
}

impl Cell {
    #[must_use]
    pub fn text(&self) -> String {
        self.paragraphs
            .iter()
            .map(Paragraph::text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureFormat {
    Png,
    Jpeg,
}

impl PictureFormat {
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
        }
    }

    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Picture {
    pub frame: Rect,
    pub format: PictureFormat,
    pub data: Vec<u8>,
    pub pixels: (u32, u32),
    pub stored: Option<String>,
    pub background: bool,
}
