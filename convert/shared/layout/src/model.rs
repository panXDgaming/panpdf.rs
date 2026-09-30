pub type Rgb = [u8; 3];

pub const BLACK: Rgb = [0, 0, 0];

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub family: String,
    pub family_complex: Option<String>,
    pub size: f32,
    pub size_complex: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub colour: Rgb,
    pub background: Option<Rgb>,
    pub shift: VerticalShift,
    pub letter_spacing: f32,
    pub link: Option<String>,
    pub caps: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: "Liberation Serif".into(),
            family_complex: None,
            size: 12.0,
            size_complex: None,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            colour: BLACK,
            background: None,
            shift: VerticalShift::None,
            letter_spacing: 0.0,
            link: None,
            caps: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VerticalShift {
    #[default]
    None,
    Super,
    Sub,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text {
        text: String,
        style: TextStyle,
    },
    Image {
        image: ImageRef,
        width: f32,
        height: f32,
    },
    LineBreak,
    Tab {
        style: TextStyle,
    },
    Anchor(usize),
}

pub type ImageRef = usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageData {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    Multiple(f32),
    OfSize(f32),
    Exact(f32),
    AtLeast(f32),
}

impl Default for LineHeight {
    fn default() -> Self {
        Self::Multiple(1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f32,
    pub colour: Rgb,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Sides<T> {
    pub const fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub text: String,
    pub style: TextStyle,
    pub outside: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStop {
    pub pos: f32,
    pub right: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Paragraph {
    pub inlines: Vec<Inline>,
    pub align: Align,
    pub indent_left: f32,
    pub indent_right: f32,
    pub indent_first: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub line_height: LineHeight,
    pub marker: Option<Marker>,
    pub keep_with_next: bool,
    pub page_break_before: bool,
    pub background: Option<Rgb>,
    pub borders: Sides<Option<Border>>,
    pub tabs: Vec<TabStop>,
    pub empty_style: TextStyle,
    pub direction: Direction,
    pub preformatted: bool,
}

impl Default for Paragraph {
    fn default() -> Self {
        Self {
            inlines: Vec::new(),
            align: Align::Left,
            indent_left: 0.0,
            indent_right: 0.0,
            indent_first: 0.0,
            space_before: 0.0,
            space_after: 0.0,
            line_height: LineHeight::default(),
            marker: None,
            keep_with_next: false,
            page_break_before: false,
            background: None,
            borders: Sides::default(),
            tabs: Vec::new(),
            empty_style: TextStyle::default(),
            direction: Direction::Ltr,
            preformatted: false,
        }
    }
}

impl Paragraph {
    #[must_use]
    pub fn is_rtl(&self) -> bool {
        match self.direction {
            Direction::Ltr => false,
            Direction::Rtl => true,
            Direction::Auto => {
                let chars: Vec<char> = self.text().chars().collect();
                convert_pdf_canvas::bidi::paragraph(&chars, None).level == 1
            }
        }
    }

    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for inline in &self.inlines {
            match inline {
                Inline::Text { text, .. } => out.push_str(text),
                Inline::LineBreak => out.push('\n'),
                Inline::Tab { .. } => out.push('\t'),
                Inline::Image { .. } | Inline::Anchor(_) => {}
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TableWidth {
    #[default]
    Columns,
    Fixed(f32),
    Percent(f32),
    Auto,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub columns: Vec<f32>,
    pub width: TableWidth,
    pub rows: Vec<Row>,
    pub indent: f32,
    pub align: Align,
    pub space_before: f32,
    pub space_after: f32,
    pub padding: Sides<f32>,
    pub outer_border: Option<Border>,
    pub collapse: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub min_height: f32,
    pub exact_height: Option<f32>,
    pub header: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VMerge {
    #[default]
    None,
    Restart,
    Continue,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub blocks: Vec<Block>,
    pub span: usize,
    pub row_span: usize,
    pub vmerge: VMerge,
    pub background: Option<Rgb>,
    pub borders: Sides<Option<Border>>,
    pub padding: Option<Sides<f32>>,
    pub valign: VAlign,
    pub width: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "paragraphs are most blocks; boxing them would cost more than the size"
)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Rule {
        width: f32,
        colour: Rgb,
        space_before: f32,
        space_after: f32,
    },
    PageBreak,
    SectionBreak(PageSetup),
    Group(Group),
    Columns {
        count: usize,
        gap: f32,
        blocks: Vec<Block>,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub blocks: Vec<Block>,
    pub margin_left: f32,
    pub margin_right: f32,
    pub padding: Sides<f32>,
    pub background: Option<Rgb>,
    pub borders: Sides<Option<Border>>,
    pub space_before: f32,
    pub space_after: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin: Sides<f32>,
    pub header_distance: f32,
    pub footer_distance: f32,
}

impl PageSetup {
    #[must_use]
    pub const fn a4() -> Self {
        Self {
            width: 595.276,
            height: 841.89,
            margin: Sides::all(72.0),
            header_distance: 36.0,
            footer_distance: 36.0,
        }
    }

    #[must_use]
    pub const fn letter() -> Self {
        Self {
            width: 612.0,
            height: 792.0,
            margin: Sides::all(72.0),
            header_distance: 36.0,
            footer_distance: 36.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    PageNumber,
    PageCount,
}

pub const FIELD_PAGE: &str = "\u{E000}PAGE\u{E000}";
pub const FIELD_PAGES: &str = "\u{E000}NUMPAGES\u{E000}";

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub setup: PageSetup,
    pub header: Vec<Block>,
    pub footer: Vec<Block>,
    pub first_header: Option<Vec<Block>>,
    pub first_footer: Option<Vec<Block>>,
    pub blocks: Vec<Block>,
    pub columns: usize,
    pub column_gap: f32,
}

impl Section {
    #[must_use]
    pub const fn new(setup: PageSetup) -> Self {
        Self {
            setup,
            header: Vec::new(),
            footer: Vec::new(),
            first_header: None,
            first_footer: None,
            blocks: Vec::new(),
            columns: 1,
            column_gap: 36.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub sections: Vec<Section>,
    pub images: Vec<ImageData>,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub collapse_margins: bool,
    pub notes: Vec<Vec<Block>>,
}
