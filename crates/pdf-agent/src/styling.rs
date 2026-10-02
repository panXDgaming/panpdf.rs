use pdf_edit::{Alignment, BlockRange, Command, ParagraphLayout, RowEnds, TextStyle};
use pdf_session::PageView;

use crate::desk::{self, Block};

pub const LEAST_SPACING: f64 = 0.8;

pub const MOST_SPACING: f64 = 10.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Align {
    Left,
    Centre,
    Right,
    Justify,
}

impl Align {
    #[must_use]
    pub fn of(word: &str) -> Option<Self> {
        match word.trim().to_lowercase().as_str() {
            "left" | "start" => Some(Self::Left),
            "center" | "centre" | "middle" => Some(Self::Centre),
            "right" | "end" => Some(Self::Right),
            "justify" | "justified" => Some(Self::Justify),
            _ => None,
        }
    }

    #[must_use]
    pub const fn alignment(self) -> Alignment {
        match self {
            Self::Left => Alignment::Start,
            Self::Centre => Alignment::Centre,
            Self::Right => Alignment::End,
            Self::Justify => Alignment::Justify,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Centre => "centre",
            Self::Right => "right",
            Self::Justify => "justified",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub size: Option<f64>,
    pub fill: Option<[f64; 3]>,
    pub family: Option<String>,
    pub line_spacing: Option<f64>,
    pub align: Option<Align>,
}

impl Look {
    #[must_use]
    pub fn asks_for_nothing(&self) -> bool {
        self == &Self::default()
    }

    #[must_use]
    pub fn text_style(&self, text_size: f64) -> TextStyle {
        TextStyle {
            size: self.size,
            fill: self.fill,
            bold: self.bold,
            italic: self.italic,
            family: self.family.clone(),
            underline: self.underline,
            line_spacing: self.line_spacing.map(|times| times * text_size),
        }
    }

    #[must_use]
    pub fn paragraph(&self) -> ParagraphLayout {
        ParagraphLayout {
            alignment: self.align.map(Align::alignment),
            flow_round: false,
        }
    }

    #[must_use]
    pub fn words(&self) -> String {
        let mut said: Vec<String> = Vec::new();
        let mut flag = |on: Option<bool>, name: &str| match on {
            Some(true) => said.push(name.to_owned()),
            Some(false) => said.push(format!("not {name}")),
            None => {}
        };
        flag(self.bold, "bold");
        flag(self.italic, "italic");
        flag(self.underline, "underlined");
        if let Some(size) = self.size {
            said.push(format!("{size} pt"));
        }
        if let Some([red, green, blue]) = self.fill {
            let byte = |part: f64| {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a colour part between 0 and 1, scaled to a byte"
                )]
                let byte = (part.clamp(0.0, 1.0) * 255.0).round() as u8;
                byte
            };
            said.push(format!(
                "colour #{:02x}{:02x}{:02x}",
                byte(red),
                byte(green),
                byte(blue)
            ));
        }
        if let Some(family) = &self.family {
            said.push(format!("in {family}"));
        }
        if let Some(times) = self.line_spacing {
            said.push(format!("line spacing {times} times the text size"));
        }
        if let Some(align) = self.align {
            said.push(format!("{} text", align.as_str()));
        }
        said.join(", ")
    }
}

pub fn style_command(
    view: &PageView,
    page: usize,
    index: usize,
    find: Option<&str>,
    look: &Look,
) -> Result<(Command, Block), String> {
    let (block, parts) = desk::read(view, page, index)
        .ok_or_else(|| format!("p{}-b{} cannot be read as text", page + 1, index + 1))?;
    if let Some(reason) = block.fixed {
        return Err(format!("{} cannot be styled: {reason}", block.name()));
    }
    let desk::Parts {
        rows,
        frame,
        reading,
    } = parts;
    let range: BlockRange = match find {
        None => crate::finding::whole(&reading),
        Some(find) => desk::range_within(&reading, find)
            .map_err(|problem| format!("{}: {problem}", block.name()))?,
    };
    let text_size = reading.lines.first().map_or(block.size, |line| line.em);
    let breaks = Some(RowEnds::paragraphs((0..rows.len()).collect()));
    Ok((
        Command::StyleBlock {
            page_index: page,
            rows,
            frame,
            edges: (0, 0),
            breaks,
            range,
            style: look.text_style(text_size),
            paragraph: look.paragraph(),
        },
        block,
    ))
}

#[cfg(test)]
mod tests;
