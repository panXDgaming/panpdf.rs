use std::collections::HashSet;

use super::{
    Area, Choice, Group, Inputs, Kind, Made, Setting, SettingKind, Tool, Value, Values, Why,
};

const WEB_ORDER: [&str; 22] = [
    "pdf-to-word",
    "pdf-to-excel",
    "pdf-to-powerpoint",
    "pdf-to-jpg",
    "pdf-to-html",
    "pdf-to-markdown",
    "pdf-to-text",
    "pdf-to-pdfa",
    "word-to-pdf",
    "excel-to-pdf",
    "powerpoint-to-pdf",
    "jpg-to-pdf",
    "scan-to-pdf",
    "html-to-pdf",
    "compress-pdf",
    "repair-pdf",
    "ocr-pdf",
    "unlock-pdf",
    "sign-pdf",
    "redact-pdf",
    "compare-pdf",
    "protect-pdf",
];

fn near(found: Option<f64>, wanted: f64) -> bool {
    found.is_some_and(|n| (n - wanted).abs() < 1e-9)
}

#[test]
fn there_are_twenty_two_tools_in_the_order_the_website_lists_them() {
    assert_eq!(Tool::ALL.len(), 22);
    let slugs: Vec<&str> = Tool::ALL.into_iter().map(Tool::slug).collect();
    assert_eq!(slugs, WEB_ORDER);
    for (at, tool) in Tool::ALL.iter().enumerate() {
        assert_eq!(*tool as usize, at, "{tool:?} is out of place in ALL");
    }
}

#[test]
fn slugs_are_unique_and_each_finds_its_tool_again() {
    let mut seen = HashSet::new();
    for tool in Tool::ALL {
        assert!(seen.insert(tool.slug()), "{} is used twice", tool.slug());
        assert_eq!(Tool::from_slug(tool.slug()), Some(tool));
    }
    assert_eq!(Tool::from_slug("no-such-tool"), None);
    assert_eq!(Tool::from_slug(" PDF-to-Word "), Some(Tool::PdfToWord));
}

#[test]
fn the_engines_own_names_are_understood_as_slugs_too() {
    assert_eq!(Tool::from_slug("pdf-to-image"), Some(Tool::PdfToImage));
    assert_eq!(Tool::from_slug("image-to-pdf"), Some(Tool::ImageToPdf));
}

#[test]
fn the_groups_hold_the_tools_the_website_puts_in_them() {
    let of = |group: Group| -> Vec<&'static str> { group.tools().map(Tool::slug).collect() };
    assert_eq!(of(Group::FromPdf), WEB_ORDER[..8]);
    assert_eq!(of(Group::ToPdf), WEB_ORDER[8..14]);
    assert_eq!(
        of(Group::Optimize),
        ["compress-pdf", "repair-pdf", "ocr-pdf"]
    );
    assert_eq!(
        of(Group::Security),
        [
            "unlock-pdf",
            "sign-pdf",
            "redact-pdf",
            "compare-pdf",
            "protect-pdf"
        ]
    );
    assert_eq!(Group::ALL.len(), 4);
}

#[test]
fn every_tool_lists_each_of_its_settings_once_under_its_own_key() {
    for tool in Tool::ALL {
        let mut keys = HashSet::new();
        let mut settings = HashSet::new();
        for &setting in tool.settings() {
            assert!(settings.insert(setting), "{tool:?} lists {setting:?} twice");
            assert!(
                keys.insert(setting.key()),
                "{tool:?} has two settings under the key {}",
                setting.key()
            );
        }
    }
}

#[test]
fn every_default_choice_is_among_the_choices_offered() {
    for setting in Setting::ALL {
        if let SettingKind::Choice { choices, default } = setting.kind() {
            assert!(choices.contains(&default), "{setting:?}");
            assert!(!choices.is_empty(), "{setting:?}");
        }
    }
}

#[test]
fn the_choices_of_one_setting_never_share_a_value() {
    for setting in Setting::ALL {
        let mut seen = HashSet::new();
        for choice in setting.choices() {
            assert!(
                seen.insert(choice.value()),
                "{setting:?} offers {:?} twice",
                choice.value()
            );
        }
    }
}

#[test]
fn every_rule_about_when_a_setting_shows_names_a_choice_of_the_same_tool() {
    let mut rules = 0;
    for tool in Tool::ALL {
        for (at, &setting) in tool.settings().iter().enumerate() {
            let Some((on, choice)) = setting.when() else {
                continue;
            };
            rules += 1;
            let before = tool.settings()[..at].contains(&on);
            assert!(
                before,
                "{tool:?}: {setting:?} waits on {on:?}, which is not before it"
            );
            assert!(
                matches!(on.kind(), SettingKind::Choice { .. }),
                "{setting:?} waits on {on:?}, which is not a choice"
            );
            assert!(
                on.choices().contains(&choice),
                "{setting:?} waits on {choice:?}, which {on:?} does not offer"
            );
        }
    }
    assert_eq!(rules, 7);
}

#[test]
fn a_setting_that_waits_on_a_choice_never_waits_on_itself() {
    for setting in Setting::ALL {
        if let Some((on, _)) = setting.when() {
            assert_ne!(on, setting);
            assert_eq!(on.when(), None, "{setting:?} waits on a setting that waits");
        }
    }
}

#[test]
fn the_tables_of_settings_and_choices_list_every_variant_in_order() {
    for (at, setting) in Setting::ALL.iter().enumerate() {
        assert_eq!(*setting as usize, at, "{setting:?}");
    }
    assert_eq!(Setting::FilePassword as usize + 1, Setting::ALL.len());
    for (at, choice) in Choice::ALL.iter().enumerate() {
        assert_eq!(*choice as usize, at, "{choice:?}");
    }
    assert_eq!(Choice::Assemble as usize + 1, Choice::ALL.len());
}

#[test]
fn every_setting_belongs_to_a_tool_and_every_choice_to_a_setting() {
    let in_tools: HashSet<Setting> = Tool::ALL
        .iter()
        .flat_map(|tool| tool.settings().iter().copied())
        .collect();
    for setting in Setting::ALL {
        assert!(in_tools.contains(&setting), "{setting:?} is in no tool");
    }
    let offered: HashSet<Choice> = Setting::ALL
        .iter()
        .flat_map(|setting| setting.choices().iter().copied())
        .collect();
    for choice in Choice::ALL {
        assert!(offered.contains(&choice), "{choice:?} is offered nowhere");
    }
}

#[test]
fn every_number_keeps_its_default_inside_its_range() {
    for setting in Setting::ALL {
        if let SettingKind::Number {
            least,
            most,
            default,
            step,
        } = setting.kind()
        {
            assert!(least < most, "{setting:?}");
            assert!((least..=most).contains(&default), "{setting:?}");
            assert!(step > 0.0, "{setting:?}");
        }
    }
}

#[test]
fn the_defaults_are_the_websites_where_it_has_one() {
    let values = Values::new();
    assert_eq!(
        values.choice(Setting::PageSize),
        Some(Choice::SameAsPicture)
    );
    assert_eq!(values.choice(Setting::ScanPageSize), Some(Choice::A4));
    assert_eq!(values.choice(Setting::WhatToTake), Some(Choice::EveryPage));
    assert_eq!(values.choice(Setting::PictureFormat), Some(Choice::Jpg));
    assert_eq!(values.choice(Setting::Level), Some(Choice::Recommended));
    assert_eq!(values.choice(Setting::Look), Some(Choice::Colour));
    assert_eq!(values.choice(Setting::Place), Some(Choice::BottomRight));
    assert_eq!(values.choice(Setting::BoxColour), Some(Choice::Black));
    assert_eq!(values.choice(Setting::ReportAs), Some(Choice::HtmlReport));
    assert_eq!(values.choice(Setting::Fit), Some(Choice::AsInWorkbook));
    assert!(near(values.number(Setting::Resolution), 150.0));
    assert!(near(values.number(Setting::ComparisonResolution), 72.0));
    assert!(values.flag(Setting::Merge));
    assert!(values.flag(Setting::Crop));
    assert!(!values.flag(Setting::Grid));
    assert_eq!(values.text(Setting::SignPages).as_deref(), Some("last"));
    assert_eq!(values.text(Setting::Languages).as_deref(), Some("eng"));
    assert_eq!(values.text(Setting::Pages), None);
}

#[test]
fn a_setting_the_website_gives_no_size_to_keeps_the_scan_default() {
    assert!(near(Values::new().number(Setting::ScanQuality), 80.0));
    assert!(near(Values::new().number(Setting::PictureQuality), 85.0));
}

#[test]
fn what_was_set_wins_over_the_default_and_can_be_taken_back() {
    let mut values = Values::new();
    values.set(Setting::Resolution, Value::Number(300.0));
    values.set(Setting::Merge, Value::Flag(false));
    assert!(near(values.number(Setting::Resolution), 300.0));
    assert!(!values.flag(Setting::Merge));
    assert!(values.is_set(Setting::Merge));
    assert_eq!(values.unset(Setting::Merge), Some(Value::Flag(false)));
    assert!(values.flag(Setting::Merge));
    assert!(!values.is_set(Setting::Merge));
}

#[test]
fn a_value_of_the_wrong_kind_or_out_of_range_is_a_fault() {
    let fault = |setting: Setting, value: Value| {
        Values::new().with(setting, value).faults(
            Tool::ALL
                .into_iter()
                .find(|t| t.settings().contains(&setting))
                .unwrap(),
        )
    };
    let low = fault(Setting::Resolution, Value::Number(5.0));
    assert_eq!(low.len(), 1);
    assert!(matches!(low[0].why, Why::OutOfRange { .. }));
    assert_eq!(low[0].to_string(), "dpi: 5 is not between 10 and 1200");
    assert_eq!(
        fault(Setting::Resolution, Value::Number(f64::NAN))[0].why,
        Why::NotANumber
    );
    assert_eq!(
        fault(Setting::Pages, Value::Number(1.0))[0].why,
        Why::WrongKind
    );
    assert_eq!(
        fault(Setting::PageSize, Value::Choice(Choice::A3))[0].why,
        Why::NotOneOf
    );
    assert_eq!(
        fault(Setting::Forbid, Value::Choices(vec![Choice::Jpg]))[0].why,
        Why::NotOneOf
    );
    assert_eq!(
        fault(
            Setting::Areas,
            Value::Areas(vec![Area {
                page: 0,
                x0: 0.0,
                y0: 0.0,
                x1: 1.0,
                y1: 1.0
            }])
        )[0]
        .why,
        Why::NoSuchPage
    );
    assert_eq!(
        fault(
            Setting::Drawing,
            Value::Strokes(vec![vec![(0.0, f64::INFINITY)]])
        )[0]
        .why,
        Why::BadPoint
    );
    assert!(fault(Setting::Resolution, Value::Number(150.0)).is_empty());
    assert!(fault(Setting::Resolution, Value::Number(10.0)).is_empty());
    assert!(fault(Setting::Resolution, Value::Number(1200.0)).is_empty());
}

#[test]
fn a_value_for_a_setting_the_tool_does_not_have_is_not_a_fault() {
    let values = Values::new().with(Setting::Resolution, Value::Number(5.0));
    assert!(values.faults(Tool::PdfToWord).is_empty());
    assert_eq!(values.faults(Tool::PdfToImage).len(), 1);
}

#[test]
fn a_password_never_shows_when_the_values_are_printed() {
    let values = Values::new()
        .with(Setting::Password, Value::Secret("hunter2".into()))
        .with(
            Setting::SignaturePicture,
            Value::File {
                name: "me.png".into(),
                bytes: vec![0; 5000],
            },
        );
    let shown = format!("{values:?}");
    assert!(!shown.contains("hunter2"), "{shown}");
    assert!(shown.contains("me.png"));
    assert!(shown.contains("5000"));
    assert!(!shown.contains("0, 0, 0"));
}

#[test]
fn a_tool_takes_the_files_whose_names_its_kinds_list() {
    assert!(Tool::PdfToWord.takes("Report.PDF"));
    assert!(!Tool::PdfToWord.takes("report.docx"));
    assert!(Tool::WordToPdf.takes("letter.docx"));
    assert!(Tool::ExcelToPdf.takes("book.xlsx"));
    assert!(Tool::PowerPointToPdf.takes("deck.pptx"));
    assert!(Tool::ImageToPdf.takes("photo.jpeg"));
    assert!(Tool::ImageToPdf.takes("scan.PNG"));
    assert!(Tool::ScanToPdf.takes("page.jpg"));
    assert!(!Tool::ScanToPdf.takes("style.css"));
    assert!(Tool::HtmlToPdf.takes("page.htm"));
    assert!(Tool::HtmlToPdf.takes("style.css"));
    assert!(Tool::HtmlToPdf.takes("logo.svg"));
    assert!(!Tool::HtmlToPdf.takes("notes.txt"));
    assert!(!Tool::Compress.takes("noextension"));
}

#[test]
fn a_file_name_says_which_kind_it_is() {
    assert_eq!(Kind::of_name("a.pdf"), Some(Kind::Pdf));
    assert_eq!(Kind::of_name("a.docx"), Some(Kind::Word));
    assert_eq!(Kind::of_name("a.xlsx"), Some(Kind::Excel));
    assert_eq!(Kind::of_name("a.pptx"), Some(Kind::PowerPoint));
    assert_eq!(Kind::of_name("a.xhtml"), Some(Kind::Html));
    assert_eq!(Kind::of_name("a.gif"), Some(Kind::Picture));
    assert_eq!(Kind::of_name("a.css"), Some(Kind::PageAsset));
    assert_eq!(Kind::of_name("a.exe"), None);
}

#[test]
fn only_the_tools_that_take_a_pdf_start_from_one() {
    let from_pdf: Vec<Tool> = Tool::ALL
        .into_iter()
        .filter(|tool| tool.starts_from_a_pdf())
        .collect();
    assert_eq!(from_pdf.len(), 16);
    for tool in [
        Tool::WordToPdf,
        Tool::ExcelToPdf,
        Tool::PowerPointToPdf,
        Tool::ImageToPdf,
        Tool::ScanToPdf,
        Tool::HtmlToPdf,
    ] {
        assert!(!tool.starts_from_a_pdf(), "{tool:?}");
    }
}

#[test]
fn the_number_of_files_a_tool_takes_follows_what_it_does() {
    let many = Inputs {
        least: 1,
        most: None,
    };
    assert_eq!(Tool::PdfToWord.inputs(), many);
    assert_eq!(Tool::HtmlToPdf.inputs(), many);
    assert_eq!(Tool::ImageToPdf.inputs(), many);
    assert!(Tool::Compare.inputs().allows(2));
    assert!(!Tool::Compare.inputs().allows(1));
    assert!(!Tool::Compare.inputs().allows(3));
    assert!(Tool::Sign.inputs().allows(1));
    assert!(!Tool::Sign.inputs().allows(2));
    assert!(!Tool::Sign.inputs().allows(0));
    assert!(Tool::Ocr.inputs().allows(1));
    assert!(many.allows(40));
}

#[test]
fn what_a_tool_makes_names_the_extension_of_its_files() {
    assert_eq!(Tool::PdfToWord.output(), Made::Word);
    assert_eq!(Tool::PdfToWord.output().extension(), "docx");
    assert_eq!(Tool::PdfToExcel.output().extension(), "xlsx");
    assert_eq!(Tool::PdfToPowerPoint.output().extension(), "pptx");
    assert_eq!(Tool::PdfToImage.output(), Made::Pictures);
    assert_eq!(Tool::PdfToMarkdown.output().extension(), "md");
    assert_eq!(Tool::PdfToText.output().extension(), "txt");
    assert_eq!(Tool::PdfToHtml.output().extension(), "html");
    assert_eq!(Tool::Compare.output(), Made::Report);
    for tool in [Tool::Sign, Tool::Compress, Tool::WordToPdf, Tool::Ocr] {
        assert_eq!(tool.output().extension(), "pdf");
    }
}

#[test]
fn each_tool_names_the_setting_that_carries_the_password_of_its_file() {
    assert_eq!(Tool::PdfToWord.file_password(), Some(Setting::Password));
    assert_eq!(Tool::Unlock.file_password(), Some(Setting::Password));
    assert_eq!(Tool::Protect.file_password(), Some(Setting::FilePassword));
    assert_eq!(Tool::WordToPdf.file_password(), None);
    assert_eq!(Tool::ImageToPdf.file_password(), None);
    assert_eq!(Tool::Compare.file_password(), Some(Setting::Password));
}

#[test]
fn the_three_ways_of_signing_wait_on_the_choice_of_how() {
    assert_eq!(
        Setting::Drawing.when(),
        Some((Setting::Signature, Choice::Drawn))
    );
    assert_eq!(
        Setting::TypedName.when(),
        Some((Setting::Signature, Choice::Typed))
    );
    assert_eq!(
        Setting::SignaturePicture.when(),
        Some((Setting::Signature, Choice::FromPicture))
    );
    assert_eq!(
        Setting::Resolution.when(),
        Some((Setting::WhatToTake, Choice::EveryPage))
    );
}

#[test]
fn the_settings_the_website_shows_are_essential_and_the_extras_are_not() {
    for setting in [
        Setting::Pages,
        Setting::Level,
        Setting::PageSize,
        Setting::Merge,
        Setting::Search,
        Setting::NewPassword,
        Setting::Forbid,
        Setting::ReportAs,
    ] {
        assert!(setting.essential(), "{setting:?}");
    }
    for setting in [
        Setting::Password,
        Setting::OwnerPassword,
        Setting::Areas,
        Setting::ScanQuality,
        Setting::Flatten,
    ] {
        assert!(!setting.essential(), "{setting:?}");
    }
}

#[test]
fn a_choice_is_found_again_from_the_text_the_engine_uses() {
    assert_eq!(Setting::PageSize.choice_named("A4"), Some(Choice::A4));
    assert_eq!(
        Setting::PageSize.choice_named(" fit "),
        Some(Choice::SameAsPicture)
    );
    assert_eq!(Setting::PageSize.choice_named("legal"), None);
    assert_eq!(Setting::Fit.choice_named(""), Some(Choice::AsInWorkbook));
    assert_eq!(Setting::Forbid.choice_named("copy"), Some(Choice::Copy));
    assert_eq!(Setting::Pages.choice_named("1"), None);
}

#[test]
fn the_engine_values_of_the_website_choices_are_the_ones_the_tools_read() {
    let wanted = [
        (Choice::EveryPage, "pages"),
        (Choice::PicturesInside, "extract"),
        (Choice::SameAsPicture, "fit"),
        (Choice::BlackAndWhite, "bw"),
        (Choice::AsTaken, "original"),
        (Choice::ColumnsOnOnePage, "width"),
        (Choice::SheetOnOnePage, "page"),
        (Choice::BottomCentre, "bottom-centre"),
        (Choice::Black, "0,0,0"),
        (Choice::White, "1,1,1"),
        (Choice::TextReport, "txt"),
        (Choice::PrintHighQuality, "print-high"),
        (Choice::FillForms, "forms"),
    ];
    for (choice, value) in wanted {
        assert_eq!(choice.value(), value, "{choice:?}");
    }
}
