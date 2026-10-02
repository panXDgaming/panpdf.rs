#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    Paragraph {
        inlines: Vec<Inline>,
    },
    Code {
        info: String,
        text: String,
    },
    Break,
    Quote {
        blocks: Vec<Block>,
    },
    List(List),
    Table {
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        align: Vec<Align>,
    },
    Html(String),
    Footnotes(Vec<Vec<Block>>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct List {
    pub first: Option<u64>,
    pub loose: bool,
    pub items: Vec<Vec<Block>>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Align {
    #[default]
    Start,
    Left,
    Middle,
    End,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Inline {
    Text(String),
    Soft,
    Hard,
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Strike(Vec<Inline>),
    Code(String),
    Link {
        to: String,
        title: String,
        text: Vec<Inline>,
    },
    Image {
        at: String,
        title: String,
        text: Vec<Inline>,
    },
    Html(String),
    Check(bool),
    Note(usize),
    Highlight(Vec<Inline>),
    Superscript(Vec<Inline>),
    Subscript(Vec<Inline>),
}

impl Inline {
    #[must_use]
    pub fn plain(&self) -> String {
        match self {
            Self::Text(text) | Self::Code(text) => text.clone(),
            Self::Soft => " ".to_owned(),
            Self::Hard => "\n".to_owned(),
            Self::Html(_) => String::new(),
            Self::Check(ticked) => if *ticked { "\u{2611} " } else { "\u{2610} " }.to_owned(),
            Self::Note(number) => super::present::superscript(&number.to_string())
                .unwrap_or_else(|| format!("[{number}]")),
            Self::Superscript(inside) => {
                let text = plain(inside);
                super::present::superscript(&text).unwrap_or(text)
            }
            Self::Subscript(inside) => {
                let text = plain(inside);
                super::present::subscript(&text).unwrap_or(text)
            }
            Self::Emphasis(inside)
            | Self::Strong(inside)
            | Self::Strike(inside)
            | Self::Highlight(inside)
            | Self::Link { text: inside, .. }
            | Self::Image { text: inside, .. } => plain(inside),
        }
    }
}

#[must_use]
pub fn plain(inlines: &[Inline]) -> String {
    inlines.iter().map(Inline::plain).collect()
}
