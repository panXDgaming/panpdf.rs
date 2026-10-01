use pdf_agent::tools::request::Request;

use crate::wording::{Assistant, Lang};

#[must_use]
pub fn doing(request: &Request, lang: Lang) -> String {
    match lang {
        Lang::English => doing_in_english(request),
    }
}

#[must_use]
pub fn did(name: &str, lang: Lang) -> String {
    match lang {
        Lang::English => did_in_english(name),
    }
}

#[must_use]
pub fn target(request: &Request) -> Option<Assistant> {
    match request {
        Request::DocumentInfo
        | Request::ListFonts { .. }
        | Request::AddBlankPage { .. }
        | Request::Undo
        | Request::Redo
        | Request::AskPerson { .. }
        | Request::UpdatePlan { .. } => None,
        Request::ReadText { first, last } => match (first, last) {
            (None, None) => Some(Assistant::TargetDocument),
            (Some(first), Some(last)) if first == last => Some(Assistant::TargetPage(first + 1)),
            (Some(first), Some(last)) => Some(Assistant::TargetPages {
                first: first + 1,
                last: last + 1,
            }),
            (Some(first), None) => Some(Assistant::TargetPage(first + 1)),
            (None, Some(last)) => Some(Assistant::TargetPages {
                first: 1,
                last: last + 1,
            }),
        },
        Request::FindText { text, .. } => Some(Assistant::TargetQuery(short(text))),
        Request::RenderPage { page, .. }
        | Request::AddText { page, .. }
        | Request::WritePages {
            from_page: page, ..
        }
        | Request::SetTabOrder { page, .. } => Some(Assistant::TargetPage(page + 1)),
        Request::ReplaceText { block, .. } | Request::StyleText { block, .. } => {
            Some(Assistant::TargetBlock(block.clone()))
        }
        Request::SetProperties(_) | Request::AddStamp(_) | Request::Bookmarks(_) => {
            Some(Assistant::TargetDocument)
        }
        Request::FillField { name, .. } => Some(Assistant::TargetField(short(name))),
        Request::DeletePages(list) => Some(these_pages(list)),
        Request::MovePages { pages: list, .. } | Request::RotatePages { pages: list, .. } => {
            Some(these_pages(list))
        }
        Request::InsertPages { from, .. } => {
            Some(Assistant::TargetFile(from.file_name().map_or_else(
                || from.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )))
        }
        Request::FindAndReplace { search, .. } => {
            Some(Assistant::TargetQuery(short(&search.wanted)))
        }
        Request::MarkText { search, .. } => Some(Assistant::TargetQuery(short(&search.wanted))),
        Request::PlacePicture(asked) => Some(Assistant::TargetPage(asked.page + 1)),
        Request::Objects(action) => match action {
            pdf_agent::objects::Action::List { page } => Some(Assistant::TargetPage(page + 1)),
            pdf_agent::objects::Action::Move { object, .. }
            | pdf_agent::objects::Action::Resize { object, .. }
            | pdf_agent::objects::Action::Delete { object } => {
                Some(Assistant::TargetBlock(object.clone()))
            }
        },
        Request::GoToPage { page } | Request::LookCloser { page, .. } => {
            Some(Assistant::TargetPage(page + 1))
        }
        Request::Convert(asked) => Some(match asked.files.first() {
            Some(file) => Assistant::TargetFile(file.file_name().map_or_else(
                || file.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )),
            None => Assistant::TargetDocument,
        }),
        Request::OcrPages(asked) => Some(named_pages(&asked.pages)),
        Request::ExtractPages(asked) => Some(named_pages(&asked.pages)),
        Request::ExportPictures(asked) => Some(named_pages(&asked.pages)),
        Request::SplitDocument(_) | Request::SaveCopy { .. } => Some(Assistant::TargetDocument),
        Request::Links(action) => match action {
            pdf_agent::linking::Action::List { page }
            | pdf_agent::linking::Action::Add { page, .. } => Some(Assistant::TargetPage(page + 1)),
            pdf_agent::linking::Action::Remove { link } => {
                Some(Assistant::TargetBlock(link.clone()))
            }
        },
        Request::DrawShape(asked) => Some(Assistant::TargetPage(asked.page + 1)),
        Request::AddField(asked) => Some(Assistant::TargetPage(asked.page + 1)),
    }
}

fn named_pages(spec: &str) -> Assistant {
    let spec = spec.trim();
    if spec.is_empty() {
        Assistant::TargetDocument
    } else {
        Assistant::TargetPageNumbers(short(spec))
    }
}

fn these_pages(list: &[usize]) -> Assistant {
    let mut sorted: Vec<usize> = list.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    match sorted[..] {
        [] => Assistant::TargetDocument,
        [one] => Assistant::TargetPage(one + 1),
        [low, .., high] if high - low + 1 == sorted.len() => Assistant::TargetPages {
            first: low + 1,
            last: high + 1,
        },
        _ => Assistant::TargetCount(sorted.len()),
    }
}

fn short(text: &str) -> String {
    const MOST: usize = 30;
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= MOST {
        line.to_owned()
    } else {
        let cut: String = line.chars().take(MOST).collect();
        format!("{}\u{2026}", cut.trim_end())
    }
}

fn pages(first: usize, last: usize) -> String {
    if first == last {
        format!("page {}", first + 1)
    } else {
        format!("pages {} to {}", first + 1, last + 1)
    }
}

fn some_pages(list: &[usize]) -> String {
    match list {
        [one] => format!("page {}", one + 1),
        [one, two] => format!("pages {} and {}", one + 1, two + 1),
        many => format!("{} pages", many.len()),
    }
}

fn quoted(text: &str) -> String {
    const MOST: usize = 40;
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= MOST {
        format!("\u{201c}{line}\u{201d}")
    } else {
        let cut: String = line.chars().take(MOST).collect();
        format!("\u{201c}{}\u{2026}\u{201d}", cut.trim_end())
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one line of status for each tool, read as a table"
)]
fn doing_in_english(request: &Request) -> String {
    match request {
        Request::DocumentInfo => "Reading what the document says about itself".to_owned(),
        Request::ReadText { first, last } => match (first, last) {
            (None, None) => "Reading the document's text".to_owned(),
            (Some(first), Some(last)) => format!("Reading {}", pages(*first, *last)),
            (Some(first), None) => format!("Reading from page {}", first + 1),
            (None, Some(last)) => format!("Reading up to page {}", last + 1),
        },
        Request::FindText { text, .. } => format!("Searching for {}", quoted(text)),
        Request::RenderPage { page, .. } => format!("Looking at page {}", page + 1),
        Request::ListFonts { .. } => "Looking through the fonts".to_owned(),
        Request::ReplaceText { block, .. } => format!("Changing the text of {block}"),
        Request::AddText { page, .. } => format!("Adding text to page {}", page + 1),
        Request::WritePages { from_page, .. } => {
            format!("Writing the document from page {}", from_page + 1)
        }
        Request::SetProperties(_) => "Changing the document's properties".to_owned(),
        Request::FillField { name, .. } => format!("Filling in {}", quoted(name)),
        Request::AddBlankPage { .. } => "Adding a blank page".to_owned(),
        Request::DeletePages(list) => format!("Deleting {}", some_pages(list)),
        Request::MovePages { pages: list, .. } => format!("Moving {}", some_pages(list)),
        Request::RotatePages { pages: list, .. } => format!("Turning {}", some_pages(list)),
        Request::InsertPages { from, .. } => format!(
            "Putting in pages from {}",
            from.file_name().map_or_else(
                || from.display().to_string(),
                |name| name.to_string_lossy().into_owned()
            )
        ),
        Request::Undo => "Taking back the last change".to_owned(),
        Request::Redo => "Putting back the last change".to_owned(),
        Request::AskPerson { .. } => "Asking you".to_owned(),
        Request::UpdatePlan { .. } => "Planning the work".to_owned(),
        Request::FindAndReplace { search, with, .. } => {
            format!("Replacing {} with {}", quoted(&search.wanted), quoted(with))
        }
        Request::StyleText { block, .. } => format!("Changing how {block} looks"),
        Request::MarkText {
            search, marking, ..
        } => {
            let doing = match marking.how {
                pdf_agent::marking::How::Highlight => "Highlighting",
                pdf_agent::marking::How::Underline => "Underlining",
                pdf_agent::marking::How::StrikeThrough => "Striking through",
            };
            format!("{doing} {}", quoted(&search.wanted))
        }
        Request::AddStamp(asked) => match asked.kind {
            pdf_agent::stamping::Kind::PageNumbers => "Numbering the pages".to_owned(),
            pdf_agent::stamping::Kind::HeaderFooter => "Adding a header or footer".to_owned(),
            pdf_agent::stamping::Kind::Watermark => "Adding a watermark".to_owned(),
        },
        Request::Bookmarks(action) => match action {
            pdf_agent::outlining::Action::List => "Reading the bookmarks".to_owned(),
            pdf_agent::outlining::Action::FromHeadings { .. } => {
                "Making a table of contents".to_owned()
            }
            _ => "Changing the bookmarks".to_owned(),
        },
        Request::PlacePicture(asked) => format!("Placing a picture on page {}", asked.page + 1),
        Request::Objects(action) => match action {
            pdf_agent::objects::Action::List { page } => {
                format!("Looking at what is on page {}", page + 1)
            }
            pdf_agent::objects::Action::Move { object, .. } => format!("Moving {object}"),
            pdf_agent::objects::Action::Resize { object, .. } => format!("Resizing {object}"),
            pdf_agent::objects::Action::Delete { object } => format!("Deleting {object}"),
        },
        Request::GoToPage { page } => format!("Showing page {}", page + 1),
        Request::LookCloser { page, .. } => format!("Looking closer at page {}", page + 1),
        Request::Convert(asked) => converting_status(asked.tool).to_owned(),
        Request::OcrPages(asked) => {
            if asked.pages.trim().is_empty() {
                "Reading the scanned pages".to_owned()
            } else {
                format!("Reading pages {}", short(asked.pages.trim()))
            }
        }
        Request::ExtractPages(asked) => format!("Taking out pages {}", short(asked.pages.trim())),
        Request::SplitDocument(_) => "Splitting the document into files".to_owned(),
        Request::ExportPictures(_) => "Saving pages as pictures".to_owned(),
        Request::SaveCopy { .. } => "Saving a copy of the document".to_owned(),
        Request::Links(action) => match action {
            pdf_agent::linking::Action::List { page } => {
                format!("Looking at the links of page {}", page + 1)
            }
            pdf_agent::linking::Action::Add { page, .. } => {
                format!("Adding a link to page {}", page + 1)
            }
            pdf_agent::linking::Action::Remove { link } => format!("Removing link {link}"),
        },
        Request::DrawShape(asked) => format!(
            "Drawing {} on page {}",
            asked.shape.with_article(),
            asked.page + 1
        ),
        Request::AddField(asked) => format!("Adding a form field to page {}", asked.page + 1),
        Request::SetTabOrder { page, .. } => {
            format!("Ordering the form fields of page {}", page + 1)
        }
    }
}

fn converting_status(tool: pdf_convert::Tool) -> &'static str {
    use pdf_convert::Tool;
    match tool {
        Tool::PdfToWord => "Making a Word file",
        Tool::PdfToExcel => "Making an Excel file",
        Tool::PdfToPowerPoint => "Making a PowerPoint file",
        Tool::PdfToImage => "Making picture files",
        Tool::PdfToHtml => "Making a web page",
        Tool::PdfToMarkdown => "Making a Markdown file",
        Tool::PdfToText => "Making a text file",
        Tool::PdfToPdfA => "Making a PDF/A copy",
        Tool::WordToPdf
        | Tool::ExcelToPdf
        | Tool::PowerPointToPdf
        | Tool::ImageToPdf
        | Tool::ScanToPdf
        | Tool::HtmlToPdf => "Making a PDF",
        Tool::Compress => "Compressing a copy",
        Tool::Repair => "Repairing a copy",
        Tool::Ocr => "Making a searchable copy",
        Tool::Unlock => "Taking the password off a copy",
        Tool::Sign => "Signing a copy",
        Tool::Redact => "Removing words from a copy for good",
        Tool::Compare => "Comparing two files",
        Tool::Protect => "Protecting a copy with a password",
    }
}

fn did_in_english(name: &str) -> String {
    match name {
        "document_info" => "Read the document's facts",
        "read_text" => "Read the text",
        "find_text" => "Searched",
        "render_page" => "Looked at the page",
        "list_fonts" => "Looked through the fonts",
        "replace_text" => "Changed the text",
        "add_text" => "Added text",
        "write_pages" => "Wrote the document",
        "set_properties" => "Changed the properties",
        "fill_field" => "Filled in a field",
        "add_blank_page" => "Added a page",
        "delete_pages" => "Deleted pages",
        "move_pages" => "Moved pages",
        "rotate_pages" => "Turned pages",
        "insert_pages" => "Put in pages",
        "undo" => "Took back a change",
        "redo" => "Put back a change",
        "ask_person" => "Asked you",
        "update_plan" => "Updated the plan",
        "find_and_replace" => "Replaced text throughout",
        "style_text" => "Changed how text looks",
        "mark_text" => "Marked text",
        "add_stamp" => "Stamped the pages",
        "bookmarks" => "Worked on the bookmarks",
        "place_picture" => "Placed a picture",
        "objects" => "Worked on pictures and drawings",
        "go_to_page" => "Showed a page",
        "look_closer" => "Looked closer",
        "convert" => "Made a new file",
        "protect_document" => "Made a protected copy",
        "ocr_pages" => "Read scanned pages",
        "extract_pages" => "Took pages out",
        "split_document" => "Split the document",
        "export_page_pictures" => "Saved pages as pictures",
        "save_copy" => "Saved a copy",
        "links" => "Worked on links",
        "draw_shape" => "Drew a shape",
        "add_field" => "Added a form field",
        "set_tab_order" => "Ordered the form fields",
        other => return other.to_owned(),
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use pdf_agent::tools::request::Request;

    use super::{did, doing, target};
    use crate::wording::{Assistant, Lang};

    #[test]
    fn a_status_says_what_it_is_about() {
        let say = |request: Request| doing(&request, Lang::English);
        assert_eq!(
            say(Request::ReadText {
                first: Some(0),
                last: Some(2)
            }),
            "Reading pages 1 to 3"
        );
        assert_eq!(
            say(Request::ReadText {
                first: Some(1),
                last: Some(1)
            }),
            "Reading page 2"
        );
        assert_eq!(
            say(Request::FindText {
                text: "total due".to_owned(),
                match_case: false,
                first: None,
                last: None,
            }),
            "Searching for \u{201c}total due\u{201d}"
        );
        assert_eq!(
            say(Request::DeletePages(vec![1, 4])),
            "Deleting pages 2 and 5"
        );
        assert_eq!(
            say(Request::AskPerson {
                question: "Which?".to_owned(),
                options: Vec::new()
            }),
            "Asking you"
        );
        assert_eq!(
            say(Request::UpdatePlan { steps: Vec::new() }),
            "Planning the work"
        );
    }

    #[test]
    fn a_status_stays_one_line_and_a_done_line_is_words() {
        let long = doing(
            &Request::FindText {
                text: "a".repeat(100),
                match_case: false,
                first: None,
                last: None,
            },
            Lang::English,
        );
        assert!(long.chars().count() < 70, "{long}");
        assert!(long.ends_with("\u{2026}\u{201d}"));
        assert_eq!(did("ask_person", Lang::English), "Asked you");
        assert_eq!(did("update_plan", Lang::English), "Updated the plan");
        assert_eq!(did("read_text", Lang::English), "Read the text");
        assert_eq!(did("something_new", Lang::English), "something_new");
    }

    #[test]
    fn a_step_names_what_it_was_done_to() {
        let said = |request: Request| target(&request).map(|target| target.say(Lang::English));
        assert_eq!(
            said(Request::ReadText {
                first: Some(1),
                last: Some(1)
            })
            .as_deref(),
            Some("page 2")
        );
        assert_eq!(
            said(Request::ReadText {
                first: None,
                last: None
            })
            .as_deref(),
            Some("the document")
        );
        assert_eq!(
            said(Request::DeletePages(vec![4, 2, 3])).as_deref(),
            Some("pages 3 to 5")
        );
        assert_eq!(
            said(Request::DeletePages(vec![0, 5, 9])).as_deref(),
            Some("3 pages")
        );
        assert_eq!(
            said(Request::ReplaceText {
                block: "p2-b3".to_owned(),
                find: None,
                text: String::new()
            })
            .as_deref(),
            Some("p2-b3")
        );
        assert_eq!(said(Request::Undo), None);
        assert_eq!(
            target(&Request::FindText {
                text: "a very long thing to look for in the whole of the document".to_owned(),
                match_case: false,
                first: None,
                last: None
            }),
            Some(Assistant::TargetQuery(
                "a very long thing to look for\u{2026}".to_owned()
            ))
        );
    }

    fn asked(name: &str, arguments: &str) -> Request {
        let arguments = pdf_agent::json::Json::parse(arguments).expect("JSON");
        pdf_agent::tools::request::parse(name, &arguments).expect("a call that reads")
    }

    #[test]
    fn every_tool_a_window_offers_has_a_done_line_in_words() {
        for tool in pdf_agent::tools::offered_to_a_window() {
            assert_ne!(did(&tool.name, Lang::English), tool.name, "{}", tool.name);
        }
    }

    #[test]
    fn the_editing_tools_say_what_they_are_doing_and_to_what() {
        let rows = [
            (
                "find_and_replace",
                r#"{"find":"cat","replace_with":"dog"}"#,
                "Replacing \u{201c}cat\u{201d} with \u{201c}dog\u{201d}",
                "\u{201c}cat\u{201d}",
            ),
            (
                "style_text",
                r#"{"block":"p2-b3","bold":true}"#,
                "Changing how p2-b3 looks",
                "p2-b3",
            ),
            (
                "mark_text",
                r#"{"text":"Bangkok","how":"underline"}"#,
                "Underlining \u{201c}Bangkok\u{201d}",
                "\u{201c}Bangkok\u{201d}",
            ),
            (
                "add_stamp",
                r#"{"kind":"page_numbers"}"#,
                "Numbering the pages",
                "the document",
            ),
            (
                "bookmarks",
                r#"{"action":"from_headings"}"#,
                "Making a table of contents",
                "the document",
            ),
            (
                "place_picture",
                r#"{"page":2,"left":72,"top":72,"path":"/tmp/a.png"}"#,
                "Placing a picture on page 2",
                "page 2",
            ),
            (
                "objects",
                r#"{"action":"move","object":"p1-o2","left":10}"#,
                "Moving p1-o2",
                "p1-o2",
            ),
            ("go_to_page", r#"{"page":4}"#, "Showing page 4", "page 4"),
            (
                "look_closer",
                r#"{"page":3,"left":0,"top":0,"right":100,"bottom":50}"#,
                "Looking closer at page 3",
                "page 3",
            ),
        ];
        for (name, arguments, doing_says, to) in rows {
            let request = asked(name, arguments);
            assert_eq!(doing(&request, Lang::English), doing_says, "{name}");
            assert_eq!(
                target(&request)
                    .map(|target| target.say(Lang::English))
                    .as_deref(),
                Some(to),
                "{name}"
            );
        }
    }

    const ONE_CALL_OF_EACH: &[(&str, &str)] = &[
        ("document_info", "{}"),
        ("read_text", "{}"),
        ("find_text", r#"{"text":"a"}"#),
        ("render_page", r#"{"page":1}"#),
        ("list_fonts", "{}"),
        ("replace_text", r#"{"block":"p1-b1","text":"x"}"#),
        (
            "add_text",
            r#"{"page":1,"left":0,"top":0,"width":50,"text":"x","font":"DejaVu Sans"}"#,
        ),
        (
            "write_pages",
            r##"{"markdown":"# a","font":"DejaVu Sans"}"##,
        ),
        ("set_properties", r#"{"title":"a"}"#),
        ("fill_field", r#"{"name":"a","value":"b"}"#),
        ("add_blank_page", r#"{"after_page":0}"#),
        ("delete_pages", r#"{"pages":[1]}"#),
        ("move_pages", r#"{"pages":[1],"to":2}"#),
        ("rotate_pages", r#"{"pages":[1],"degrees":90}"#),
        ("insert_pages", r#"{"from":"/tmp/a.pdf","after_page":0}"#),
        ("undo", "{}"),
        ("redo", "{}"),
        ("find_and_replace", r#"{"find":"a","replace_with":"b"}"#),
        ("style_text", r#"{"block":"p1-b1","bold":true}"#),
        ("mark_text", r#"{"text":"a"}"#),
        ("add_stamp", r#"{"kind":"watermark"}"#),
        ("bookmarks", r#"{"action":"list"}"#),
        (
            "place_picture",
            r#"{"page":1,"left":0,"top":0,"path":"/tmp/a.png"}"#,
        ),
        ("objects", r#"{"action":"list","page":1}"#),
        (
            "look_closer",
            r#"{"page":1,"left":0,"top":0,"right":9,"bottom":9}"#,
        ),
        ("convert", r#"{"tool":"pdf-to-text"}"#),
        ("protect_document", r#"{"password":"x"}"#),
        ("ocr_pages", "{}"),
        ("extract_pages", r#"{"pages":"1"}"#),
        ("split_document", r#"{"every":1}"#),
        ("export_page_pictures", "{}"),
        ("save_copy", "{}"),
        ("links", r#"{"action":"list","page":1}"#),
        (
            "draw_shape",
            r#"{"page":1,"shape":"line","left":0,"top":0,"right":9,"bottom":9}"#,
        ),
        (
            "add_field",
            r#"{"page":1,"kind":"text","left":0,"top":0,"width":50,"height":20}"#,
        ),
        ("set_tab_order", r#"{"page":1,"order":"rows"}"#),
        ("go_to_page", r#"{"page":1}"#),
        (
            "ask_person",
            r#"{"question":"Which?","options":[{"label":"a"},{"label":"b"}]}"#,
        ),
        ("update_plan", r#"{"steps":[{"text":"a","status":"done"}]}"#),
    ];

    #[test]
    fn every_tool_a_window_offers_can_be_worded_before_it_runs_and_while_it_runs() {
        let offered: Vec<String> = pdf_agent::tools::offered_to_a_window()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        for name in &offered {
            let Some((_, arguments)) = ONE_CALL_OF_EACH.iter().find(|(held, _)| held == name)
            else {
                panic!("{name} is offered but has no call in this table: add one");
            };
            let request = asked(name, arguments);
            assert!(
                !crate::ai_permission::describe_call(&request, Lang::English)
                    .trim()
                    .is_empty(),
                "{name} says nothing on its card"
            );
            assert!(!doing(&request, Lang::English).trim().is_empty(), "{name}");
            assert_ne!(did(name, Lang::English), *name, "{name}");
        }
        for (name, _) in ONE_CALL_OF_EACH {
            assert!(
                offered.iter().any(|held| held == name),
                "{name} is in the table and is not offered"
            );
        }
        assert_eq!(offered.len(), ONE_CALL_OF_EACH.len());
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one row per tool, read as a table")]
    fn the_tools_for_files_scans_links_shapes_and_forms_say_what_they_are_doing_and_to_what() {
        let rows = [
            (
                "convert",
                r#"{"tool":"pdf-to-word"}"#,
                "Making a Word file",
                "the document",
            ),
            (
                "convert",
                r#"{"tool":"compress-pdf","files":["/a/big.pdf"]}"#,
                "Compressing a copy",
                "big.pdf",
            ),
            (
                "convert",
                r#"{"tool":"redact-pdf","options":{"search":["x"]}}"#,
                "Removing words from a copy for good",
                "the document",
            ),
            (
                "protect_document",
                r#"{"password":"x"}"#,
                "Protecting a copy with a password",
                "the document",
            ),
            (
                "ocr_pages",
                r#"{"pages":"2-3"}"#,
                "Reading pages 2-3",
                "pages 2-3",
            ),
            (
                "ocr_pages",
                "{}",
                "Reading the scanned pages",
                "the document",
            ),
            (
                "extract_pages",
                r#"{"pages":"1, 4"}"#,
                "Taking out pages 1, 4",
                "pages 1, 4",
            ),
            (
                "split_document",
                r#"{"every":5}"#,
                "Splitting the document into files",
                "the document",
            ),
            (
                "export_page_pictures",
                "{}",
                "Saving pages as pictures",
                "the document",
            ),
            (
                "save_copy",
                "{}",
                "Saving a copy of the document",
                "the document",
            ),
            (
                "links",
                r#"{"action":"list","page":3}"#,
                "Looking at the links of page 3",
                "page 3",
            ),
            (
                "links",
                r#"{"action":"add","block":"p2-b1","url":"https://a.org"}"#,
                "Adding a link to page 2",
                "page 2",
            ),
            (
                "links",
                r#"{"action":"remove","link":"p1-l2"}"#,
                "Removing link p1-l2",
                "p1-l2",
            ),
            (
                "draw_shape",
                r#"{"page":2,"shape":"ellipse","left":0,"top":0,"right":9,"bottom":9}"#,
                "Drawing an ellipse on page 2",
                "page 2",
            ),
            (
                "add_field",
                r#"{"page":1,"kind":"text","left":0,"top":0,"width":50,"height":20}"#,
                "Adding a form field to page 1",
                "page 1",
            ),
            (
                "set_tab_order",
                r#"{"page":4,"order":"rows"}"#,
                "Ordering the form fields of page 4",
                "page 4",
            ),
        ];
        for (name, arguments, doing_says, to) in rows {
            let request = asked(name, arguments);
            assert_eq!(doing(&request, Lang::English), doing_says, "{name}");
            assert_eq!(
                target(&request)
                    .map(|target| target.say(Lang::English))
                    .as_deref(),
                Some(to),
                "{name}"
            );
        }
        for name in [
            "convert",
            "protect_document",
            "ocr_pages",
            "extract_pages",
            "split_document",
            "export_page_pictures",
            "save_copy",
            "links",
            "draw_shape",
            "add_field",
            "set_tab_order",
        ] {
            assert_ne!(
                did(name, Lang::English),
                name,
                "{name} has a line for what it did"
            );
        }
    }
}
