use pdf_edit::{Command, PenStep, PenStroke};
use pdf_paint::Matrix;

use crate::composing::shapes;
use crate::desk;

pub const DEFAULT_WIDTH: f64 = 2.0;

pub const MOST_WIDTH: f64 = 100.0;

const LEAST_SIDE: f64 = 1.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    Rectangle,
    Ellipse,
    Line,
    Arrow,
}

impl Shape {
    #[must_use]
    pub fn of(word: &str) -> Option<Self> {
        match word.trim() {
            "rectangle" | "box" => Some(Self::Rectangle),
            "ellipse" | "oval" | "circle" => Some(Self::Ellipse),
            "line" => Some(Self::Line),
            "arrow" => Some(Self::Arrow),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rectangle => "rectangle",
            Self::Ellipse => "ellipse",
            Self::Line => "line",
            Self::Arrow => "arrow",
        }
    }

    #[must_use]
    pub const fn with_article(self) -> &'static str {
        match self {
            Self::Rectangle => "a rectangle",
            Self::Ellipse => "an ellipse",
            Self::Line => "a line",
            Self::Arrow => "an arrow",
        }
    }

    #[must_use]
    pub const fn is_closed(self) -> bool {
        matches!(self, Self::Rectangle | Self::Ellipse)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub page: usize,
    pub shape: Shape,
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub colour: [f64; 3],
    pub width: f64,
    pub fill: Option<[f64; 3]>,
}

impl Asked {
    pub fn check(&self) -> Result<(), String> {
        let (across, down) = (
            (self.to.0 - self.from.0).abs(),
            (self.to.1 - self.from.1).abs(),
        );
        let fits = if self.shape.is_closed() {
            across >= LEAST_SIDE && down >= LEAST_SIDE
        } else {
            across.hypot(down) >= LEAST_SIDE
        };
        if fits {
            Ok(())
        } else {
            Err(format!(
                "the {} would be less than {LEAST_SIDE:.0} point across: `left`, `top`, `right` \
                 and `bottom` are its corners",
                self.shape.as_str()
            ))
        }
    }

    #[must_use]
    pub fn steps(&self) -> Vec<PenStep> {
        let (left, right) = (self.from.0.min(self.to.0), self.from.0.max(self.to.0));
        let (top, bottom) = (self.from.1.min(self.to.1), self.from.1.max(self.to.1));
        match self.shape {
            Shape::Line => shapes::line(self.from.0, self.from.1, self.to.0, self.to.1),
            Shape::Arrow => shapes::arrow(self.from, self.to, self.width),
            Shape::Rectangle => vec![
                PenStep::Move((left, top)),
                PenStep::Line((right, top)),
                PenStep::Line((right, bottom)),
                PenStep::Line((left, bottom)),
            ],
            Shape::Ellipse => shapes::ellipse(
                f64::midpoint(left, right),
                f64::midpoint(top, bottom),
                (right - left) / 2.0,
                (bottom - top) / 2.0,
            ),
        }
    }

    #[must_use]
    pub fn bounds(&self) -> [f64; 4] {
        [
            self.from.0.min(self.to.0),
            self.from.1.min(self.to.1),
            self.from.0.max(self.to.0),
            self.from.1.max(self.to.1),
        ]
    }
}

pub fn command(asked: &Asked, shown: &Matrix, page_size: [f64; 2]) -> Result<Command, String> {
    asked.check()?;
    let [left, top, right, bottom] = asked.bounds();
    if right < 0.0 || bottom < 0.0 || left > page_size[0] || top > page_size[1] {
        return Err(format!(
            "the {} lies off the page, which is {:.0} x {:.0} pt: positions are points from the \
             top-left corner",
            asked.shape.as_str(),
            page_size[0],
            page_size[1]
        ));
    }
    let steps = desk::steps_in_user_space_with(shown, &asked.steps())?;
    let scale = shown.a.mul_add(shown.d, -(shown.b * shown.c)).abs().sqrt();
    if scale <= f64::MIN_POSITIVE {
        return Err("this page has no size".to_owned());
    }
    let width = ((asked.width / scale) * 1000.0).round() / 1000.0;
    Ok(Command::DrawPath {
        page_index: asked.page,
        steps,
        closed: asked.shape.is_closed(),
        stroke: Some(PenStroke::pen(asked.colour, width)),
        fill: asked.fill.filter(|_| asked.shape.is_closed()),
    })
}

#[must_use]
pub fn said(asked: &Asked) -> String {
    let [left, top, right, bottom] = asked.bounds();
    let filled = if asked.fill.is_some() && asked.shape.is_closed() {
        ", filled"
    } else {
        ""
    };
    format!(
        "Drew {} on page {} over [{left:.0}, {top:.0}, {right:.0}, {bottom:.0}]{filled}, as one \
         step undo takes back. objects with action list names it.",
        asked.shape.with_article(),
        asked.page + 1
    )
}

#[cfg(test)]
mod tests;
