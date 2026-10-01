use std::borrow::Cow;

pub use pdf_convert::placing::{Placed, named_after, placed, stem_of, unused};
use pdf_convert::{Choice, Group, Kind, Setting, Stroke, Tool, Value, Values};

use crate::wording::{Lang, Tools};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Missing {
    AFile,
    TwoFiles,
    TheNewerFile,
    TheWebPage,
    WordsToRemove,
    APassword,
    PasswordsDiffer,
    TheDrawing,
    TheName,
    ThePicture,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trouble {
    NotAPdf,
    Damaged,
    NotAWordDocument,
    NotAWorkbook,
    NotAPresentation,
    PageNotThere,
    NoMatches,
    NothingToRedact,
    NoPictures,
    PictureUnreadable,
    Unexpected,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Manner {
    Cards,
    Segments,
    Menu,
    Ticks,
    Switch,
    Presets,
    Slider,
    Amount,
    Words,
    Secret,
    Pages,
    SignPages,
    Picture,
    Pad,
    Typeface,
    Hidden,
}

pub const PICTURE_PRESETS: [f64; 3] = [72.0, 150.0, 300.0];

pub const PAD_ASPECT: f32 = 0.34;

pub fn ready(tool: Tool, names: &[&str], values: &Values, repeated: &str) -> Result<(), Missing> {
    if !tool.inputs().allows(names.len()) {
        return Err(match (tool, names.len()) {
            (Tool::Compare, 1) => Missing::TheNewerFile,
            (Tool::Compare, _) => Missing::TwoFiles,
            _ => Missing::AFile,
        });
    }
    match tool {
        Tool::HtmlToPdf => {
            if names.iter().any(|name| Kind::Html.matches(name)) {
                Ok(())
            } else {
                Err(Missing::TheWebPage)
            }
        }
        Tool::Redact => {
            let words = terms_of(values).iter().any(|word| !word.is_empty());
            if words || values.flag(Setting::UseMarks) {
                Ok(())
            } else {
                Err(Missing::WordsToRemove)
            }
        }
        Tool::Protect => {
            let password = values
                .text(Setting::NewPassword)
                .unwrap_or(Cow::Borrowed(""));
            if password.is_empty() {
                Err(Missing::APassword)
            } else if *password != *repeated {
                Err(Missing::PasswordsDiffer)
            } else {
                Ok(())
            }
        }
        Tool::Sign => signature_ready(values),
        _ => Ok(()),
    }
}

fn signature_ready(values: &Values) -> Result<(), Missing> {
    match values.choice(Setting::Signature).unwrap_or(Choice::Drawn) {
        Choice::Typed => {
            let named = values
                .text(Setting::TypedName)
                .is_some_and(|name| !name.trim().is_empty());
            if named { Ok(()) } else { Err(Missing::TheName) }
        }
        Choice::FromPicture => match values.explicit(Setting::SignaturePicture) {
            Some(Value::File { bytes, .. }) if !bytes.is_empty() => Ok(()),
            _ => Err(Missing::ThePicture),
        },
        _ => match values.explicit(Setting::Drawing) {
            Some(Value::Strokes(strokes)) if strokes.iter().any(|stroke| stroke.len() > 1) => {
                Ok(())
            }
            _ => Err(Missing::TheDrawing),
        },
    }
}

fn terms_of(values: &Values) -> Vec<String> {
    match values.explicit(Setting::Search) {
        Some(Value::Terms(terms)) => terms.clone(),
        _ => Vec::new(),
    }
}

#[must_use]
pub fn terms_in(text: &str) -> Vec<String> {
    text.split([',', '\n'])
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

#[must_use]
pub fn shown(setting: Setting, values: &Values) -> bool {
    if manner(setting) == Manner::Hidden {
        return false;
    }
    match setting.when() {
        None => true,
        Some((on, choice)) => values.choice(on) == Some(choice),
    }
}

#[must_use]
pub fn arranged(tool: Tool, values: &Values) -> (Vec<Setting>, Vec<Setting>) {
    let all: Vec<Setting> = tool
        .settings()
        .iter()
        .copied()
        .filter(|setting| shown(*setting, values))
        .collect();
    let (first, more): (Vec<Setting>, Vec<Setting>) =
        all.into_iter().partition(|setting| setting.essential());
    (first, more)
}

#[must_use]
pub fn manner(setting: Setting) -> Manner {
    match setting {
        Setting::WhatToTake | Setting::Fit | Setting::Level | Setting::ReportAs => Manner::Cards,
        Setting::Paper | Setting::Place => Manner::Menu,
        Setting::Forbid => Manner::Ticks,
        Setting::Resolution => Manner::Presets,
        Setting::PictureQuality | Setting::ScanQuality => Manner::Slider,
        Setting::SignatureWidth | Setting::ComparisonResolution => Manner::Amount,
        Setting::Pages => Manner::Pages,
        Setting::SignPages => Manner::SignPages,
        Setting::SignaturePicture => Manner::Picture,
        Setting::Drawing => Manner::Pad,
        Setting::TypeFace => Manner::Typeface,
        Setting::Areas | Setting::Languages => Manner::Hidden,
        Setting::Password
        | Setting::SecondPassword
        | Setting::NewPassword
        | Setting::OwnerPassword
        | Setting::FilePassword => Manner::Secret,
        Setting::TypedName | Setting::SignatureAt | Setting::Search => Manner::Words,
        Setting::Grid
        | Setting::HiddenSlides
        | Setting::Merge
        | Setting::Crop
        | Setting::ReadPagesWithText
        | Setting::Flatten
        | Setting::MatchCase
        | Setting::UseMarks
        | Setting::LeavePicturesOut => Manner::Switch,
        Setting::PictureFormat
        | Setting::SheetOrientation
        | Setting::PageSize
        | Setting::ScanPageSize
        | Setting::Orientation
        | Setting::Margin
        | Setting::Look
        | Setting::Signature
        | Setting::BoxColour => Manner::Segments,
    }
}

#[must_use]
pub fn defaults_to_the_document(tool: Tool, has_document: bool) -> bool {
    has_document && tool.starts_from_a_pdf() && tool != Tool::Ocr
}

#[must_use]
pub fn takes_several(tool: Tool) -> bool {
    tool.inputs().most != Some(1)
}

#[must_use]
pub fn order_matters(tool: Tool) -> bool {
    matches!(tool, Tool::ImageToPdf | Tool::ScanToPdf)
}

#[must_use]
pub fn reports_each_file(tool: Tool) -> bool {
    !matches!(
        tool,
        Tool::PdfToImage | Tool::ImageToPdf | Tool::ScanToPdf | Tool::HtmlToPdf | Tool::Compare
    )
}

#[must_use]
pub fn room_for_more(tool: Tool, held: usize) -> usize {
    tool.inputs()
        .most
        .map_or(usize::MAX, |most| most.saturating_sub(held))
}

#[must_use]
pub fn accept(tool: Tool, names: &[&str], room: usize) -> Vec<usize> {
    names
        .iter()
        .enumerate()
        .filter(|(_, name)| tool.takes(name))
        .map(|(at, _)| at)
        .take(room)
        .collect()
}

#[must_use]
pub fn extensions(tool: Tool) -> &'static [&'static str] {
    match tool {
        Tool::WordToPdf => &["docx"],
        Tool::ExcelToPdf => &["xlsx"],
        Tool::PowerPointToPdf => &["pptx"],
        Tool::ImageToPdf | Tool::ScanToPdf => &["jpg", "jpeg", "png", "gif"],
        Tool::HtmlToPdf => &[
            "html", "htm", "xhtml", "css", "svg", "jpg", "jpeg", "png", "gif",
        ],
        Tool::PdfToWord
        | Tool::PdfToExcel
        | Tool::PdfToPowerPoint
        | Tool::PdfToImage
        | Tool::PdfToHtml
        | Tool::PdfToMarkdown
        | Tool::PdfToText
        | Tool::PdfToPdfA
        | Tool::Compress
        | Tool::Repair
        | Tool::Ocr
        | Tool::Unlock
        | Tool::Sign
        | Tool::Redact
        | Tool::Compare
        | Tool::Protect => &["pdf"],
    }
}

#[must_use]
pub fn opens_the_text_panel(tool: Tool) -> bool {
    tool == Tool::Ocr
}

#[must_use]
pub fn found(query: &str, group: Option<Group>, lang: Lang) -> Vec<Tool> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    Tool::ALL
        .into_iter()
        .filter(|tool| group.is_none_or(|group| tool.group() == group))
        .filter(|tool| {
            let name = Tools::Name(*tool).say(lang).to_lowercase();
            let blurb = Tools::Blurb(*tool).say(lang).to_lowercase();
            words
                .iter()
                .all(|word| name.contains(word.as_str()) || blurb.contains(word.as_str()))
        })
        .collect()
}

#[must_use]
pub fn sign_strokes(pad: &[Vec<(f32, f32)>]) -> Vec<Stroke> {
    let points = pad.iter().flatten();
    let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in points {
        let (x, y) = (f64::from(x), f64::from(y));
        left = left.min(x);
        right = right.max(x);
        top = top.min(y);
        bottom = bottom.max(y);
    }
    if left > right {
        return Vec::new();
    }
    let (across, down) = ((right - left).max(1.0), (bottom - top).max(1.0));
    let ratio = (down / across).clamp(0.01, 4.0);
    let (reach_x, reach_y) = if ratio <= 1.0 {
        (1.0, ratio.sqrt())
    } else {
        (1.0 / ratio.sqrt(), 1.0)
    };
    pad.iter()
        .filter(|stroke| !stroke.is_empty())
        .map(|stroke| {
            stroke
                .iter()
                .map(|&(x, y)| {
                    (
                        (f64::from(x) - left) / across * reach_x,
                        (bottom - f64::from(y)) / down * reach_y,
                    )
                })
                .collect()
        })
        .collect()
}

#[must_use]
pub fn trouble_in(words: &str) -> Trouble {
    const TABLE: [(&[&str], Trouble); 11] = [
        (
            &[
                "pdf header does not begin at byte zero",
                "cannot be read as a pdf",
                "not a pdf",
                "no document catalogue",
            ],
            Trouble::NotAPdf,
        ),
        (&["not a word document"], Trouble::NotAWordDocument),
        (&["not an excel workbook"], Trouble::NotAWorkbook),
        (
            &["not a powerpoint presentation"],
            Trouble::NotAPresentation,
        ),
        (
            &[
                "was asked for and the file has",
                "is not a page",
                "pages count from 1",
            ],
            Trouble::PageNotThere,
        ),
        (&["searched words were not found"], Trouble::NoMatches),
        (&["nothing to redact"], Trouble::NothingToRedact),
        (&["no pictures were found"], Trouble::NoPictures),
        (
            &[
                "jpeg marker",
                "jpeg segment",
                "png chunk",
                "png header",
                "not a picture",
                "pixels could not be decoded",
                "not a png or a jpeg",
            ],
            Trouble::PictureUnreadable,
        ),
        (
            &[
                "the file is damaged",
                "cross-reference",
                "object stream",
                "trailer",
                "xref",
                "could not be read",
                "central directory",
                "not a zip archive",
            ],
            Trouble::Damaged,
        ),
        (&["panicked", "stopped unexpectedly"], Trouble::Unexpected),
    ];
    let lowered = words.to_lowercase();
    TABLE
        .iter()
        .find(|(phrases, _)| phrases.iter().any(|phrase| lowered.contains(phrase)))
        .map_or(Trouble::Other, |(_, trouble)| *trouble)
}

#[cfg(test)]
mod tests;
