use pdf_convert::{Choice, Group, Kind, Setting, Tool, Value, Values};

use super::{
    Manner, Missing, Trouble, accept, arranged, defaults_to_the_document, extensions, found,
    manner, named_after, placed, ready, room_for_more, shown, sign_strokes, stem_of, takes_several,
    terms_in, trouble_in, unused,
};
use crate::wording::Lang;

fn names(tool: Tool, names: &[&str], values: &Values) -> Result<(), Missing> {
    ready(tool, names, values, "")
}

#[test]
fn a_tool_that_takes_a_file_is_not_ready_without_one() {
    for tool in Tool::ALL {
        assert_eq!(
            names(tool, &[], &Values::new()),
            Err(if tool == Tool::Compare {
                Missing::TwoFiles
            } else {
                Missing::AFile
            }),
            "{tool:?} ready with nothing"
        );
    }
}

#[test]
fn most_tools_are_ready_with_one_file_and_no_settings_touched() {
    for tool in [
        Tool::PdfToWord,
        Tool::PdfToExcel,
        Tool::PdfToImage,
        Tool::PdfToPdfA,
        Tool::WordToPdf,
        Tool::ExcelToPdf,
        Tool::ImageToPdf,
        Tool::ScanToPdf,
        Tool::Compress,
        Tool::Repair,
        Tool::Unlock,
    ] {
        assert_eq!(names(tool, &["a.file"], &Values::new()), Ok(()), "{tool:?}");
    }
}

#[test]
fn comparing_takes_exactly_two_files_and_no_more() {
    let none = Values::new();
    assert_eq!(
        names(Tool::Compare, &["old.pdf"], &none),
        Err(Missing::TheNewerFile)
    );
    assert_eq!(names(Tool::Compare, &["old.pdf", "new.pdf"], &none), Ok(()));
    assert_eq!(
        names(Tool::Compare, &["a.pdf", "b.pdf", "c.pdf"], &none),
        Err(Missing::TwoFiles)
    );
}

#[test]
fn a_web_page_is_not_ready_until_the_page_itself_is_among_the_files() {
    let none = Values::new();
    assert_eq!(
        names(Tool::HtmlToPdf, &["logo.png", "style.css"], &none),
        Err(Missing::TheWebPage)
    );
    assert_eq!(
        names(Tool::HtmlToPdf, &["logo.png", "page.HTML"], &none),
        Ok(())
    );
    assert_eq!(names(Tool::HtmlToPdf, &["old.htm"], &none), Ok(()));
}

#[test]
fn redaction_waits_for_words_or_the_files_own_marks() {
    let mut values = Values::new();
    assert_eq!(
        names(Tool::Redact, &["a.pdf"], &values),
        Err(Missing::WordsToRemove)
    );
    values.set(Setting::Search, Value::Terms(Vec::new()));
    assert_eq!(
        names(Tool::Redact, &["a.pdf"], &values),
        Err(Missing::WordsToRemove)
    );
    values.set(Setting::Search, Value::Terms(vec!["12345".to_owned()]));
    assert_eq!(names(Tool::Redact, &["a.pdf"], &values), Ok(()));
    let marks = Values::new().with(Setting::UseMarks, Value::Flag(true));
    assert_eq!(names(Tool::Redact, &["a.pdf"], &marks), Ok(()));
}

#[test]
fn protecting_waits_for_a_password_typed_twice_the_same() {
    let values = Values::new();
    assert_eq!(
        ready(Tool::Protect, &["a.pdf"], &values, ""),
        Err(Missing::APassword)
    );
    let values = Values::new().with(Setting::NewPassword, Value::Secret("sesame".to_owned()));
    assert_eq!(
        ready(Tool::Protect, &["a.pdf"], &values, ""),
        Err(Missing::PasswordsDiffer)
    );
    assert_eq!(
        ready(Tool::Protect, &["a.pdf"], &values, "sesamE"),
        Err(Missing::PasswordsDiffer)
    );
    assert_eq!(ready(Tool::Protect, &["a.pdf"], &values, "sesame"), Ok(()));
}

#[test]
fn a_signature_is_ready_when_what_the_chosen_way_needs_is_there() {
    let one = ["a.pdf"];
    assert_eq!(
        names(Tool::Sign, &one, &Values::new()),
        Err(Missing::TheDrawing)
    );
    let dot = Values::new().with(Setting::Drawing, Value::Strokes(vec![vec![(0.0, 0.0)]]));
    assert_eq!(names(Tool::Sign, &one, &dot), Err(Missing::TheDrawing));
    let line = Values::new().with(
        Setting::Drawing,
        Value::Strokes(vec![vec![(0.0, 0.0), (1.0, 0.5)]]),
    );
    assert_eq!(names(Tool::Sign, &one, &line), Ok(()));
    let typed = Values::new().with(Setting::Signature, Value::Choice(Choice::Typed));
    assert_eq!(names(Tool::Sign, &one, &typed), Err(Missing::TheName));
    let typed = typed.with(Setting::TypedName, Value::Text("  ".to_owned()));
    assert_eq!(names(Tool::Sign, &one, &typed), Err(Missing::TheName));
    let typed = typed.with(Setting::TypedName, Value::Text("Ada".to_owned()));
    assert_eq!(names(Tool::Sign, &one, &typed), Ok(()));
    let picture = Values::new().with(Setting::Signature, Value::Choice(Choice::FromPicture));
    assert_eq!(names(Tool::Sign, &one, &picture), Err(Missing::ThePicture));
    let picture = picture.with(
        Setting::SignaturePicture,
        Value::File {
            name: "me.png".to_owned(),
            bytes: vec![1, 2, 3],
        },
    );
    assert_eq!(names(Tool::Sign, &one, &picture), Ok(()));
}

#[test]
fn a_drawing_in_a_signature_that_the_way_does_not_use_does_not_make_it_ready() {
    let typed_but_drawn = Values::new()
        .with(Setting::Signature, Value::Choice(Choice::Typed))
        .with(
            Setting::Drawing,
            Value::Strokes(vec![vec![(0.0, 0.0), (1.0, 1.0)]]),
        );
    assert_eq!(
        names(Tool::Sign, &["a.pdf"], &typed_but_drawn),
        Err(Missing::TheName)
    );
}

#[test]
fn the_open_document_is_the_first_file_only_for_a_tool_that_starts_from_a_pdf() {
    for tool in Tool::ALL {
        let expected = tool.starts_from_a_pdf() && tool != Tool::Ocr;
        assert_eq!(defaults_to_the_document(tool, true), expected, "{tool:?}");
        assert!(!defaults_to_the_document(tool, false), "{tool:?}");
    }
    assert!(defaults_to_the_document(Tool::Compress, true));
    assert!(defaults_to_the_document(Tool::Compare, true));
    assert!(!defaults_to_the_document(Tool::WordToPdf, true));
    assert!(!defaults_to_the_document(Tool::ImageToPdf, true));
    assert!(!defaults_to_the_document(Tool::HtmlToPdf, true));
}

#[test]
fn only_files_of_the_kind_a_tool_takes_are_taken_and_only_as_many_as_it_has_room_for() {
    let offered = ["a.docx", "b.pdf", "c.DOCX", "d.txt", "e.docx"];
    assert_eq!(accept(Tool::WordToPdf, &offered, usize::MAX), vec![0, 2, 4]);
    assert_eq!(accept(Tool::WordToPdf, &offered, 2), vec![0, 2]);
    assert_eq!(accept(Tool::Compress, &offered, 5), vec![1]);
    assert_eq!(accept(Tool::WordToPdf, &offered, 0), Vec::<usize>::new());
    assert_eq!(
        accept(Tool::HtmlToPdf, &["p.html", "x.css", "i.svg", "n.txt"], 9),
        vec![0, 1, 2]
    );
}

#[test]
fn room_runs_out_where_a_tool_takes_one_or_two_files() {
    assert_eq!(room_for_more(Tool::Sign, 0), 1);
    assert_eq!(room_for_more(Tool::Sign, 1), 0);
    assert_eq!(room_for_more(Tool::Compare, 1), 1);
    assert_eq!(room_for_more(Tool::Compress, 40), usize::MAX);
    assert!(takes_several(Tool::Compress));
    assert!(!takes_several(Tool::Redact));
}

#[test]
fn the_file_chooser_offers_what_the_catalogue_says_a_tool_accepts() {
    for tool in Tool::ALL {
        let offered = extensions(tool);
        for extension in offered {
            assert!(
                tool.takes(&format!("file.{extension}")),
                "{tool:?} offers .{extension} and does not take it"
            );
        }
        for kind in tool.accepts() {
            for extension in kind.extensions() {
                assert!(
                    offered.contains(extension),
                    "{tool:?} takes .{extension} and does not offer it"
                );
            }
        }
    }
}

#[test]
fn settings_up_front_are_the_essential_ones_and_the_rest_wait_under_more() {
    let (first, more) = arranged(Tool::PdfToImage, &Values::new());
    assert_eq!(
        first,
        vec![
            Setting::WhatToTake,
            Setting::PictureFormat,
            Setting::Resolution,
            Setting::Pages
        ]
    );
    assert_eq!(more, vec![Setting::PictureQuality, Setting::Password]);
    let (first, more) = arranged(Tool::Unlock, &Values::new());
    assert!(first.is_empty());
    assert_eq!(more, vec![Setting::Password]);
    let (first, more) = arranged(Tool::PdfToPdfA, &Values::new());
    assert!(first.is_empty());
    assert_eq!(more, vec![Setting::Password]);
}

#[test]
fn a_setting_that_depends_on_another_appears_only_when_that_one_says_so() {
    let pages = Values::new();
    assert!(shown(Setting::Resolution, &pages));
    assert!(!shown(
        Setting::PictureQuality,
        &Values::new().with(Setting::PictureFormat, Value::Choice(Choice::Png))
    ));
    assert!(shown(Setting::PictureQuality, &pages));
    let inside = Values::new().with(Setting::WhatToTake, Value::Choice(Choice::PicturesInside));
    assert!(!shown(Setting::Resolution, &inside));
    let (first, _) = arranged(Tool::PdfToImage, &inside);
    assert!(!first.contains(&Setting::Resolution));
    let (first, _) = arranged(Tool::Sign, &Values::new());
    assert!(first.contains(&Setting::Drawing));
    assert!(!first.contains(&Setting::TypedName));
    let typed = Values::new().with(Setting::Signature, Value::Choice(Choice::Typed));
    let (first, _) = arranged(Tool::Sign, &typed);
    assert!(first.contains(&Setting::TypedName));
    assert!(!first.contains(&Setting::Drawing));
}

#[test]
fn what_the_room_cannot_draw_is_never_offered() {
    for tool in Tool::ALL {
        let (first, more) = arranged(tool, &Values::new());
        for setting in first.iter().chain(&more) {
            assert_ne!(manner(*setting), Manner::Hidden, "{tool:?} {setting:?}");
        }
    }
    assert!(!shown(Setting::Areas, &Values::new()));
}

#[test]
fn a_run_of_cards_is_for_the_settings_whose_choices_each_need_a_sentence() {
    for setting in Setting::ALL {
        if manner(setting) == Manner::Cards {
            assert!(setting.choices().len() >= 2, "{setting:?}");
        }
        if manner(setting) == Manner::Segments {
            assert!(
                (2..=4).contains(&setting.choices().len()),
                "{setting:?} has {} choices for a row of buttons",
                setting.choices().len()
            );
        }
    }
}

#[test]
fn words_to_remove_are_split_at_commas_and_new_lines() {
    assert_eq!(
        terms_in("account 12345, Jane Doe\n  secret ,, "),
        vec!["account 12345", "Jane Doe", "secret"]
    );
    assert!(terms_in("  ,  ").is_empty());
}

#[test]
fn tools_are_found_by_what_they_are_called_and_what_they_do() {
    let english = Lang::English;
    assert_eq!(found("", None, english).len(), 22);
    let word = found("word", None, english);
    assert!(word.contains(&Tool::PdfToWord) && word.contains(&Tool::WordToPdf));
    assert!(word.contains(&Tool::PdfToText), "the words of a PDF");
    assert_eq!(found("COMPRESS", None, english), vec![Tool::Compress]);
    assert!(found("password", None, english).contains(&Tool::Protect));
    assert!(found("password", None, english).contains(&Tool::Unlock));
    assert_eq!(found("zzzz", None, english), Vec::<Tool>::new());
    let security = found("", Some(Group::Security), english);
    assert_eq!(security.len(), 5);
    assert!(security.iter().all(|tool| tool.group() == Group::Security));
    assert_eq!(
        found("excel", Some(Group::ToPdf), english),
        vec![Tool::ExcelToPdf]
    );
    assert_eq!(
        found("  searchable   scanned ", None, english),
        vec![Tool::Ocr]
    );
}

#[test]
fn a_name_without_its_ending_is_what_a_result_is_named_after() {
    assert_eq!(stem_of("report.pdf"), "report");
    assert_eq!(stem_of("/home/a/report.final.pdf"), "report.final");
    assert_eq!(stem_of("C:\\docs\\scan.PDF"), "scan");
    assert_eq!(stem_of(".hidden"), ".hidden");
    assert_eq!(stem_of(""), "result");
}

#[test]
fn one_plain_file_goes_beside_the_original_and_several_go_into_a_folder() {
    let one = placed(&["report.docx".to_owned()], &["report.pdf"]);
    assert_eq!(one.folder, None);
    let pictures: Vec<String> = (1..=3).map(|n| format!("report-{n}.jpg")).collect();
    let several = placed(&pictures, &["report.pdf"]);
    assert_eq!(several.folder.as_deref(), Some("report"));
    let with_attachments = placed(
        &["report.md".to_owned(), "report_files/image1.png".to_owned()],
        &["report.pdf"],
    );
    assert_eq!(with_attachments.folder.as_deref(), Some("report"));
    let nested_alone = placed(&["report/report.md".to_owned()], &["report.pdf"]);
    assert_eq!(nested_alone.folder.as_deref(), Some("report"));
    let batch = placed(
        &[
            "a.docx".to_owned(),
            "b.docx".to_owned(),
            "c.docx".to_owned(),
        ],
        &["a.pdf", "b.pdf", "c.pdf"],
    );
    assert_eq!(batch.folder.as_deref(), Some("a-and-2-more"));
    let none = placed(&[], &[]);
    assert_eq!(none.folder.as_deref(), Some("result"));
}

#[test]
fn a_comparison_is_named_after_the_older_file() {
    assert_eq!(
        named_after(Tool::Compare, "comparison.html", &["v1.pdf", "v2.pdf"]),
        "v1-comparison.html"
    );
    assert_eq!(
        named_after(Tool::Compare, "comparison.txt", &["v1.pdf", "v2.pdf"]),
        "v1-comparison.txt"
    );
    assert_eq!(
        named_after(Tool::Compress, "a-compressed.pdf", &["a.pdf"]),
        "a-compressed.pdf"
    );
}

#[test]
fn a_name_already_taken_is_numbered_from_two_and_never_overwritten() {
    let taken = |name: &str| ["a.docx", "a-2.docx", "dir"].contains(&name);
    assert_eq!(unused("b.docx", &taken, true), "b.docx");
    assert_eq!(unused("a.docx", &taken, true), "a-3.docx");
    assert_eq!(unused("dir", &taken, false), "dir-2");
    let dotted = |name: &str| name == "report.v2";
    assert_eq!(unused("report.v2", &dotted, false), "report.v2-2");
    assert_eq!(unused("report.v2", &dotted, true), "report-2.v2");
    assert_eq!(
        unused("noending", &|name: &str| name == "noending", true),
        "noending-2"
    );
}

fn as_the_tool_places_it(strokes: &[Vec<(f64, f64)>], width: f64) -> (f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (1.0_f64, 1.0_f64, 0.0_f64, 0.0_f64);
    for &(x, y) in strokes.iter().flatten() {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let aspect = ((y1 - y0).max(0.05) / (x1 - x0).max(0.05)).clamp(0.1, 2.0);
    let height = width * aspect;
    let (mut left, mut bottom, mut right, mut top) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in strokes.iter().flatten() {
        let (px, py) = (x * width, y * height);
        left = left.min(px);
        right = right.max(px);
        bottom = bottom.min(py);
        top = top.max(py);
    }
    (right - left, top - bottom)
}

#[test]
fn a_drawn_signature_keeps_its_shape_whatever_its_proportions() {
    for (across, down) in [
        (400.0_f32, 100.0_f32),
        (300.0, 300.0),
        (120.0, 240.0),
        (500.0, 40.0),
    ] {
        let pad = vec![vec![
            (10.0, 10.0 + down),
            (10.0 + across * 0.5, 10.0),
            (10.0 + across, 10.0 + down * 0.5),
        ]];
        let strokes = sign_strokes(&pad);
        let (width, height) = as_the_tool_places_it(&strokes, 160.0);
        let wanted = f64::from(down) / f64::from(across);
        assert!(
            (height / width - wanted).abs() < 1e-6,
            "{across}x{down}: {} against {wanted}",
            height / width
        );
        for &(x, y) in strokes.iter().flatten() {
            assert!((0.0..=1.0 + 1e-9).contains(&x), "{x}");
            assert!((0.0..=1.0 + 1e-9).contains(&y), "{y}");
        }
    }
}

#[test]
fn a_signature_is_turned_the_right_way_up() {
    let pad = vec![vec![(0.0, 0.0), (100.0, 100.0)]];
    let strokes = sign_strokes(&pad);
    let first = strokes[0][0];
    let last = strokes[0][1];
    assert!(
        first.1 > last.1,
        "the top of the pad is the top of the signature"
    );
    assert!(first.0 < last.0);
    assert!(sign_strokes(&[]).is_empty());
}

#[test]
fn what_a_tool_says_when_it_fails_is_put_in_plain_words() {
    for (said, expected) in [
        ("PDF header does not begin at byte zero", Trouble::NotAPdf),
        (
            "not a Word document: Zip(NotZip)",
            Trouble::NotAWordDocument,
        ),
        ("this is not an Excel workbook", Trouble::NotAWorkbook),
        (
            "This is not a PowerPoint presentation",
            Trouble::NotAPresentation,
        ),
        (
            "page 9 was asked for and the file has 2 pages",
            Trouble::PageNotThere,
        ),
        ("'x': pages count from 1", Trouble::PageNotThere),
        ("the searched words were not found", Trouble::NoMatches),
        ("nothing to redact", Trouble::NothingToRedact),
        ("no pictures were found on these pages", Trouble::NoPictures),
        (
            "the signature picture is not a PNG or a JPEG",
            Trouble::PictureUnreadable,
        ),
        (
            "the file is damaged (xref); repair it first",
            Trouble::Damaged,
        ),
        (
            "the converter stopped unexpectedly: boom",
            Trouble::Unexpected,
        ),
        ("something nobody has met", Trouble::Other),
    ] {
        assert_eq!(trouble_in(said), expected, "{said}");
    }
}

#[test]
fn the_kinds_of_file_are_told_apart_by_their_ending() {
    assert!(Kind::Word.matches("a.DOCX"));
    assert!(!Kind::Word.matches("a.doc"));
}

#[test]
fn a_tool_that_works_on_all_its_files_as_one_job_does_not_count_files() {
    for tool in [
        Tool::ImageToPdf,
        Tool::ScanToPdf,
        Tool::PdfToImage,
        Tool::HtmlToPdf,
        Tool::Compare,
    ] {
        assert!(!super::reports_each_file(tool), "{tool:?}");
    }
    for tool in [
        Tool::PdfToWord,
        Tool::Compress,
        Tool::WordToPdf,
        Tool::Protect,
    ] {
        assert!(super::reports_each_file(tool), "{tool:?}");
    }
}

#[test]
fn only_pictures_made_into_pages_care_which_comes_first() {
    for tool in Tool::ALL {
        assert_eq!(
            super::order_matters(tool),
            matches!(tool, Tool::ImageToPdf | Tool::ScanToPdf),
            "{tool:?}"
        );
    }
}
