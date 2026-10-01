use std::collections::BTreeMap;
use std::fmt::Write as _;

use pdf_edit::Command;
use pdf_edit::outline::{Bookmark, Change};

use crate::desk::Block;

pub const MOST_HEADINGS: usize = 200;

pub const MOST_LEVELS: usize = 3;

const MOST_TITLE: usize = 120;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Up,
    Down,
    In,
    Out,
}

impl Step {
    #[must_use]
    pub fn of(word: &str) -> Option<Self> {
        match word.trim() {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "in" | "nest" | "indent" => Some(Self::In),
            "out" | "unnest" | "outdent" => Some(Self::Out),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Page(usize),
    Block(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    List,
    Add {
        title: Option<String>,
        place: Place,
        after: Option<usize>,
        inside: Option<usize>,
    },
    Rename {
        bookmark: usize,
        title: String,
    },
    Retarget {
        bookmark: usize,
        page: usize,
    },
    Move {
        bookmark: usize,
        step: Step,
    },
    Delete {
        bookmark: usize,
    },
    FromHeadings {
        replace: bool,
    },
}

impl Action {
    #[must_use]
    pub const fn only_reads(&self) -> bool {
        matches!(self, Self::List)
    }

    #[must_use]
    pub const fn is_destructive(&self) -> bool {
        matches!(
            self,
            Self::Delete { .. } | Self::FromHeadings { replace: true }
        )
    }
}

#[must_use]
pub fn listing(bookmarks: &[Bookmark]) -> String {
    if bookmarks.is_empty() {
        return "The document has no bookmarks.".to_owned();
    }
    let mut said = format!(
        "{} bookmark{}, numbered in the order they are listed:",
        bookmarks.len(),
        if bookmarks.len() == 1 { "" } else { "s" }
    );
    for (at, bookmark) in bookmarks.iter().enumerate() {
        let _ = write!(
            said,
            "\n{}{}. {}{}",
            "  ".repeat(bookmark.depth),
            at + 1,
            bookmark.title,
            bookmark
                .page
                .map_or_else(String::new, |page| format!(" (page {})", page + 1))
        );
    }
    said
}

pub fn named(bookmarks: &[Bookmark], number: usize) -> Result<&Bookmark, String> {
    number
        .checked_sub(1)
        .and_then(|at| bookmarks.get(at))
        .ok_or_else(|| {
            format!(
                "there is no bookmark {number}: the document has {}; the numbers are the ones \
                 the list gives now, and they change when bookmarks are added, moved or deleted",
                bookmarks.len()
            )
        })
}

pub fn moved(bookmarks: &[Bookmark], number: usize, step: Step) -> Result<Change, String> {
    let book = named(bookmarks, number)?;
    let siblings: Vec<&Bookmark> = bookmarks
        .iter()
        .filter(|other| other.parent == book.parent && other.depth == book.depth)
        .collect();
    let at = siblings
        .iter()
        .position(|other| other.reference == book.reference)
        .ok_or("that bookmark cannot be found among its own level")?;
    let to = |after: Option<pdf_syntax::Reference>, inside: bool, before: bool| {
        Ok(Change::Move {
            bookmark: book.reference,
            after,
            inside,
            before,
        })
    };
    match step {
        Step::Up => match at.checked_sub(1).and_then(|above| siblings.get(above)) {
            Some(above) => to(Some(above.reference), false, true),
            None => Err("it is the first of its level, so it cannot go up".to_owned()),
        },
        Step::Down => match siblings.get(at + 1) {
            Some(below) => to(Some(below.reference), false, false),
            None => Err("it is the last of its level, so it cannot go down".to_owned()),
        },
        Step::In => match at.checked_sub(1).and_then(|above| siblings.get(above)) {
            Some(above) => to(Some(above.reference), true, false),
            None => Err("nothing is above it at its level to go inside".to_owned()),
        },
        Step::Out => match book.parent {
            Some(parent) => to(Some(parent), false, false),
            None => Err("it is already at the top level".to_owned()),
        },
    }
}

pub fn change_for(
    bookmarks: &[Bookmark],
    action: &Action,
    (block, pages): (Option<&(usize, String)>, usize),
) -> Result<Change, String> {
    let on_the_page = |page: usize| {
        if page < pages {
            Ok(page)
        } else {
            Err(format!(
                "there is no page {}: the document has {pages}",
                page + 1
            ))
        }
    };
    match action {
        Action::List | Action::FromHeadings { .. } => {
            Err("that is not one change to the bookmarks".to_owned())
        }
        Action::Add {
            title,
            place,
            after,
            inside,
        } => {
            let (page, found) = match (place, block) {
                (Place::Page(page), _) => (on_the_page(*page)?, None),
                (Place::Block(_), Some((page, text))) => (*page, Some(text.clone())),
                (Place::Block(name), None) => {
                    return Err(format!(
                        "{name} has not been read, so it cannot be bookmarked"
                    ));
                }
            };
            let title = title
                .clone()
                .or(found)
                .map(|title| one_line(&title, MOST_TITLE))
                .filter(|title| !title.is_empty())
                .ok_or("`title` is needed: say what to call the bookmark")?;
            if after.is_some() && inside.is_some() {
                return Err("give `after` or `inside`, not both".to_owned());
            }
            let (after, nested) = match (after, inside) {
                (Some(number), None) => (Some(named(bookmarks, *number)?.reference), false),
                (None, Some(number)) => (Some(named(bookmarks, *number)?.reference), true),
                _ => (None, false),
            };
            Ok(Change::Add {
                title,
                page,
                after,
                inside: nested,
            })
        }
        Action::Rename { bookmark, title } => {
            let book = named(bookmarks, *bookmark)?;
            Ok(Change::Rename {
                bookmark: book.reference,
                title: one_line(title, MOST_TITLE),
            })
        }
        Action::Retarget { bookmark, page } => Ok(Change::Retarget {
            bookmark: named(bookmarks, *bookmark)?.reference,
            page: on_the_page(*page)?,
        }),
        Action::Move { bookmark, step } => moved(bookmarks, *bookmark, *step),
        Action::Delete { bookmark } => Ok(Change::Remove {
            bookmark: named(bookmarks, *bookmark)?.reference,
        }),
    }
}

#[must_use]
pub fn one_line(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= most {
        flat
    } else {
        let cut: String = flat.chars().take(most).collect();
        format!("{}\u{2026}", cut.trim_end())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Heading {
    pub title: String,
    pub page: usize,
    pub level: usize,
}

fn tenths(size: f64) -> i64 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a point size, a few hundred at most"
    )]
    let tenths = (size * 10.0).round() as i64;
    tenths
}

#[must_use]
pub fn headings_in(pages: &[(usize, Vec<Block>)]) -> Vec<Heading> {
    let mut weight: BTreeMap<i64, usize> = BTreeMap::new();
    for (_, blocks) in pages {
        for block in blocks {
            *weight.entry(tenths(block.size)).or_default() += block.text.chars().count();
        }
    }
    let Some((body, _)) = weight.iter().max_by_key(|(_, characters)| **characters) else {
        return Vec::new();
    };
    let body = *body;
    let bigger = |size: i64| size >= body + 10 && size * 100 >= body * 115;
    let mut repeats: BTreeMap<String, usize> = BTreeMap::new();
    let mut found: Vec<(i64, usize, String)> = Vec::new();
    for (page, blocks) in pages {
        for block in blocks {
            let size = tenths(block.size);
            let title = one_line(&block.text, MOST_TITLE);
            if !bigger(size) || title.is_empty() || block.text.chars().count() > 160 {
                continue;
            }
            *repeats.entry(title.clone()).or_default() += 1;
            found.push((size, *page, title));
        }
    }
    let found: Vec<(i64, usize, String)> = found
        .into_iter()
        .filter(|(_, _, title)| repeats.get(title).copied().unwrap_or(0) <= 2)
        .take(MOST_HEADINGS)
        .collect();
    let mut sizes: Vec<i64> = found.iter().map(|(size, ..)| *size).collect();
    sizes.sort_unstable_by(|one, other| other.cmp(one));
    sizes.dedup();
    found
        .into_iter()
        .map(|(size, page, title)| Heading {
            title,
            page,
            level: sizes
                .iter()
                .position(|held| *held == size)
                .unwrap_or(0)
                .min(MOST_LEVELS - 1),
        })
        .collect()
}

#[must_use]
pub fn commands_for_headings(
    headings: &[Heading],
    first_new_object: u32,
    removing: &[Bookmark],
) -> Vec<Command> {
    let mut commands: Vec<Command> = removing
        .iter()
        .filter(|book| book.depth == 0)
        .map(|book| Command::ChangeOutline {
            page_index: 0,
            change: Change::Remove {
                bookmark: book.reference,
            },
        })
        .collect();
    let mut parents: Vec<pdf_syntax::Reference> = Vec::new();
    for (at, heading) in headings.iter().enumerate() {
        let level = heading.level.min(parents.len());
        parents.truncate(level);
        let number = first_new_object.saturating_add(u32::try_from(at).unwrap_or(u32::MAX));
        let change = Change::Add {
            title: heading.title.clone(),
            page: heading.page,
            after: parents.last().copied(),
            inside: !parents.is_empty(),
        };
        parents.push(pdf_syntax::Reference::new(number, 0));
        commands.push(Command::ChangeOutline {
            page_index: heading.page,
            change,
        });
    }
    commands
}

pub fn first_new_object(
    source: &pdf_bytes::ByteStore,
    credential: &[u8],
    fonts: Option<std::sync::Arc<dyn pdf_content::FontProvider>>,
    page: usize,
) -> Result<u32, String> {
    let mut session = pdf_session::Session::with_fonts(source.clone(), credential, fonts);
    let probe = Command::ChangeOutline {
        page_index: page,
        change: Change::Add {
            title: "probe".to_owned(),
            page,
            after: None,
            inside: false,
        },
    };
    let plan = session.plan(&probe).map_err(|error| error.to_string())?;
    plan.writes()
        .iter()
        .map(|write| write.reference.object_number())
        .max()
        .ok_or_else(|| "the bookmarks cannot be added to this document".to_owned())
}

#[must_use]
pub fn said_change(change: &Change, before: &[Bookmark]) -> String {
    let title = |wanted: pdf_syntax::Reference| {
        before
            .iter()
            .find(|book| book.reference == wanted)
            .map_or_else(String::new, |book| book.title.clone())
    };
    match change {
        Change::Add { title, page, .. } => format!(
            "Added the bookmark \u{201c}{title}\u{201d} on page {}.",
            page + 1
        ),
        Change::Rename {
            bookmark,
            title: new,
        } => format!(
            "Renamed \u{201c}{}\u{201d} to \u{201c}{new}\u{201d}.",
            title(*bookmark)
        ),
        Change::Retarget { bookmark, page } => format!(
            "\u{201c}{}\u{201d} now goes to page {}.",
            title(*bookmark),
            page + 1
        ),
        Change::Move { bookmark, .. } => format!("Moved \u{201c}{}\u{201d}.", title(*bookmark)),
        Change::Remove { bookmark } => format!("Deleted \u{201c}{}\u{201d}.", title(*bookmark)),
        Change::Fold { bookmark, open } => format!(
            "{} \u{201c}{}\u{201d}.",
            if *open { "Opened" } else { "Closed" },
            title(*bookmark)
        ),
    }
}

#[must_use]
pub fn said_headings(headings: &[Heading], replaced: usize) -> String {
    let mut said = format!(
        "Made {} bookmark{} from the headings, as one step undo takes back",
        headings.len(),
        if headings.len() == 1 { "" } else { "s" }
    );
    if replaced > 0 {
        let _ = write!(said, ", after taking out the {replaced} that were there");
    }
    said.push('.');
    for heading in headings.iter().take(40) {
        let _ = write!(
            said,
            "\n{}{} (page {})",
            "  ".repeat(heading.level),
            heading.title,
            heading.page + 1
        );
    }
    if headings.len() > 40 {
        let _ = write!(said, "\n... and {} more", headings.len() - 40);
    }
    said
}

#[cfg(test)]
mod tests;
