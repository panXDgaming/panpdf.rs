#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    ChatOnly,
    #[default]
    AskBeforeChanges,
    DoIt,
    Free,
}

impl Mode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChatOnly => "chat_only",
            Self::AskBeforeChanges => "ask_before_changes",
            Self::DoIt => "do_it",
            Self::Free => "free",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "chat_only" => Some(Self::ChatOnly),
            "ask_before_changes" => Some(Self::AskBeforeChanges),
            "do_it" => Some(Self::DoIt),
            "free" => Some(Self::Free),
            _ => None,
        }
    }

    #[must_use]
    pub const fn kept_for_next_time(self) -> Self {
        match self {
            Self::Free => Self::DoIt,
            other => other,
        }
    }
}

pub const ALWAYS_ASK: [&str; 3] = ["insert_pages", "protect_document", "save_copy"];

pub const ASK_EVEN_IN_FULL_ACCESS: [&str; 2] = ["protect_document", "save_copy"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Why {
    ToolsAreOff,
    UnknownTool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Run,
    Ask { may_allow_for_chat: bool },
    Refuse(Why),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Answer {
    Once,
    ForThisChat,
    AllOfThem,
    Refuse,
}

#[must_use]
pub fn may_allow_all(name: &str) -> bool {
    !ALWAYS_ASK.contains(&name)
}

#[must_use]
pub const fn refusal_text() -> &'static str {
    "The person refused this action. Do not retry it; ask what they would like instead."
}

#[must_use]
pub fn decide(
    mode: Mode,
    name: &str,
    facts: Option<(bool, bool)>,
    allowed_for_chat: bool,
) -> Decision {
    let Some((read_only, destructive)) = facts else {
        return Decision::Refuse(Why::UnknownTool);
    };
    if mode == Mode::ChatOnly {
        return Decision::Refuse(Why::ToolsAreOff);
    }
    if read_only {
        return Decision::Run;
    }
    let always = ALWAYS_ASK.contains(&name);
    let careful = always || destructive;
    match mode {
        Mode::ChatOnly => Decision::Refuse(Why::ToolsAreOff),
        Mode::AskBeforeChanges => {
            if allowed_for_chat && !careful {
                Decision::Run
            } else {
                Decision::Ask {
                    may_allow_for_chat: !careful,
                }
            }
        }
        Mode::DoIt => {
            if always {
                Decision::Ask {
                    may_allow_for_chat: false,
                }
            } else {
                Decision::Run
            }
        }
        Mode::Free => {
            if ASK_EVEN_IN_FULL_ACCESS.contains(&name) {
                Decision::Ask {
                    may_allow_for_chat: false,
                }
            } else {
                Decision::Run
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use words::{Shown, WrittenTo, describe_call, describe_change};

#[cfg(not(target_arch = "wasm32"))]
mod words {
    use pdf_agent::tools::request::Request;

    use crate::wording::{Assistant, Lang};

    const MOST: usize = 200;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum WrittenTo {
        Folder(String),
        File(String),
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct Shown {
        pub headline: String,
        pub before: Option<String>,
        pub after: Option<String>,
        pub written_to: Option<WrittenTo>,
    }

    fn page_of_block(name: &str) -> Option<usize> {
        let rest = name.trim().strip_prefix('p')?;
        let (page, block) = rest.split_once("-b")?;
        block.parse::<usize>().ok()?;
        page.parse().ok().filter(|page| *page > 0)
    }

    #[must_use]
    pub fn describe_change(request: &Request, was: Option<&str>, lang: Lang) -> Shown {
        let plain = || Shown {
            headline: describe_call(request, lang),
            before: None,
            after: None,
            written_to: None,
        };
        match request {
            Request::ReplaceText { block, find, text } => {
                let Some(page) = page_of_block(block) else {
                    return plain();
                };
                Shown {
                    headline: Assistant::ReplaceTextOn(page).say(lang),
                    before: find.clone().or_else(|| was.map(str::to_owned)),
                    after: Some(text.clone()),
                    written_to: None,
                }
            }
            Request::AddText { page, text, .. } => Shown {
                headline: Assistant::AddTextOn(page + 1).say(lang),
                before: None,
                after: Some(text.clone()),
                written_to: None,
            },
            Request::WritePages { markdown, .. } => Shown {
                after: Some(markdown.clone()),
                ..plain()
            },
            Request::FindAndReplace { search, with, .. } => Shown {
                before: Some(search.wanted.clone()),
                after: Some(with.clone()),
                ..plain()
            },
            Request::AddStamp(asked) => Shown {
                after: asked.wording.clone(),
                ..plain()
            },
            _ => plain(),
        }
    }

    #[must_use]
    pub fn describe_call(request: &Request, lang: Lang) -> String {
        match lang {
            Lang::English => describe_in_english(request),
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one sentence per tool, read as a table"
    )]
    fn describe_in_english(request: &Request) -> String {
        match request {
            Request::DocumentInfo => "Read what the document says about itself".to_owned(),
            Request::ReadText { first, last } => match (first, last) {
                (None, None) => "Read the document's text".to_owned(),
                (first, last) => {
                    let from = first.unwrap_or(0) + 1;
                    match last {
                        Some(last) if *last + 1 != from => {
                            format!("Read the text of pages {from} to {}", last + 1)
                        }
                        Some(_) => {
                            format!("Read the text of page {from}")
                        }
                        None => {
                            format!("Read the text from page {from} on")
                        }
                    }
                }
            },
            Request::FindText { text, .. } => {
                let text = clip(text);
                format!("Find \u{201c}{text}\u{201d} in the document")
            }
            Request::RenderPage { page, .. } => {
                format!("Look at page {} as a picture", page + 1)
            }
            Request::ListFonts { .. } => "List the fonts new text can be set in".to_owned(),
            Request::ReplaceText { block, find, text } if text.trim().is_empty() => match find {
                Some(find) => {
                    let find = clip(find);
                    format!("Delete \u{201c}{find}\u{201d} from block {block}")
                }
                None => format!("Delete all the text of block {block}"),
            },
            Request::ReplaceText { block, find, text } => {
                let text = clip(text);
                match find {
                    Some(find) => {
                        let find = clip(find);
                        format!("Replace \u{201c}{find}\u{201d} in block {block} with: {text}")
                    }
                    None => {
                        format!("Replace the text of block {block} with: {text}")
                    }
                }
            }
            Request::AddText { page, text, .. } if text.trim().is_empty() => {
                format!("Write nothing at all on page {}", page + 1)
            }
            Request::AddText { page, text, .. } => {
                let text = clip(text);
                format!("Write new text on page {}: {text}", page + 1)
            }
            Request::WritePages {
                from_page,
                markdown,
                replace,
                ..
            } => {
                let words = markdown.split_whitespace().count();
                let doing = if *replace {
                    "Write a document over"
                } else {
                    "Write a document onto"
                };
                let covers = if *replace {
                    "; what is there is covered, not removed"
                } else {
                    ""
                };
                let first = clip(
                    markdown
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or_default(),
                );
                format!(
                    "{doing} the pages from page {}, {words} words, starting \u{201c}{first}\u{201d}{covers}",
                    from_page + 1
                )
            }
            Request::SetProperties(edit) => {
                let named = properties(edit);
                format!("Set the document's properties ({named})")
            }
            Request::FillField { name, value } => {
                let said = clip(&match value {
                    pdf_agent::json::Json::Text(text) => text.clone(),
                    other => other.write(),
                });
                format!("Fill the form field \u{201c}{name}\u{201d} with: {said}")
            }
            Request::AddBlankPage { after, .. } => {
                if *after == 0 {
                    "Add a blank page at the front".to_owned()
                } else {
                    format!("Add a blank page after page {after}")
                }
            }
            Request::DeletePages(pages) => {
                let said = pages_said(pages);
                format!("Delete page{} {said}", plural(pages.len()))
            }
            Request::MovePages { pages, to } => {
                let said = pages_said(pages);
                format!(
                    "Move page{} {said} so that the first becomes page {}",
                    plural(pages.len()),
                    to + 1
                )
            }
            Request::RotatePages {
                pages,
                quarter_turns,
            } => {
                let said = pages_said(pages);
                let degrees = quarter_turns * 90;
                format!(
                    "Turn page{} {said} by {degrees} degrees",
                    plural(pages.len())
                )
            }
            Request::InsertPages {
                from, pages, after, ..
            } => {
                let file = from.display();
                let which = match pages {
                    Some(pages) => {
                        let said = pages_said(pages);
                        format!("page{} {said} of", plural(pages.len()))
                    }
                    None => "every page of".to_owned(),
                };
                let place = if *after == 0 {
                    "at the front".to_owned()
                } else {
                    format!("after page {after}")
                };
                format!("Put {which} the file {file} in {place}")
            }
            Request::Undo => "Take back the last change".to_owned(),
            Request::Redo => "Put back the last change that was taken back".to_owned(),
            Request::AskPerson { question, .. } => format!("Ask you: {question}"),
            Request::UpdatePlan { steps } => {
                format!("Show you its plan, {} steps", steps.len())
            }
            Request::FindAndReplace {
                search,
                with,
                first,
                last,
            } => {
                let find = clip(&search.wanted);
                let with = clip(with);
                let how = match (search.match_case, search.whole_words) {
                    (true, true) => ", matching capitals, whole words only",
                    (true, false) => ", matching capitals",
                    (false, true) => ", whole words only",
                    (false, false) => "",
                };
                let range = pages_range(*first, *last);
                if with.is_empty() {
                    format!("Delete every \u{201c}{find}\u{201d}{range}{how}")
                } else {
                    format!(
                        "Replace every \u{201c}{find}\u{201d} with \u{201c}{with}\u{201d}{range}{how}"
                    )
                }
            }
            Request::StyleText { block, find, look } => {
                let words = look.words();
                match find {
                    Some(find) => format!(
                        "Change how \u{201c}{}\u{201d} in block {block} looks: {words}",
                        clip(find)
                    ),
                    None => format!("Change how block {block} looks: {words}"),
                }
            }
            Request::MarkText {
                search,
                marking,
                first,
                last,
            } => {
                let doing = match marking.how {
                    pdf_agent::marking::How::Highlight => "Highlight",
                    pdf_agent::marking::How::Underline => "Underline",
                    pdf_agent::marking::How::StrikeThrough => "Strike through",
                };
                format!(
                    "{doing} every \u{201c}{}\u{201d}{}",
                    clip(&search.wanted),
                    pages_range(*first, *last)
                )
            }
            Request::AddStamp(asked) => stamp_sentence(asked),
            Request::Bookmarks(action) => bookmark_sentence(action),
            Request::PlacePicture(asked) => {
                let from = match &asked.source {
                    pdf_agent::pictures::Source::Attachment(None) => {
                        "the picture attached to this chat".to_owned()
                    }
                    pdf_agent::pictures::Source::Attachment(Some(number)) => {
                        format!("attached picture {number}")
                    }
                    pdf_agent::pictures::Source::File(path) => {
                        format!("the file {}", path.display())
                    }
                };
                format!(
                    "Put {from} on page {} at left {:.0}, top {:.0}",
                    asked.page + 1,
                    asked.left,
                    asked.top
                )
            }
            Request::Objects(action) => object_sentence(action),
            Request::GoToPage { page } => format!("Show page {} in the window", page + 1),
            Request::LookCloser { page, region, .. } => format!(
                "Look closer at the part [{:.0}, {:.0}, {:.0}, {:.0}] of page {}",
                region[0],
                region[1],
                region[2],
                region[3],
                page + 1
            ),
            Request::Convert(asked) => convert_sentence(asked),
            Request::OcrPages(asked) => ocr_sentence(asked),
            Request::ExtractPages(asked) => format!(
                "Save pages {} of the document as a new PDF file beside it",
                clip(asked.pages.trim())
            ),
            Request::SplitDocument(split) => match split {
                pdf_agent::taking::Split::Every(size) => format!(
                    "Split the document into new PDF files of {size} page{} each, in a new \
                     folder beside it",
                    plural(*size)
                ),
                pdf_agent::taking::Split::At(spec) => format!(
                    "Split the document into new PDF files starting at pages {}, in a new \
                     folder beside it",
                    clip(spec.trim())
                ),
            },
            Request::ExportPictures(asked) => {
                let which = if asked.pages.trim().is_empty() {
                    "every page".to_owned()
                } else {
                    format!("pages {}", clip(asked.pages.trim()))
                };
                format!(
                    "Save {which} as PNG pictures at {:.0} dpi, as new files beside the document",
                    asked.dpi
                )
            }
            Request::SaveCopy { path: Some(path) } => format!(
                "Save a copy of the document, with its changes, as the new file {}",
                path.display()
            ),
            Request::SaveCopy { path: None } => {
                "Save a copy of the document, with its changes, beside the original under a name \
                 no file has yet"
                    .to_owned()
            }
            Request::Links(action) => link_sentence(action),
            Request::DrawShape(asked) => format!(
                "Draw {} on page {}",
                asked.shape.with_article(),
                asked.page + 1
            ),
            Request::AddField(asked) => {
                let named = asked.name.as_ref().map_or_else(String::new, |name| {
                    format!(" named \u{201c}{}\u{201d}", clip(name))
                });
                format!(
                    "Add a {} form field{named} to page {}",
                    pdf_agent::fielding::kind_name(asked.kind),
                    asked.page + 1
                )
            }
            Request::SetTabOrder { page, order } => format!(
                "Put the form fields of page {} in tab order by {}",
                page + 1,
                order.as_str()
            ),
        }
    }

    fn pages_range(first: Option<usize>, last: Option<usize>) -> String {
        match (first, last) {
            (None, None) => " in the whole document".to_owned(),
            (Some(first), Some(last)) if first == last => format!(" on page {}", first + 1),
            (first, last) => format!(
                " on pages {} to {}",
                first.unwrap_or(0) + 1,
                last.map_or_else(|| "the last".to_owned(), |last| (last + 1).to_string())
            ),
        }
    }

    fn stamp_sentence(asked: &pdf_agent::stamping::Asked) -> String {
        use pdf_agent::stamping::{Kind, spot_as_str};
        let what = match asked.kind {
            Kind::PageNumbers => "page numbers",
            Kind::HeaderFooter => "a header or footer",
            Kind::Watermark => "a watermark",
        };
        let place = asked.spot.map_or_else(String::new, |spot| {
            format!(" at {}", spot_as_str(spot).replace('_', " "))
        });
        let which = if asked.pages.trim().is_empty() {
            "every page".to_owned()
        } else {
            format!("pages {}", asked.pages.trim())
        };
        let only = match asked.only {
            pdf_edit::stamp::Only::Every => "",
            pdf_edit::stamp::Only::Odd => ", the odd ones only",
            pdf_edit::stamp::Only::Even => ", the even ones only",
        };
        let wording = asked
            .wording
            .as_deref()
            .map_or_else(String::new, |wording| {
                format!(": \u{201c}{}\u{201d}", clip(wording))
            });
        format!("Put {what}{place} on {which}{only}{wording}")
    }

    fn bookmark_sentence(action: &pdf_agent::outlining::Action) -> String {
        use pdf_agent::outlining::{Action, Place};
        match action {
            Action::List => "List the bookmarks".to_owned(),
            Action::Add { title, place, .. } => {
                let title = title.as_deref().map_or_else(String::new, |title| {
                    format!(" \u{201c}{}\u{201d}", clip(title))
                });
                match place {
                    Place::Page(page) => format!("Add the bookmark{title} for page {}", page + 1),
                    Place::Block(block) => format!("Add the bookmark{title} for block {block}"),
                }
            }
            Action::Rename { bookmark, title } => {
                format!(
                    "Rename bookmark {bookmark} to \u{201c}{}\u{201d}",
                    clip(title)
                )
            }
            Action::Retarget { bookmark, page } => {
                format!("Make bookmark {bookmark} go to page {}", page + 1)
            }
            Action::Move { bookmark, step } => {
                format!("Move bookmark {bookmark} {}", step.as_str())
            }
            Action::Delete { bookmark } => format!("Delete bookmark {bookmark}"),
            Action::FromHeadings { replace: false } => {
                "Make a table of contents from the headings".to_owned()
            }
            Action::FromHeadings { replace: true } => {
                "Make a table of contents from the headings, taking out the bookmarks there are"
                    .to_owned()
            }
        }
    }

    fn object_sentence(action: &pdf_agent::objects::Action) -> String {
        use pdf_agent::objects::Action;
        match action {
            Action::List { page } => format!(
                "List the pictures, drawings and text blocks of page {}",
                page + 1
            ),
            Action::Move { object, left, top } => {
                let at = match (left, top) {
                    (Some(left), Some(top)) => format!("left {left:.0}, top {top:.0}"),
                    (Some(left), None) => format!("left {left:.0}"),
                    (None, Some(top)) => format!("top {top:.0}"),
                    (None, None) => "a new place".to_owned(),
                };
                format!("Move {object} to {at}")
            }
            Action::Resize {
                object,
                width,
                height,
            } => {
                let size = match (width, height) {
                    (Some(width), Some(height)) => format!("{width:.0} x {height:.0} pt"),
                    (Some(width), None) => format!("{width:.0} pt wide"),
                    (None, Some(height)) => format!("{height:.0} pt tall"),
                    (None, None) => "a new size".to_owned(),
                };
                format!("Resize {object} to {size}")
            }
            Action::Delete { object } => format!("Delete {object}"),
        }
    }

    fn source_words(files: &[std::path::PathBuf]) -> String {
        let names: Vec<String> = files
            .iter()
            .take(3)
            .map(|path| {
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
            })
            .collect();
        match files.len() {
            0 => "the open document".to_owned(),
            1..=3 => names.join(", "),
            many => format!("{} and {} more", names.join(", "), many - 3),
        }
    }

    fn convert_sentence(asked: &pdf_agent::converting::Asked) -> String {
        use pdf_convert::{Setting, Tool, Value};
        let from = source_words(&asked.files);
        let made = match asked.tool {
            Tool::PdfToWord => format!("Turn {from} into a Word file"),
            Tool::PdfToExcel => format!("Turn {from} into an Excel file"),
            Tool::PdfToPowerPoint => format!("Turn {from} into a PowerPoint file"),
            Tool::PdfToImage => format!("Save the pages or pictures of {from} as picture files"),
            Tool::PdfToHtml => format!("Turn {from} into a web page"),
            Tool::PdfToMarkdown => format!("Turn {from} into a Markdown file"),
            Tool::PdfToText => format!("Turn {from} into a plain text file"),
            Tool::PdfToPdfA => format!("Turn {from} into a PDF/A file, the form kept for archives"),
            Tool::WordToPdf
            | Tool::ExcelToPdf
            | Tool::PowerPointToPdf
            | Tool::ImageToPdf
            | Tool::ScanToPdf
            | Tool::HtmlToPdf => format!("Make a PDF from {from}"),
            Tool::Compress => format!("Compress a copy of {from}"),
            Tool::Repair => format!("Repair a copy of {from}"),
            Tool::Ocr => format!("Make a searchable copy of {from}"),
            Tool::Unlock => format!("Take the password off a copy of {from}"),
            Tool::Sign => {
                let name = match asked.values.explicit(Setting::TypedName) {
                    Some(Value::Text(name)) => {
                        format!(" with the name \u{201c}{}\u{201d}", clip(name))
                    }
                    _ => " with a picture".to_owned(),
                };
                format!("Sign a copy of {from}{name}")
            }
            Tool::Redact => {
                let words = match asked.values.explicit(Setting::Search) {
                    Some(Value::Terms(terms)) if !terms.is_empty() => {
                        format!(" \u{201c}{}\u{201d}", clip(&terms.join(", ")))
                    }
                    _ => " what the file's own redaction marks cover".to_owned(),
                };
                format!(
                    "Remove{words} from a copy of {from} for good: the words cannot be read \
                     back from the new file"
                )
            }
            Tool::Compare => {
                let (one, other) = match asked.files.as_slice() {
                    [one, other] => (
                        source_words(std::slice::from_ref(one)),
                        source_words(std::slice::from_ref(other)),
                    ),
                    [other] => (
                        "the open document".to_owned(),
                        source_words(std::slice::from_ref(other)),
                    ),
                    _ => ("a file".to_owned(), "another file".to_owned()),
                };
                format!("Compare {one} with {other}")
            }
            Tool::Protect => {
                let deny = match asked.values.explicit(Setting::Forbid) {
                    Some(Value::Choices(choices)) if !choices.is_empty() => format!(
                        ", forbidding {}",
                        choices
                            .iter()
                            .map(|choice| choice.value())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    _ => String::new(),
                };
                format!("Protect a copy of {from} with a password{deny}")
            }
        };
        format!("{made}, writing a new file beside it")
    }

    fn ocr_sentence(asked: &pdf_agent::recognizing::Asked) -> String {
        let pages = if asked.pages.trim().is_empty() {
            "every page".to_owned()
        } else {
            format!("pages {}", clip(asked.pages.trim()))
        };
        let languages = if asked.languages.is_empty() {
            String::new()
        } else {
            format!(" in {}", asked.languages.join(" and "))
        };
        format!(
            "Read the words of {pages}{languages} with the text recogniser, so they can be searched"
        )
    }

    fn link_sentence(action: &pdf_agent::linking::Action) -> String {
        use pdf_agent::linking::{Action, Goes, Place};
        match action {
            Action::List { page } => format!("List the links of page {}", page + 1),
            Action::Add { page, place, goes } => {
                let over = match place {
                    Place::Block(name) => format!("block {name}"),
                    Place::Area([left, top, right, bottom]) => {
                        format!("the box [{left:.0}, {top:.0}, {right:.0}, {bottom:.0}]")
                    }
                };
                let to = match goes {
                    Goes::Address(address) => clip(address),
                    Goes::Page(to) => format!("page {}", to + 1),
                };
                format!(
                    "Add a link over {over} on page {} that goes to {to}",
                    page + 1
                )
            }
            Action::Remove { link } => format!("Delete link {link}"),
        }
    }

    fn properties(edit: &pdf_edit::info::InfoEdit) -> String {
        let named: Vec<&str> = [
            (edit.title.is_some(), "title"),
            (edit.author.is_some(), "author"),
            (edit.subject.is_some(), "subject"),
            (edit.keywords.is_some(), "keywords"),
        ]
        .into_iter()
        .filter_map(|(asked, word)| asked.then_some(word))
        .collect();
        named.join(", ")
    }

    fn pages_said(pages: &[usize]) -> String {
        let numbers: Vec<String> = pages.iter().map(|page| (page + 1).to_string()).collect();
        let and = " and ";
        match numbers.split_last() {
            None => String::new(),
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{}{and}{last}", rest.join(", ")),
        }
    }

    fn plural(count: usize) -> &'static str {
        if count == 1 { "" } else { "s" }
    }

    fn clip(text: &str) -> String {
        let flat = text.replace(['\n', '\r'], " ");
        if flat.chars().count() <= MOST {
            flat
        } else {
            let mut clipped: String = flat.chars().take(MOST).collect();
            clipped.push('\u{2026}');
            clipped
        }
    }
}

#[cfg(test)]
mod tests;
