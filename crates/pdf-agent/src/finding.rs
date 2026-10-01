use std::fmt::Write as _;
use std::ops::Range;

use pdf_edit::{BlockRange, BlockReading, Command, LineEnd};
use pdf_session::PageView;

use crate::desk;

pub const MOST_BLOCKS: usize = 500;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Search {
    pub wanted: String,
    pub match_case: bool,
    pub whole_words: bool,
}

impl Search {
    #[must_use]
    pub fn exactly(wanted: &str) -> Self {
        Self {
            wanted: wanted.to_owned(),
            match_case: true,
            whole_words: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Match {
    pub bytes: Range<usize>,
    pub from: (usize, usize),
    pub to: (usize, usize),
}

impl Match {
    #[must_use]
    pub const fn range(&self) -> BlockRange {
        BlockRange::Between {
            from: self.from,
            to: self.to,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Matches {
    pub found: Vec<Match>,
    pub inside_a_character: usize,
}

pub(crate) fn units(reading: &BlockReading) -> Vec<((usize, usize), String)> {
    let mut out = Vec::new();
    for (line, read) in reading.lines.iter().enumerate() {
        for (stop, cluster) in read.clusters.iter().enumerate() {
            out.push(((line, stop), cluster.clone()));
        }
        let last = line + 1 == reading.lines.len();
        match read.end {
            _ if last => {}
            LineEnd::Wrap => {}
            LineEnd::WrapWithSpace => out.push(((line, read.clusters.len()), " ".to_owned())),
            _ => out.push(((line, read.clusters.len()), "\n".to_owned())),
        }
    }
    out
}

pub(crate) fn whole(reading: &BlockReading) -> BlockRange {
    let last = reading.lines.len().saturating_sub(1);
    let stops = reading
        .lines
        .get(last)
        .map_or(0, |line| line.clusters.len());
    BlockRange::Between {
        from: (0, 0),
        to: (last, stops),
    }
}

fn folded(letter: char, match_case: bool) -> char {
    if match_case {
        letter
    } else {
        letter.to_lowercase().next().unwrap_or(letter)
    }
}

fn is_word(letter: char) -> bool {
    letter.is_alphanumeric() || letter == '_'
}

#[must_use]
pub fn matches_in(reading: &BlockReading, search: &Search) -> Matches {
    let units = units(reading);
    let mut text = String::new();
    let mut starts = Vec::with_capacity(units.len() + 1);
    for (_, unit) in &units {
        starts.push(text.len());
        text.push_str(unit);
    }
    starts.push(text.len());
    let letters: Vec<(usize, char)> = text.char_indices().collect();
    let wanted: Vec<char> = search
        .wanted
        .chars()
        .map(|letter| folded(letter, search.match_case))
        .collect();
    let mut matches = Matches::default();
    if wanted.is_empty() || wanted.len() > letters.len() {
        return matches;
    }
    let edge_words = (
        wanted.first().copied().is_some_and(is_word),
        wanted.last().copied().is_some_and(is_word),
    );
    let position = |unit: usize| -> (usize, usize) {
        units.get(unit).map_or_else(
            || {
                let line = reading.lines.len().saturating_sub(1);
                (
                    line,
                    reading
                        .lines
                        .get(line)
                        .map_or(0, |read| read.clusters.len()),
                )
            },
            |(position, _)| *position,
        )
    };
    let mut at = 0;
    while at + wanted.len() <= letters.len() {
        let same = letters[at..at + wanted.len()]
            .iter()
            .zip(&wanted)
            .all(|((_, letter), wanted)| folded(*letter, search.match_case) == *wanted);
        if !same {
            at += 1;
            continue;
        }
        let end = at + wanted.len();
        let before = at.checked_sub(1).map(|before| letters[before].1);
        let after = letters.get(end).map(|(_, letter)| *letter);
        let crowded = search.whole_words
            && ((edge_words.0 && before.is_some_and(is_word))
                || (edge_words.1 && after.is_some_and(is_word)));
        if crowded {
            at += 1;
            continue;
        }
        let from = letters[at].0;
        let to = letters.get(end).map_or(text.len(), |(offset, _)| *offset);
        let first = starts.iter().position(|start| *start == from);
        let last = starts.iter().position(|start| *start == to);
        match (first, last) {
            (Some(first), Some(last)) => matches.found.push(Match {
                bytes: from..to,
                from: position(first),
                to: position(last),
            }),
            _ => matches.inside_a_character += 1,
        }
        at = end;
    }
    matches
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rewriting {
    pub range: BlockRange,
    pub text: String,
    pub count: usize,
}

#[must_use]
pub fn rewriting(text: &str, found: &[Match], with: &str) -> Option<Rewriting> {
    let first = found.first()?;
    let last = found.last()?;
    let with = with.replace("\r\n", "\n");
    let mut rewritten = String::new();
    let mut taken = first.bytes.start;
    for hit in found {
        rewritten.push_str(&text[taken..hit.bytes.start]);
        rewritten.push_str(&with);
        taken = hit.bytes.end;
    }
    Some(Rewriting {
        range: BlockRange::Between {
            from: first.from,
            to: last.to,
        },
        text: rewritten,
        count: found.len(),
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageWork {
    pub page: usize,
    pub found: usize,
    pub commands: Vec<Command>,
    pub left: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Replaced {
    pub commands: Vec<Command>,
    pub per_page: Vec<(usize, usize)>,
    pub left: Vec<String>,
    pub reached: Option<usize>,
}

impl Replaced {
    #[must_use]
    pub fn of(works: Vec<PageWork>) -> Self {
        let mut replaced = Self {
            reached: works.last().map(|work| work.page),
            ..Self::default()
        };
        for work in works {
            if work.found > 0 {
                replaced.per_page.push((work.page, work.found));
            }
            replaced.commands.extend(work.commands);
            replaced.left.extend(work.left);
        }
        replaced
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.per_page.iter().map(|(_, count)| count).sum()
    }
}

pub type Check<'a> = dyn FnMut(&Command) -> Result<(), String> + 'a;

fn place_in_stream(command: &Command) -> (u32, usize) {
    let Command::RewriteBlock { rows, .. } = command else {
        return (0, 0);
    };
    rows.iter()
        .flatten()
        .map(|cluster| {
            (
                cluster.anchor.stream.object_number(),
                cluster.anchor.operator_offset,
            )
        })
        .min()
        .unwrap_or((0, 0))
}

fn by_page(per_page: &[(usize, usize)]) -> String {
    per_page
        .iter()
        .take(30)
        .map(|(page, count)| format!("page {}: {count}", page + 1))
        .collect::<Vec<_>>()
        .join(", ")
}

fn where_searched((first, last): (usize, usize), pages: usize) -> String {
    if first == 0 && last + 1 >= pages {
        String::new()
    } else {
        format!(" in pages {} to {}", first + 1, last + 1)
    }
}

fn what_was_not_reached(reached: Option<usize>, last: usize, what: &str) -> String {
    match reached {
        Some(reached) if reached < last => format!(
            "\nThat is as many {what} as one call changes, so pages {} to {} were not searched: \
             call it again with first_page: {}.",
            reached + 2,
            last + 1,
            reached + 2
        ),
        _ => String::new(),
    }
}

fn what_was_left(left: &[String]) -> String {
    if left.is_empty() {
        return String::new();
    }
    let mut said = String::from("\nLeft alone:");
    for why in left.iter().take(10) {
        said.push_str("\n- ");
        said.push_str(why);
    }
    if left.len() > 10 {
        let _ = write!(said, "\n- and {} more", left.len() - 10);
    }
    said
}

#[must_use]
pub fn said_replaced(
    (search, with): (&Search, &str),
    replaced: &Replaced,
    (span, pages): ((usize, usize), usize),
) -> String {
    let total = replaced.total();
    let left = what_was_left(&replaced.left);
    let quoted = format!("\u{201c}{}\u{201d}", search.wanted);
    let around = where_searched(span, pages);
    let more = what_was_not_reached(replaced.reached, span.1, "blocks");
    if total == 0 {
        return format!("Nothing was replaced: {quoted} was not found{around}.{left}{more}");
    }
    format!(
        "Replaced {total} match{} of {quoted} with \u{201c}{with}\u{201d} on {} page{} ({}), as one \
         step undo takes back.{left}{more}",
        if total == 1 { "" } else { "es" },
        replaced.per_page.len(),
        if replaced.per_page.len() == 1 {
            ""
        } else {
            "s"
        },
        by_page(&replaced.per_page),
    )
}

#[must_use]
pub fn said_marked(
    (search, done): (&Search, &str),
    marked: &Replaced,
    (span, pages): ((usize, usize), usize),
) -> String {
    let total = marked.total();
    let left = what_was_left(&marked.left);
    let quoted = format!("\u{201c}{}\u{201d}", search.wanted);
    let around = where_searched(span, pages);
    let more = what_was_not_reached(marked.reached, span.1, "marks");
    if total == 0 {
        return format!("Nothing was marked: {quoted} was not found{around}.{left}{more}");
    }
    format!(
        "Marked {total} place{} of {quoted} ({done}) on {} page{} ({}), as one step undo takes \
         back.{left}{more}",
        if total == 1 { "" } else { "s" },
        marked.per_page.len(),
        if marked.per_page.len() == 1 { "" } else { "s" },
        by_page(&marked.per_page),
    )
}

#[must_use]
pub fn replacements_on(
    view: &PageView,
    page: usize,
    (search, with): (&Search, &str),
    check: &mut Check<'_>,
) -> PageWork {
    let mut work = PageWork {
        page,
        ..PageWork::default()
    };
    for index in 0..view.index.blocks.len() {
        let Some((block, parts)) = desk::read(view, page, index) else {
            continue;
        };
        let matches = matches_in(&parts.reading, search);
        if matches.found.is_empty() && matches.inside_a_character == 0 {
            continue;
        }
        if let Some(why) = block.fixed {
            work.left.push(format!(
                "{} holds the words but cannot be changed: {why}",
                block.name()
            ));
            continue;
        }
        if matches.inside_a_character > 0 {
            work.left.push(format!(
                "{} has {} match{} that start or end inside one character, left alone",
                block.name(),
                matches.inside_a_character,
                if matches.inside_a_character == 1 {
                    ""
                } else {
                    "es"
                }
            ));
        }
        let Some(rewriting) = rewriting(&block.text, &matches.found, with) else {
            continue;
        };
        let desk::Parts { rows, frame, .. } = parts;
        let command = Command::RewriteBlock {
            page_index: page,
            rows,
            frame,
            edges: (0, 0),
            breaks: None,
            frame_declared: false,
            paragraph: pdf_edit::ParagraphLayout::default(),
            range: rewriting.range,
            text: rewriting.text,
        };
        match check(&command) {
            Ok(()) => {
                work.found += rewriting.count;
                work.commands.push(command);
            }
            Err(why) => work
                .left
                .push(format!("{} could not be changed: {why}", block.name())),
        }
    }
    work.commands
        .sort_by_key(|command| std::cmp::Reverse(place_in_stream(command)));
    work
}

#[cfg(test)]
mod tests;
