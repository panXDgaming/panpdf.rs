use std::collections::BTreeSet;

use pdf_convert::{Choice, Group, Kind, Made, Setting, Tool};

use super::{Lang, Tools};
use crate::tools::{Missing, Trouble};

fn said(tools: &Tools) -> String {
    tools.say(Lang::English)
}

#[test]
fn every_tool_has_a_name_a_line_a_button_and_a_menu_entry() {
    let mut names = BTreeSet::new();
    for tool in Tool::ALL {
        for sentence in [
            Tools::Name(tool),
            Tools::Blurb(tool),
            Tools::Go(tool),
            Tools::Entry(tool),
        ] {
            let text = said(&sentence);
            assert!(!text.trim().is_empty(), "{sentence:?} says nothing");
        }
        assert!(
            names.insert(said(&Tools::Name(tool))),
            "two tools are called {}",
            said(&Tools::Name(tool))
        );
        assert_eq!(
            said(&Tools::Entry(tool)),
            format!("{}\u{2026}", said(&Tools::Name(tool)))
        );
    }
    assert_eq!(names.len(), 22);
}

#[test]
fn every_group_has_a_title_and_a_short_chip() {
    for group in Group::ALL {
        let title = said(&Tools::GroupName(group));
        let chip = said(&Tools::GroupChip(group));
        assert!(!title.is_empty() && !chip.is_empty(), "{group:?}");
        assert!(
            chip.len() <= title.len(),
            "{chip} is not shorter than {title}"
        );
    }
}

#[test]
fn every_setting_has_a_label_and_every_hint_that_exists_says_something() {
    for setting in Setting::ALL {
        let label = said(&Tools::Label(setting));
        assert!(!label.trim().is_empty(), "{setting:?} has no label");
        for sentence in [
            Tools::Help(setting),
            Tools::Hint(setting),
            Tools::Unit(setting),
        ] {
            if let Some(text) = sentence.said(Lang::English) {
                assert!(!text.trim().is_empty(), "{sentence:?}");
            }
        }
    }
}

#[test]
fn every_choice_every_setting_offers_is_named() {
    for choice in Choice::ALL {
        let text = said(&Tools::Choice(choice));
        assert!(!text.trim().is_empty(), "{choice:?} has no name");
    }
    for setting in Setting::ALL {
        for choice in setting.choices() {
            assert!(
                !said(&Tools::Choice(*choice)).is_empty(),
                "{setting:?} offers {choice:?} unnamed"
            );
        }
    }
}

#[test]
fn a_setting_that_shows_its_choices_as_cards_explains_each_of_them() {
    for setting in [
        Setting::WhatToTake,
        Setting::Fit,
        Setting::Level,
        Setting::ReportAs,
    ] {
        for choice in setting.choices() {
            let explained = Tools::Explains(*choice).said(Lang::English);
            assert!(
                explained.is_some_and(|text| text.ends_with('.')),
                "{setting:?} does not explain {choice:?}"
            );
        }
    }
    for choice in [Choice::Jpg, Choice::A4, Choice::Portrait] {
        assert_eq!(Tools::Explains(choice).said(Lang::English), None);
    }
}

#[test]
fn what_a_tool_adds_to_its_page_is_said_only_when_it_has_something_to_add() {
    let with_a_note: Vec<Tool> = Tool::ALL
        .into_iter()
        .filter(|tool| Tools::Note(*tool).said(Lang::English).is_some())
        .collect();
    assert!(with_a_note.contains(&Tool::PdfToPdfA));
    assert!(with_a_note.contains(&Tool::HtmlToPdf));
    assert!(!with_a_note.contains(&Tool::PdfToWord));
    for tool in with_a_note {
        let note = said(&Tools::Note(tool));
        assert!(note.ends_with('.'), "{note}");
    }
}

#[test]
fn every_result_names_what_was_made() {
    let every = [
        Made::Word,
        Made::Excel,
        Made::PowerPoint,
        Made::Pictures,
        Made::Html,
        Made::Markdown,
        Made::Text,
        Made::Pdf,
        Made::Report,
    ];
    let heads: BTreeSet<String> = every
        .iter()
        .map(|made| said(&Tools::Ready(*made)))
        .collect();
    assert_eq!(heads.len(), every.len(), "{heads:?}");
    for kind in [
        Kind::Pdf,
        Kind::Word,
        Kind::Excel,
        Kind::PowerPoint,
        Kind::Html,
        Kind::Picture,
        Kind::PageAsset,
    ] {
        assert!(said(&Tools::Choose(kind)).starts_with("Choose"));
    }
}

#[test]
fn every_failure_and_every_missing_piece_has_its_own_plain_sentence() {
    let troubles = [
        Trouble::NotAPdf,
        Trouble::Damaged,
        Trouble::NotAWordDocument,
        Trouble::NotAWorkbook,
        Trouble::NotAPresentation,
        Trouble::PageNotThere,
        Trouble::NoMatches,
        Trouble::NothingToRedact,
        Trouble::NoPictures,
        Trouble::PictureUnreadable,
        Trouble::Unexpected,
        Trouble::Other,
    ];
    let said_troubles: BTreeSet<String> = troubles
        .iter()
        .map(|trouble| said(&Tools::Trouble(*trouble)))
        .collect();
    assert_eq!(said_troubles.len(), troubles.len());
    let missing = [
        Missing::AFile,
        Missing::TwoFiles,
        Missing::TheNewerFile,
        Missing::TheWebPage,
        Missing::WordsToRemove,
        Missing::APassword,
        Missing::PasswordsDiffer,
        Missing::TheDrawing,
        Missing::TheName,
        Missing::ThePicture,
    ];
    let said_missing: BTreeSet<String> = missing
        .iter()
        .map(|missing| said(&Tools::Missing(*missing)))
        .collect();
    assert_eq!(said_missing.len(), missing.len());
}

#[test]
fn how_far_a_job_has_got_reads_as_the_website_reads_it() {
    assert_eq!(said(&Tools::FileOf { index: 0, of: 3 }), "File 1 of 3");
    assert_eq!(said(&Tools::FileOf { index: 2, of: 3 }), "File 3 of 3");
    assert_eq!(said(&Tools::PageOf { done: 4, total: 10 }), "Page 4 of 10");
    assert_eq!(
        said(&Tools::PictureOf { done: 1, total: 2 }),
        "Picture 1 of 2"
    );
    assert_eq!(said(&Tools::Percent(40)), "40%");
}

#[test]
fn the_time_a_job_took_is_counted_in_tenths_of_a_second() {
    assert_eq!(
        said(&Tools::DoneIn {
            files: 1,
            tenths: 23
        }),
        "1 file, done on this computer in 2.3 s"
    );
    assert_eq!(
        said(&Tools::DoneIn {
            files: 3,
            tenths: 5
        }),
        "3 files, done on this computer in 0.5 s"
    );
}

#[test]
fn a_file_is_described_by_its_pages_its_size_and_what_it_holds_that_is_unsaved() {
    let plain = Tools::Detail {
        pages: Some(42),
        bytes: 1_258_291,
        unsaved: false,
    };
    assert_eq!(said(&plain), "42 pages \u{00b7} 1.2 MB");
    let changed = Tools::Detail {
        pages: Some(1),
        bytes: 900,
        unsaved: true,
    };
    assert_eq!(
        said(&changed),
        "1 page \u{00b7} 900 B \u{00b7} includes your unsaved changes"
    );
    let picture = Tools::Detail {
        pages: None,
        bytes: 2048,
        unsaved: false,
    };
    assert_eq!(said(&picture), "2.0 KB");
}

#[test]
fn a_compressed_file_shows_what_it_was_and_what_it_is() {
    assert_eq!(
        said(&Tools::Shrank {
            from: 72_499,
            to: 53_965
        }),
        "70.8 KB \u{2192} 52.7 KB"
    );
}

#[test]
fn no_sentence_has_a_stray_space_or_an_ascii_ellipsis() {
    let mut sentences = vec![
        Tools::HomeTile,
        Tools::HomeTileHelp,
        Tools::AllTools,
        Tools::RoomPromise,
        Tools::NothingUploaded,
        Tools::StaysHere,
        Tools::NeedsPassword,
        Tools::DropHint,
        Tools::SignHere,
    ];
    for tool in Tool::ALL {
        sentences.extend([
            Tools::Name(tool),
            Tools::Blurb(tool),
            Tools::Go(tool),
            Tools::Note(tool),
        ]);
    }
    for setting in Setting::ALL {
        sentences.extend([
            Tools::Label(setting),
            Tools::Help(setting),
            Tools::Hint(setting),
        ]);
    }
    for choice in Choice::ALL {
        sentences.extend([Tools::Choice(choice), Tools::Explains(choice)]);
    }
    for sentence in sentences {
        let text = said(&sentence);
        assert_eq!(text, text.trim(), "{sentence:?}");
        assert!(!text.contains("  "), "{sentence:?}: {text}");
        assert!(!text.contains("..."), "{sentence:?}: {text}");
        assert!(!text.contains(" -- "), "{sentence:?}: {text}");
    }
}
