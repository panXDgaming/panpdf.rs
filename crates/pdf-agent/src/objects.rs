use std::fmt::Write as _;

use pdf_cli::ObjectBox;
use pdf_edit::{Command, FixedPoint, ObjectSelection, SourceAnchor};
use pdf_paint::Matrix;
use pdf_semantics::ObjectKind;
use pdf_session::PageView;

use crate::desk::{self, Block};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    List {
        page: usize,
    },
    Move {
        object: String,
        left: Option<f64>,
        top: Option<f64>,
    },
    Resize {
        object: String,
        width: Option<f64>,
        height: Option<f64>,
    },
    Delete {
        object: String,
    },
}

impl Action {
    #[must_use]
    pub const fn only_reads(&self) -> bool {
        matches!(self, Self::List { .. })
    }

    #[must_use]
    pub const fn is_destructive(&self) -> bool {
        matches!(self, Self::Delete { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Name {
    Object { page: usize, index: usize },
    Block { page: usize, index: usize },
}

impl Name {
    #[must_use]
    pub const fn page(self) -> usize {
        match self {
            Self::Object { page, .. } | Self::Block { page, .. } => page,
        }
    }
}

#[must_use]
pub fn object_name(page: usize, index: usize) -> String {
    format!("p{}-o{}", page + 1, index + 1)
}

pub fn parse_name(name: &str) -> Result<Name, String> {
    let bad = || {
        format!(
            "{name:?} is not the name of an object: list_objects gives names like p3-o2 for a \
             picture or a drawing, and read_text gives names like p3-b12 for text"
        )
    };
    let rest = name.trim().strip_prefix('p').ok_or_else(bad)?;
    let (page, rest) = rest.split_once('-').ok_or_else(bad)?;
    let page: usize = page.parse().map_err(|_| bad())?;
    let (kind, number) = rest.split_at_checked(1).ok_or_else(bad)?;
    let number: usize = number.parse().map_err(|_| bad())?;
    if page == 0 || number == 0 {
        return Err(bad());
    }
    match kind {
        "o" => Ok(Name::Object {
            page: page - 1,
            index: number - 1,
        }),
        "b" => Ok(Name::Block {
            page: page - 1,
            index: number - 1,
        }),
        _ => Err(bad()),
    }
}

const fn kind_word(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Image => "picture",
        ObjectKind::Form => "group of pictures or drawings",
        ObjectKind::Path => "drawing",
        ObjectKind::Shading => "gradient",
        ObjectKind::Text(_) | ObjectKind::TextRun => "text",
    }
}

fn round(area: [f64; 4]) -> String {
    format!(
        "[{:.0}, {:.0}, {:.0}, {:.0}]",
        area[0], area[1], area[2], area[3]
    )
}

fn short(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 40 {
        flat
    } else {
        let cut: String = flat.chars().take(40).collect();
        format!("{}\u{2026}", cut.trim_end())
    }
}

#[must_use]
pub fn listing(page: usize, objects: &[ObjectBox], blocks: &[Block]) -> String {
    let mut said = format!(
        "Page {} holds {} picture{} and drawing{}, and {} text block{}. Boxes are [left, top, \
         right, bottom] in points from the top-left of the page as shown.",
        page + 1,
        objects.len(),
        if objects.len() == 1 { "" } else { "s" },
        if objects.len() == 1 { "" } else { "s" },
        blocks.len(),
        if blocks.len() == 1 { "" } else { "s" }
    );
    for (index, object) in objects.iter().enumerate() {
        let _ = write!(
            said,
            "\n{} {} {}",
            object_name(page, index),
            kind_word(object.kind),
            round(object.box_pixels)
        );
    }
    for block in blocks {
        let _ = write!(
            said,
            "\n{} text, {} pt {}: \u{201c}{}\u{201d}",
            block.name(),
            block.size,
            round(block.area),
            short(&block.text)
        );
    }
    if !objects.is_empty() {
        said.push_str(
            "\nA picture or drawing is moved, resized or deleted by the name p<page>-o<number>; \
             a name is good until that page changes.",
        );
    }
    said
}

pub fn the_object<'a>(
    objects: &'a [ObjectBox],
    index: usize,
    recorded: Option<&str>,
    name: &str,
) -> Result<(usize, &'a ObjectBox), String> {
    let Some(recorded) = recorded else {
        return Err(format!(
            "{name} has not been listed yet: call list_objects for that page first"
        ));
    };
    if let Some(object) = objects
        .get(index)
        .filter(|object| object.anchor == recorded)
    {
        return Ok((index, object));
    }
    objects
        .iter()
        .enumerate()
        .find(|(_, object)| object.anchor == recorded)
        .ok_or_else(|| {
            format!("{name} is no longer on the page as it was listed: call list_objects again")
        })
}

fn anchors_of_block(parts: &desk::Parts) -> Vec<SourceAnchor> {
    let mut anchors: Vec<SourceAnchor> = Vec::new();
    for cluster in parts.rows.iter().flatten() {
        if !anchors.contains(&cluster.anchor) {
            anchors.push(cluster.anchor.clone());
        }
    }
    anchors
}

fn translation(view: &PageView, dx: f64, dy: f64) -> Result<(f64, f64), String> {
    pdf_cli::page_offset_view(view, 1.0, dx, dy)
}

pub fn command_on_object(
    view: &PageView,
    page: usize,
    object: &ObjectBox,
    action: &Action,
) -> Result<(Command, String), String> {
    let [left, top, right, bottom] = object.box_pixels;
    let (width, height) = (right - left, bottom - top);
    let target = SourceAnchor::decode(&object.anchor)
        .ok_or_else(|| "the object has no usable place in the page".to_owned())?;
    match action {
        Action::Move {
            left: to_left,
            top: to_top,
            ..
        } => {
            if to_left.is_none() && to_top.is_none() {
                return Err(
                    "`left` or `top` is needed: where the object's top-left corner goes".to_owned(),
                );
            }
            let (dx, dy) = (
                to_left.map_or(0.0, |to| to - left),
                to_top.map_or(0.0, |to| to - top),
            );
            let (ux, uy) = translation(view, dx, dy)?;
            Ok((
                Command::PlaceObject {
                    page_index: page,
                    target: ObjectSelection::Painted(target),
                    transform: Matrix {
                        e: ux,
                        f: uy,
                        ..Matrix::IDENTITY
                    },
                    about: FixedPoint::Origin,
                },
                format!(
                    "Moved it to left {:.0}, top {:.0}.",
                    to_left.unwrap_or(left),
                    to_top.unwrap_or(top)
                ),
            ))
        }
        Action::Resize {
            width: to_width,
            height: to_height,
            ..
        } => {
            let (new_width, new_height) = match (to_width, to_height) {
                (None, None) => {
                    return Err(
                        "`width` or `height` is needed: how big the object becomes, in \
                                points"
                            .to_owned(),
                    );
                }
                (Some(wide), Some(high)) => (*wide, *high),
                (Some(wide), None) => (*wide, wide * height / width),
                (None, Some(high)) => (high * width / height, *high),
            };
            if !(new_width > 0.0 && new_height > 0.0 && width > 0.0 && height > 0.0) {
                return Err("an object cannot be resized to no size".to_owned());
            }
            let scale = Matrix {
                a: new_width / width,
                d: new_height / height,
                ..Matrix::IDENTITY
            };
            let (transform, held) = pdf_cli::page_transform_view(view, 1.0, scale, (left, top))?;
            Ok((
                Command::PlaceObject {
                    page_index: page,
                    target: ObjectSelection::Painted(target),
                    transform,
                    about: FixedPoint::At(held),
                },
                format!(
                    "Resized it to {new_width:.0} x {new_height:.0} pt, its top-left corner where it was."
                ),
            ))
        }
        Action::Delete { .. } => Ok((
            Command::RemoveObject {
                page_index: page,
                target,
            },
            format!("Deleted the {}.", kind_word(object.kind)),
        )),
        Action::List { .. } => Err("listing is not a change".to_owned()),
    }
}

pub fn command_on_block(
    view: &PageView,
    page: usize,
    index: usize,
    action: &Action,
) -> Result<(Command, String), String> {
    let (block, parts) = desk::read(view, page, index)
        .ok_or_else(|| format!("p{}-b{} cannot be read as text", page + 1, index + 1))?;
    let [left, top, ..] = block.area;
    match action {
        Action::Move {
            left: to_left,
            top: to_top,
            ..
        } => {
            if let Some(why) = block.fixed {
                return Err(format!("{} cannot be moved: {why}", block.name()));
            }
            if to_left.is_none() && to_top.is_none() {
                return Err(
                    "`left` or `top` is needed: where the block's top-left corner goes".to_owned(),
                );
            }
            let (dx, dy) = (
                to_left.map_or(0.0, |to| to - left),
                to_top.map_or(0.0, |to| to - top),
            );
            let (ux, uy) = translation(view, dx, dy)?;
            Ok((
                Command::MoveTextBlock {
                    page_index: page,
                    runs: anchors_of_block(&parts),
                    dx: ux,
                    dy: uy,
                },
                format!(
                    "Moved {} to left {:.0}, top {:.0}.",
                    block.name(),
                    to_left.unwrap_or(left),
                    to_top.unwrap_or(top)
                ),
            ))
        }
        Action::Resize { .. } => Err(format!(
            "{} is text: change how large it is with style_text and `size`",
            block.name()
        )),
        Action::Delete { .. } => Err(format!(
            "{} is text: delete it with replace_text, giving an empty `text`",
            block.name()
        )),
        Action::List { .. } => Err("listing is not a change".to_owned()),
    }
}

#[cfg(test)]
mod tests;
