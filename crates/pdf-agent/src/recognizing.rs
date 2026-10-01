use std::fmt::Write as _;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Asked {
    pub pages: String,
    pub languages: Vec<String>,
    pub skip_text: bool,
}

#[must_use]
pub fn is_a_language_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 16
        && code
            .chars()
            .all(|letter| letter.is_ascii_alphanumeric() || letter == '_')
}

#[must_use]
pub fn said_no_engine(installing: &str) -> String {
    format!(
        "No pages were read: the text recogniser is not installed on this computer. {installing} \
         The person can also let the Recognize text panel in the Tools menu download it. Nothing \
         in the document changed."
    )
}

#[must_use]
pub fn said_no_language(missing: &[String], here: &[String]) -> String {
    let have = if here.is_empty() {
        "none is installed yet".to_owned()
    } else {
        format!("these are installed: {}", here.join(", "))
    };
    format!(
        "The recogniser has no model for {}; {have}. The person can download a language in the \
         Recognize text panel of the Tools menu. Nothing in the document changed.",
        missing.join(", ")
    )
}

#[must_use]
pub fn said_not_in_one_place(apart: &[String]) -> String {
    format!(
        "The recogniser can only use languages that are all installed in one place, and {} is \
         not with the others. Ask for fewer languages, or let the person download them in the \
         Recognize text panel. Nothing in the document changed.",
        apart.join(", ")
    )
}

#[must_use]
pub fn said_nothing_to_read(had_text: usize, unread: usize, why: Option<&str>) -> String {
    match why {
        Some(why) => format!(
            "No page could be read: {why}. Nothing in the document changed ({unread} page{} failed).",
            if unread == 1 { "" } else { "s" }
        ),
        None if had_text > 0 => format!(
            "Every page asked for already has text ({had_text} page{}), so nothing was read and \
             nothing changed. Ask with skip_pages_with_text: false to read them anyway.",
            if had_text == 1 { "" } else { "s" }
        ),
        None => "The recogniser found no words on those pages, so nothing was written.".to_owned(),
    }
}

#[must_use]
pub fn said_read(
    pages: &[usize],
    (confidence, had_text, unread): (u8, usize, usize),
    languages: &[String],
) -> String {
    let numbers: Vec<String> = pages.iter().map(|page| (page + 1).to_string()).collect();
    let shown = if numbers.len() > 12 {
        format!("{} pages", numbers.len())
    } else {
        format!(
            "page{} {}",
            if numbers.len() == 1 { "" } else { "s" },
            numbers.join(", ")
        )
    };
    let mut said = format!(
        "Made {shown} searchable in {} (the recogniser was {confidence} % sure), as one step \
         undo takes back. read_text and find_text can read them now.",
        languages.join("+")
    );
    if had_text > 0 {
        let _ = write!(
            said,
            " {had_text} page{} already had text and {} left alone.",
            if had_text == 1 { "" } else { "s" },
            if had_text == 1 { "was" } else { "were" }
        );
    }
    if unread > 0 {
        let _ = write!(
            said,
            " {unread} page{} could not be read.",
            if unread == 1 { "" } else { "s" }
        );
    }
    said
}

#[cfg(test)]
mod tests;
