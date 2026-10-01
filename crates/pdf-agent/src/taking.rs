use std::path::Path;
use std::sync::Arc;

use pdf_convert::run::Outcome;
use pdf_edit::stamp::{Only, pages_of};
use pdf_session::naming::{named_for_pages, numbered_as};
use pdf_session::pieces;

pub const MOST_FILES: usize = 500;

pub const LEAST_DPI: f64 = 20.0;

pub const MOST_DPI: f64 = 600.0;

pub const DEFAULT_DPI: f64 = 150.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Extract {
    pub pages: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Split {
    Every(usize),
    At(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pictures {
    pub pages: String,
    pub dpi: f64,
}

fn count_of(pages: &[usize]) -> String {
    format!(
        "{} page{}",
        pages.len(),
        if pages.len() == 1 { "" } else { "s" }
    )
}

pub fn pages_named(spec: &str, count: usize) -> Result<Vec<usize>, String> {
    pages_of(spec, count, Only::Every).map_err(|error| {
        let beyond = spec
            .split(|letter: char| !letter.is_ascii_digit())
            .filter_map(|number| number.parse::<usize>().ok())
            .find(|number| *number == 0 || *number > count);
        match beyond {
            Some(0) => "pages count from 1".to_owned(),
            Some(number) => format!("there is no page {number}: the document has {count}"),
            None => error
                .to_string()
                .trim_start_matches("cannot type here: ")
                .to_owned(),
        }
    })
}

pub fn extract_outcome(
    original: &Path,
    bytes: &Arc<[u8]>,
    credential: &[u8],
    pages: &[usize],
) -> Result<Outcome, String> {
    if pages.is_empty() {
        return Err("no page was named: give `pages` like \"1-3, 5\"".to_owned());
    }
    let written = pdf_session::extract_pages(bytes, credential, pages)
        .map_err(|error| format!("the pages could not be taken out: {error}"))?;
    Ok(Outcome {
        files: vec![(named_for_pages(original, pages), written)],
        notes: Vec::new(),
    })
}

impl Split {
    pub fn groups(&self, count: usize) -> Result<Vec<Vec<usize>>, String> {
        let groups = match self {
            Self::Every(size) => pieces::every(count, *size),
            Self::At(spec) => {
                let starts = if spec.trim().is_empty() {
                    Vec::new()
                } else {
                    pages_named(spec, count)?
                };
                pieces::at(count, &starts)
            }
        };
        if groups.len() < 2 {
            return Err(format!(
                "that makes {} file, not several: split every few pages with a smaller `every`, \
                 or name the pages new files start at with `at`",
                groups.len()
            ));
        }
        if groups.len() > MOST_FILES {
            return Err(format!(
                "that makes {} files: the most one call writes is {MOST_FILES}",
                groups.len()
            ));
        }
        Ok(groups)
    }
}

pub fn split_outcome(
    original: &Path,
    (bytes, credential): (&Arc<[u8]>, &[u8]),
    groups: &[Vec<usize>],
) -> Result<Outcome, String> {
    let mut files = Vec::with_capacity(groups.len());
    for (at, pages) in groups.iter().enumerate() {
        let written = pdf_session::extract_pages(bytes, credential, pages)
            .map_err(|error| format!("file {} could not be made: {error}", at + 1))?;
        let name = numbered_as(original, at + 1, groups.len(), "pdf");
        files.push((file_name(&name), written));
    }
    Ok(Outcome {
        files,
        notes: Vec::new(),
    })
}

#[must_use]
pub fn picture_names(original: &Path, pages: &[usize]) -> Vec<String> {
    let stem = original.file_stem().map_or_else(
        || "page".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let width = pages
        .iter()
        .max()
        .map_or(1, |last| (last + 1).to_string().len());
    pages
        .iter()
        .map(|page| format!("{stem}-p{:0width$}.png", page + 1))
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || "page.png".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[must_use]
pub fn said_extracted(pages: &[usize], made: &str) -> String {
    format!(
        "Took out {} (page{} {}) into a new PDF. {made}",
        count_of(pages),
        if pages.len() == 1 { "" } else { "s" },
        page_list(pages)
    )
}

#[must_use]
pub fn page_list(pages: &[usize]) -> String {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &page in pages {
        match runs.last_mut() {
            Some((_, last)) if *last + 1 == page => *last = page,
            _ => runs.push((page, page)),
        }
    }
    runs.iter()
        .map(|(first, last)| {
            if first == last {
                (first + 1).to_string()
            } else {
                format!("{}-{}", first + 1, last + 1)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[must_use]
pub fn said_split(groups: &[Vec<usize>], made: &str) -> String {
    let lines: Vec<String> = groups
        .iter()
        .take(8)
        .enumerate()
        .map(|(at, pages)| {
            format!(
                "file {}: page{} {}",
                at + 1,
                if pages.len() == 1 { "" } else { "s" },
                page_list(pages)
            )
        })
        .collect();
    let more = if groups.len() > 8 {
        format!("; and {} more files", groups.len() - 8)
    } else {
        String::new()
    };
    format!(
        "Split the document into {} files ({}{more}). {made}",
        groups.len(),
        lines.join("; ")
    )
}

#[must_use]
pub fn said_pictures(pages: &[usize], dpi: f64, made: &str) -> String {
    format!(
        "Drew {} (page{} {}) as PNG pictures at {dpi:.0} dpi. {made}",
        count_of(pages),
        if pages.len() == 1 { "" } else { "s" },
        page_list(pages)
    )
}

#[cfg(test)]
mod tests;
