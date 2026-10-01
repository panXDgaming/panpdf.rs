use crate::catalogue::choice::Choice;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SettingKind {
    Choice {
        choices: &'static [Choice],
        default: Choice,
    },
    Several {
        choices: &'static [Choice],
    },
    Number {
        least: f64,
        most: f64,
        default: f64,
        step: f64,
    },
    Flag {
        default: bool,
    },
    Text {
        default: &'static str,
    },
    Secret,
    Pages {
        default: &'static str,
    },
    Picture,
    Strokes,
    Terms,
    Areas,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Setting {
    Pages,
    Password,
    SecondPassword,
    WhatToTake,
    PictureFormat,
    Resolution,
    PictureQuality,
    Fit,
    Paper,
    Grid,
    SheetOrientation,
    HiddenSlides,
    PageSize,
    ScanPageSize,
    Orientation,
    Margin,
    Merge,
    Crop,
    Look,
    ScanQuality,
    Level,
    Languages,
    ReadPagesWithText,
    Signature,
    Drawing,
    TypedName,
    SignaturePicture,
    SignPages,
    Place,
    SignatureAt,
    SignatureWidth,
    TypeFace,
    Flatten,
    Search,
    MatchCase,
    UseMarks,
    BoxColour,
    Areas,
    ReportAs,
    LeavePicturesOut,
    ComparisonResolution,
    NewPassword,
    Forbid,
    OwnerPassword,
    FilePassword,
}

const TAKE: &[Choice] = &[Choice::EveryPage, Choice::PicturesInside];
const PICTURE_FORMATS: &[Choice] = &[Choice::Jpg, Choice::Png];
const FITS: &[Choice] = &[
    Choice::AsInWorkbook,
    Choice::ColumnsOnOnePage,
    Choice::SheetOnOnePage,
    Choice::ActualSize,
];
const PAPERS: &[Choice] = &[
    Choice::AsInWorkbook,
    Choice::A4,
    Choice::Letter,
    Choice::A3,
    Choice::A5,
    Choice::Legal,
];
const SHEET_TURNS: &[Choice] = &[Choice::AsInWorkbook, Choice::Portrait, Choice::Landscape];
const PICTURE_PAGES: &[Choice] = &[Choice::SameAsPicture, Choice::A4, Choice::Letter];
const SCAN_PAGES: &[Choice] = &[Choice::A4, Choice::Letter, Choice::SameAsPicture];
const TURNS: &[Choice] = &[Choice::AutoOrientation, Choice::Portrait, Choice::Landscape];
const MARGINS: &[Choice] = &[Choice::NoMargin, Choice::SmallMargin, Choice::BigMargin];
const LOOKS: &[Choice] = &[
    Choice::Colour,
    Choice::Grey,
    Choice::BlackAndWhite,
    Choice::AsTaken,
];
const LEVELS: &[Choice] = &[Choice::Extreme, Choice::Recommended, Choice::Low];
const SIGNATURES: &[Choice] = &[Choice::Drawn, Choice::Typed, Choice::FromPicture];
const PLACES: &[Choice] = &[
    Choice::BottomRight,
    Choice::BottomLeft,
    Choice::BottomCentre,
    Choice::TopRight,
    Choice::TopLeft,
    Choice::Centre,
];
const BOX_COLOURS: &[Choice] = &[Choice::Black, Choice::White];
const REPORTS: &[Choice] = &[Choice::HtmlReport, Choice::TextReport];
const FORBIDDEN: &[Choice] = &[
    Choice::Print,
    Choice::PrintHighQuality,
    Choice::Copy,
    Choice::Modify,
    Choice::Annotate,
    Choice::FillForms,
    Choice::Assemble,
];

impl Setting {
    pub const ALL: [Self; 45] = [
        Self::Pages,
        Self::Password,
        Self::SecondPassword,
        Self::WhatToTake,
        Self::PictureFormat,
        Self::Resolution,
        Self::PictureQuality,
        Self::Fit,
        Self::Paper,
        Self::Grid,
        Self::SheetOrientation,
        Self::HiddenSlides,
        Self::PageSize,
        Self::ScanPageSize,
        Self::Orientation,
        Self::Margin,
        Self::Merge,
        Self::Crop,
        Self::Look,
        Self::ScanQuality,
        Self::Level,
        Self::Languages,
        Self::ReadPagesWithText,
        Self::Signature,
        Self::Drawing,
        Self::TypedName,
        Self::SignaturePicture,
        Self::SignPages,
        Self::Place,
        Self::SignatureAt,
        Self::SignatureWidth,
        Self::TypeFace,
        Self::Flatten,
        Self::Search,
        Self::MatchCase,
        Self::UseMarks,
        Self::BoxColour,
        Self::Areas,
        Self::ReportAs,
        Self::LeavePicturesOut,
        Self::ComparisonResolution,
        Self::NewPassword,
        Self::Forbid,
        Self::OwnerPassword,
        Self::FilePassword,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Pages | Self::SignPages => "pages",
            Self::Password | Self::NewPassword => "password",
            Self::SecondPassword => "password2",
            Self::WhatToTake => "mode",
            Self::PictureFormat | Self::ReportAs => "format",
            Self::Resolution | Self::ComparisonResolution => "dpi",
            Self::PictureQuality | Self::ScanQuality => "quality",
            Self::Fit => "fit",
            Self::Paper => "paper",
            Self::Grid => "grid",
            Self::SheetOrientation | Self::Orientation => "orientation",
            Self::HiddenSlides => "hidden",
            Self::PageSize | Self::ScanPageSize => "size",
            Self::Margin => "margin",
            Self::Merge => "merge",
            Self::Crop => "crop",
            Self::Look => "look",
            Self::Level => "level",
            Self::Languages => "languages",
            Self::ReadPagesWithText => "force",
            Self::Signature => "how",
            Self::Drawing => "draw",
            Self::TypedName => "text",
            Self::SignaturePicture => "image",
            Self::Place => "position",
            Self::SignatureAt => "at",
            Self::SignatureWidth => "width",
            Self::TypeFace => "font",
            Self::Flatten => "flatten",
            Self::Search => "search",
            Self::MatchCase => "case",
            Self::UseMarks => "annotations",
            Self::BoxColour => "color",
            Self::Areas => "areas",
            Self::LeavePicturesOut => "no-pictures",
            Self::Forbid => "deny",
            Self::OwnerPassword => "owner-password",
            Self::FilePassword => "input-password",
        }
    }

    #[must_use]
    pub const fn kind(self) -> SettingKind {
        match self {
            Self::Pages => SettingKind::Pages { default: "" },
            Self::SignPages => SettingKind::Pages { default: "last" },
            Self::Password
            | Self::SecondPassword
            | Self::NewPassword
            | Self::OwnerPassword
            | Self::FilePassword => SettingKind::Secret,
            Self::WhatToTake => choice(TAKE, Choice::EveryPage),
            Self::PictureFormat => choice(PICTURE_FORMATS, Choice::Jpg),
            Self::Resolution => number(10.0, 1200.0, 150.0),
            Self::PictureQuality => number(1.0, 100.0, 85.0),
            Self::Fit => choice(FITS, Choice::AsInWorkbook),
            Self::Paper => choice(PAPERS, Choice::AsInWorkbook),
            Self::Grid
            | Self::ReadPagesWithText
            | Self::Flatten
            | Self::MatchCase
            | Self::UseMarks
            | Self::LeavePicturesOut
            | Self::HiddenSlides => SettingKind::Flag { default: false },
            Self::SheetOrientation => choice(SHEET_TURNS, Choice::AsInWorkbook),
            Self::PageSize => choice(PICTURE_PAGES, Choice::SameAsPicture),
            Self::ScanPageSize => choice(SCAN_PAGES, Choice::A4),
            Self::Orientation => choice(TURNS, Choice::AutoOrientation),
            Self::Margin => choice(MARGINS, Choice::NoMargin),
            Self::Merge | Self::Crop => SettingKind::Flag { default: true },
            Self::Look => choice(LOOKS, Choice::Colour),
            Self::ScanQuality => number(1.0, 100.0, 80.0),
            Self::Level => choice(LEVELS, Choice::Recommended),
            Self::Languages => SettingKind::Text { default: "eng" },
            Self::Signature => choice(SIGNATURES, Choice::Drawn),
            Self::Drawing => SettingKind::Strokes,
            Self::TypedName | Self::SignatureAt | Self::TypeFace => {
                SettingKind::Text { default: "" }
            }
            Self::SignaturePicture => SettingKind::Picture,
            Self::Place => choice(PLACES, Choice::BottomRight),
            Self::SignatureWidth => number(10.0, 1000.0, 160.0),
            Self::Search => SettingKind::Terms,
            Self::BoxColour => choice(BOX_COLOURS, Choice::Black),
            Self::Areas => SettingKind::Areas,
            Self::ReportAs => choice(REPORTS, Choice::HtmlReport),
            Self::ComparisonResolution => number(20.0, 300.0, 72.0),
            Self::Forbid => SettingKind::Several { choices: FORBIDDEN },
        }
    }

    #[must_use]
    pub const fn essential(self) -> bool {
        match self {
            Self::Pages
            | Self::WhatToTake
            | Self::PictureFormat
            | Self::Resolution
            | Self::Fit
            | Self::Paper
            | Self::Grid
            | Self::HiddenSlides
            | Self::PageSize
            | Self::ScanPageSize
            | Self::Orientation
            | Self::Margin
            | Self::Merge
            | Self::Crop
            | Self::Look
            | Self::Level
            | Self::Languages
            | Self::Signature
            | Self::Drawing
            | Self::TypedName
            | Self::SignaturePicture
            | Self::SignPages
            | Self::Place
            | Self::Search
            | Self::MatchCase
            | Self::UseMarks
            | Self::BoxColour
            | Self::ReportAs
            | Self::NewPassword
            | Self::Forbid => true,
            Self::Password
            | Self::SecondPassword
            | Self::PictureQuality
            | Self::SheetOrientation
            | Self::ScanQuality
            | Self::ReadPagesWithText
            | Self::SignatureAt
            | Self::SignatureWidth
            | Self::TypeFace
            | Self::Flatten
            | Self::Areas
            | Self::LeavePicturesOut
            | Self::ComparisonResolution
            | Self::OwnerPassword
            | Self::FilePassword => false,
        }
    }

    #[must_use]
    pub const fn when(self) -> Option<(Self, Choice)> {
        match self {
            Self::Resolution => Some((Self::WhatToTake, Choice::EveryPage)),
            Self::PictureQuality => Some((Self::PictureFormat, Choice::Jpg)),
            Self::Drawing => Some((Self::Signature, Choice::Drawn)),
            Self::TypedName => Some((Self::Signature, Choice::Typed)),
            Self::SignaturePicture => Some((Self::Signature, Choice::FromPicture)),
            Self::LeavePicturesOut | Self::ComparisonResolution => {
                Some((Self::ReportAs, Choice::HtmlReport))
            }
            _ => None,
        }
    }

    #[must_use]
    pub const fn choices(self) -> &'static [Choice] {
        match self.kind() {
            SettingKind::Choice { choices, .. } | SettingKind::Several { choices } => choices,
            _ => &[],
        }
    }

    #[must_use]
    pub fn choice_named(self, value: &str) -> Option<Choice> {
        self.choices()
            .iter()
            .copied()
            .find(|choice| choice.value().eq_ignore_ascii_case(value.trim()))
    }
}

const fn choice(choices: &'static [Choice], default: Choice) -> SettingKind {
    SettingKind::Choice { choices, default }
}

const fn number(least: f64, most: f64, default: f64) -> SettingKind {
    SettingKind::Number {
        least,
        most,
        default,
        step: 1.0,
    }
}
