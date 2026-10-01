use std::fmt::Write as _;

use pdf_edit::Command;
use pdf_edit::link::{Arrival, Listed, Look, Target};
use pdf_paint::Matrix;
use pdf_syntax::Reference;

use crate::desk;

pub const MOST_LISTED: usize = 200;

const SMALLEST: f64 = 3.0;

const LONGEST_ADDRESS: usize = 4_096;

#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Block(String),
    Area([f64; 4]),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Goes {
    Address(String),
    Page(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    List {
        page: usize,
    },
    Add {
        page: usize,
        place: Place,
        goes: Goes,
    },
    Remove {
        link: String,
    },
}

impl Action {
    #[must_use]
    pub const fn only_reads(&self) -> bool {
        matches!(self, Self::List { .. })
    }

    #[must_use]
    pub const fn is_destructive(&self) -> bool {
        matches!(self, Self::Remove { .. })
    }
}

#[must_use]
pub fn link_name(page: usize, index: usize) -> String {
    format!("p{}-l{}", page + 1, index + 1)
}

pub fn parse_name(name: &str) -> Result<(usize, usize), String> {
    let bad = || {
        format!("{name:?} is not a link name: they look like p3-l2, from links with action list")
    };
    let rest = name.trim().strip_prefix('p').ok_or_else(bad)?;
    let (page, link) = rest.split_once("-l").ok_or_else(bad)?;
    let page: usize = page.parse().map_err(|_| bad())?;
    let link: usize = link.parse().map_err(|_| bad())?;
    if page == 0 || link == 0 {
        return Err(bad());
    }
    Ok((page - 1, link - 1))
}

#[must_use]
pub fn anchor_of(reference: Reference) -> String {
    format!("{} {}", reference.object_number(), reference.generation())
}

#[must_use]
pub fn is_an_address(address: &str) -> bool {
    let Some((scheme, rest)) = address.split_once(':') else {
        return false;
    };
    ["http", "https", "mailto"]
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
        && !rest.is_empty()
        && address.len() <= LONGEST_ADDRESS
        && !address
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

#[must_use]
pub fn goes_in_words(target: Option<&Target>) -> String {
    match target {
        Some(Target::Page(page, _)) => format!("goes to page {}", page + 1),
        Some(Target::Address(address)) => format!("goes to {address}"),
        Some(Target::Name(name)) => format!("goes to the named place \u{201c}{name}\u{201d}"),
        Some(Target::Document { file, page, .. }) => {
            format!("goes to page {} of the file {file}", page + 1)
        }
        None => "goes nowhere this can read".to_owned(),
    }
}

pub struct Listing {
    pub text: String,
    pub anchors: Vec<String>,
}

#[must_use]
pub fn listing(page: usize, found: &[Listed], device: &pdf_render::DeviceTransform) -> Listing {
    let mut text = if found.is_empty() {
        format!("Page {} has no links.", page + 1)
    } else {
        format!(
            "Page {} has {} link{}, boxes as [left, top, right, bottom] in points:",
            page + 1,
            found.len(),
            if found.len() == 1 { "" } else { "s" }
        )
    };
    let mut anchors = Vec::with_capacity(found.len());
    for (index, (reference, rect, target)) in found.iter().enumerate() {
        anchors.push(anchor_of(*reference));
        if index >= MOST_LISTED {
            continue;
        }
        let [left, top, right, bottom] = desk::to_shown(device, *rect);
        let _ = write!(
            text,
            "\n{} [{left:.0}, {top:.0}, {right:.0}, {bottom:.0}] {}",
            link_name(page, index),
            goes_in_words(target.as_ref())
        );
    }
    if found.len() > MOST_LISTED {
        let _ = write!(
            text,
            "\n{} more links are not listed.",
            found.len() - MOST_LISTED
        );
    }
    Listing { text, anchors }
}

pub fn the_link(
    found: &[Listed],
    index: usize,
    recorded: Option<&str>,
    name: &str,
) -> Result<Reference, String> {
    let Some(recorded) = recorded else {
        return Err(format!(
            "{name} has not been listed yet: call links with action list for that page first"
        ));
    };
    match found.get(index) {
        Some((reference, _, _)) if anchor_of(*reference) == recorded => Ok(*reference),
        _ => Err(format!(
            "{name} is not the link it was when it was listed, because the page changed: call \
             links with action list again"
        )),
    }
}

pub fn target_of(goes: &Goes, pages: usize) -> Result<Target, String> {
    match goes {
        Goes::Address(address) => {
            if is_an_address(address) {
                Ok(Target::Address(address.clone()))
            } else {
                Err(format!(
                    "{address:?} is not an address a link can open: it starts with http://, \
                     https:// or mailto: and has no spaces"
                ))
            }
        }
        Goes::Page(page) if *page < pages => Ok(Target::Page(*page, Arrival::InheritZoom)),
        Goes::Page(page) => Err(format!(
            "there is no page {}: the document has {pages}",
            page + 1
        )),
    }
}

pub fn add_command(
    page: usize,
    matrix: &Matrix,
    (area, goes): ([f64; 4], &Goes),
    pages: usize,
) -> Result<Command, String> {
    let target = target_of(goes, pages)?;
    let [left, top, right, bottom] = area;
    if !(right - left >= SMALLEST && bottom - top >= SMALLEST) {
        return Err(format!(
            "the area [{left:.0}, {top:.0}, {right:.0}, {bottom:.0}] is too small for a link: it \
             is at least {SMALLEST:.0} points each way, as [left, top, right, bottom]"
        ));
    }
    let rect = desk::to_user_with(matrix, area)?;
    Ok(Command::AddLink {
        page_index: page,
        rect,
        target,
        look: Look::default(),
    })
}

#[must_use]
pub fn remove_command(page: usize, link: Reference) -> Command {
    Command::RemoveLink {
        page_index: page,
        link,
    }
}

#[must_use]
pub fn said_added(page: usize, area: [f64; 4], goes: &Goes) -> String {
    let to = match goes {
        Goes::Address(address) => address.clone(),
        Goes::Page(page) => format!("page {}", page + 1),
    };
    format!(
        "Added a link on page {} over [{:.0}, {:.0}, {:.0}, {:.0}] that goes to {to}, as one step \
         undo takes back. Links with action list names it.",
        page + 1,
        area[0],
        area[1],
        area[2],
        area[3]
    )
}

#[must_use]
pub fn said_removed(name: &str) -> String {
    format!(
        "Removed {name}, as one step undo takes back. The links of that page are numbered again: \
         call links with action list before naming one."
    )
}

#[cfg(test)]
mod tests;
