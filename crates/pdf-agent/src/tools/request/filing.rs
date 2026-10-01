use std::path::PathBuf;

use crate::json::Json;
use crate::recognizing::{Asked as OcrAsked, is_a_language_code};
use crate::taking::{DEFAULT_DPI, Extract, LEAST_DPI, MOST_DPI, Pictures, Split};
use crate::tools::{Args, expand};

use super::Request;
use super::editing::{optional_flag, optional_number};

pub(super) fn convert(args: &Args) -> Result<Request, String> {
    crate::converting::asked_for(args).map(Request::Convert)
}

pub(super) fn protect(args: &Args) -> Result<Request, String> {
    crate::converting::protection_for(args).map(Request::Convert)
}

fn words(args: &Args, key: &str) -> Result<Vec<String>, String> {
    if !args.has(key) {
        return Ok(Vec::new());
    }
    let wrong = || format!("`{key}` is a list of words");
    match args.0.get(key) {
        Some(Json::List(items)) => items
            .iter()
            .map(|item| item.as_str().map(|word| word.trim().to_owned()))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(wrong),
        Some(Json::Text(text)) => Ok(text
            .split(['+', ','])
            .map(|word| word.trim().to_owned())
            .filter(|word| !word.is_empty())
            .collect()),
        _ => Err(wrong()),
    }
}

pub(super) fn ocr_pages(args: &Args) -> Result<Request, String> {
    let languages = words(args, "languages")?;
    if let Some(bad) = languages.iter().find(|code| !is_a_language_code(code)) {
        return Err(format!(
            "{bad:?} is not a language code: they look like eng, tha, lao or chi_sim"
        ));
    }
    Ok(Request::OcrPages(OcrAsked {
        pages: args.text("pages").unwrap_or_default().trim().to_owned(),
        languages,
        skip_text: optional_flag(args, "skip_pages_with_text")?.unwrap_or(true),
    }))
}

pub(super) fn extract_pages(args: &Args) -> Result<Request, String> {
    let pages = args
        .required("pages")
        .map_err(|_| "`pages` is needed, like \"1-3, 5\"".to_owned())?
        .trim()
        .to_owned();
    if pages.is_empty() || pages.eq_ignore_ascii_case("all") {
        return Err(
            "`pages` names the pages to take out, like \"1-3, 5\": all of them is the whole \
             document, which save_copy writes"
                .to_owned(),
        );
    }
    Ok(Request::ExtractPages(Extract { pages }))
}

pub(super) fn split_document(args: &Args) -> Result<Request, String> {
    match (args.has("every"), args.has("at")) {
        (true, true) => Err("give `every` or `at`, not both".to_owned()),
        (false, false) => Err(
            "`every` (a file for each so many pages) or `at` (the pages new files start at, \
             like \"5, 12\") is needed"
                .to_owned(),
        ),
        (true, false) => {
            let size = args
                .0
                .get("every")
                .and_then(Json::as_count)
                .filter(|size| *size >= 1)
                .ok_or("`every` is a whole number of pages from 1")?;
            Ok(Request::SplitDocument(Split::Every(size)))
        }
        (false, true) => {
            let at = args
                .required("at")
                .map_err(|_| "`at` is page numbers, like \"5, 12\"".to_owned())?
                .trim()
                .to_owned();
            if at.is_empty() {
                return Err("`at` is empty: name the pages new files start at".to_owned());
            }
            Ok(Request::SplitDocument(Split::At(at)))
        }
    }
}

pub(super) fn export_page_pictures(args: &Args) -> Result<Request, String> {
    let dpi = optional_number(args, "dpi", (LEAST_DPI, MOST_DPI), "")?.unwrap_or(DEFAULT_DPI);
    Ok(Request::ExportPictures(Pictures {
        pages: args.text("pages").unwrap_or_default().trim().to_owned(),
        dpi,
    }))
}

pub(super) fn save_copy(args: &Args) -> Result<Request, String> {
    let path: Option<PathBuf> = match args.text("path") {
        None => None,
        Some(path) if path.trim().is_empty() => return Err("`path` is empty".to_owned()),
        Some(path) => {
            let path = expand(path.trim());
            if !path.is_absolute() {
                return Err(format!(
                    "{} is not a full path: give one that starts at the root, or with ~/",
                    path.display()
                ));
            }
            Some(path)
        }
    };
    Ok(Request::SaveCopy { path })
}
