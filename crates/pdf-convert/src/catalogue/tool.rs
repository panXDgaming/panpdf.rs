use crate::catalogue::setting::Setting;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tool {
    PdfToWord,
    PdfToExcel,
    PdfToPowerPoint,
    PdfToImage,
    PdfToHtml,
    PdfToMarkdown,
    PdfToText,
    PdfToPdfA,
    WordToPdf,
    ExcelToPdf,
    PowerPointToPdf,
    ImageToPdf,
    ScanToPdf,
    HtmlToPdf,
    Compress,
    Repair,
    Ocr,
    Unlock,
    Sign,
    Redact,
    Compare,
    Protect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    FromPdf,
    ToPdf,
    Optimize,
    Security,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Pdf,
    Word,
    Excel,
    PowerPoint,
    Html,
    Picture,
    PageAsset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Made {
    Word,
    Excel,
    PowerPoint,
    Pictures,
    Html,
    Markdown,
    Text,
    Pdf,
    Report,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inputs {
    pub least: usize,
    pub most: Option<usize>,
}

const ESSENTIAL_PAGES: &[Setting] = &[Setting::Pages, Setting::Password];
const PICTURES: &[Setting] = &[
    Setting::WhatToTake,
    Setting::PictureFormat,
    Setting::Resolution,
    Setting::Pages,
    Setting::PictureQuality,
    Setting::Password,
];
const PASSWORD_ONLY: &[Setting] = &[Setting::Password];
const WORKBOOK: &[Setting] = &[
    Setting::Fit,
    Setting::Paper,
    Setting::Grid,
    Setting::SheetOrientation,
];
const SLIDES: &[Setting] = &[Setting::HiddenSlides];
const PICTURES_TO_PDF: &[Setting] = &[
    Setting::PageSize,
    Setting::Orientation,
    Setting::Margin,
    Setting::Merge,
];
const SCAN: &[Setting] = &[
    Setting::Crop,
    Setting::Look,
    Setting::ScanPageSize,
    Setting::Orientation,
    Setting::Margin,
    Setting::ScanQuality,
];
const COMPRESS: &[Setting] = &[Setting::Level, Setting::Password];
const OCR: &[Setting] = &[
    Setting::Languages,
    Setting::Pages,
    Setting::ReadPagesWithText,
    Setting::Password,
];
const SIGN: &[Setting] = &[
    Setting::Signature,
    Setting::Drawing,
    Setting::TypedName,
    Setting::SignaturePicture,
    Setting::SignPages,
    Setting::Place,
    Setting::SignatureAt,
    Setting::SignatureWidth,
    Setting::TypeFace,
    Setting::Flatten,
    Setting::Password,
];
const REDACT: &[Setting] = &[
    Setting::Search,
    Setting::MatchCase,
    Setting::UseMarks,
    Setting::BoxColour,
    Setting::Areas,
    Setting::Pages,
    Setting::Password,
];
const COMPARE: &[Setting] = &[
    Setting::ReportAs,
    Setting::LeavePicturesOut,
    Setting::ComparisonResolution,
    Setting::Password,
    Setting::SecondPassword,
];
const PROTECT: &[Setting] = &[
    Setting::NewPassword,
    Setting::Forbid,
    Setting::OwnerPassword,
    Setting::FilePassword,
];

const PDF: &[Kind] = &[Kind::Pdf];
const NOTHING: &[Setting] = &[];

impl Tool {
    pub const ALL: [Self; 22] = [
        Self::PdfToWord,
        Self::PdfToExcel,
        Self::PdfToPowerPoint,
        Self::PdfToImage,
        Self::PdfToHtml,
        Self::PdfToMarkdown,
        Self::PdfToText,
        Self::PdfToPdfA,
        Self::WordToPdf,
        Self::ExcelToPdf,
        Self::PowerPointToPdf,
        Self::ImageToPdf,
        Self::ScanToPdf,
        Self::HtmlToPdf,
        Self::Compress,
        Self::Repair,
        Self::Ocr,
        Self::Unlock,
        Self::Sign,
        Self::Redact,
        Self::Compare,
        Self::Protect,
    ];

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::PdfToWord => "pdf-to-word",
            Self::PdfToExcel => "pdf-to-excel",
            Self::PdfToPowerPoint => "pdf-to-powerpoint",
            Self::PdfToImage => "pdf-to-jpg",
            Self::PdfToHtml => "pdf-to-html",
            Self::PdfToMarkdown => "pdf-to-markdown",
            Self::PdfToText => "pdf-to-text",
            Self::PdfToPdfA => "pdf-to-pdfa",
            Self::WordToPdf => "word-to-pdf",
            Self::ExcelToPdf => "excel-to-pdf",
            Self::PowerPointToPdf => "powerpoint-to-pdf",
            Self::ImageToPdf => "jpg-to-pdf",
            Self::ScanToPdf => "scan-to-pdf",
            Self::HtmlToPdf => "html-to-pdf",
            Self::Compress => "compress-pdf",
            Self::Repair => "repair-pdf",
            Self::Ocr => "ocr-pdf",
            Self::Unlock => "unlock-pdf",
            Self::Sign => "sign-pdf",
            Self::Redact => "redact-pdf",
            Self::Compare => "compare-pdf",
            Self::Protect => "protect-pdf",
        }
    }

    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        let slug = slug.trim().to_ascii_lowercase();
        match slug.as_str() {
            "pdf-to-image" => Some(Self::PdfToImage),
            "image-to-pdf" => Some(Self::ImageToPdf),
            _ => Self::ALL.into_iter().find(|tool| tool.slug() == slug),
        }
    }

    #[must_use]
    pub const fn group(self) -> Group {
        match self {
            Self::PdfToWord
            | Self::PdfToExcel
            | Self::PdfToPowerPoint
            | Self::PdfToImage
            | Self::PdfToHtml
            | Self::PdfToMarkdown
            | Self::PdfToText
            | Self::PdfToPdfA => Group::FromPdf,
            Self::WordToPdf
            | Self::ExcelToPdf
            | Self::PowerPointToPdf
            | Self::ImageToPdf
            | Self::ScanToPdf
            | Self::HtmlToPdf => Group::ToPdf,
            Self::Compress | Self::Repair | Self::Ocr => Group::Optimize,
            Self::Unlock | Self::Sign | Self::Redact | Self::Compare | Self::Protect => {
                Group::Security
            }
        }
    }

    #[must_use]
    pub const fn accepts(self) -> &'static [Kind] {
        match self {
            Self::WordToPdf => &[Kind::Word],
            Self::ExcelToPdf => &[Kind::Excel],
            Self::PowerPointToPdf => &[Kind::PowerPoint],
            Self::ImageToPdf | Self::ScanToPdf => &[Kind::Picture],
            Self::HtmlToPdf => &[Kind::Html, Kind::PageAsset],
            _ => PDF,
        }
    }

    #[must_use]
    pub const fn inputs(self) -> Inputs {
        match self {
            Self::Sign | Self::Redact | Self::Ocr => Inputs {
                least: 1,
                most: Some(1),
            },
            Self::Compare => Inputs {
                least: 2,
                most: Some(2),
            },
            _ => Inputs {
                least: 1,
                most: None,
            },
        }
    }

    #[must_use]
    pub const fn output(self) -> Made {
        match self {
            Self::PdfToWord => Made::Word,
            Self::PdfToExcel => Made::Excel,
            Self::PdfToPowerPoint => Made::PowerPoint,
            Self::PdfToImage => Made::Pictures,
            Self::PdfToHtml => Made::Html,
            Self::PdfToMarkdown => Made::Markdown,
            Self::PdfToText => Made::Text,
            Self::Compare => Made::Report,
            _ => Made::Pdf,
        }
    }

    #[must_use]
    pub fn starts_from_a_pdf(self) -> bool {
        self.accepts() == PDF
    }

    #[must_use]
    pub const fn settings(self) -> &'static [Setting] {
        match self {
            Self::PdfToWord
            | Self::PdfToExcel
            | Self::PdfToPowerPoint
            | Self::PdfToHtml
            | Self::PdfToMarkdown
            | Self::PdfToText => ESSENTIAL_PAGES,
            Self::PdfToImage => PICTURES,
            Self::PdfToPdfA | Self::Repair | Self::Unlock => PASSWORD_ONLY,
            Self::WordToPdf | Self::HtmlToPdf => NOTHING,
            Self::ExcelToPdf => WORKBOOK,
            Self::PowerPointToPdf => SLIDES,
            Self::ImageToPdf => PICTURES_TO_PDF,
            Self::ScanToPdf => SCAN,
            Self::Compress => COMPRESS,
            Self::Ocr => OCR,
            Self::Sign => SIGN,
            Self::Redact => REDACT,
            Self::Compare => COMPARE,
            Self::Protect => PROTECT,
        }
    }

    #[must_use]
    pub fn file_password(self) -> Option<Setting> {
        let settings = self.settings();
        [Setting::Password, Setting::FilePassword]
            .into_iter()
            .find(|setting| settings.contains(setting))
    }

    #[must_use]
    pub fn takes(self, name: &str) -> bool {
        self.accepts().iter().any(|kind| kind.matches(name))
    }
}

impl Group {
    pub const ALL: [Self; 4] = [Self::FromPdf, Self::ToPdf, Self::Optimize, Self::Security];

    pub fn tools(self) -> impl Iterator<Item = Tool> {
        Tool::ALL
            .into_iter()
            .filter(move |tool| tool.group() == self)
    }
}

impl Kind {
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Pdf => &["pdf"],
            Self::Word => &["docx"],
            Self::Excel => &["xlsx"],
            Self::PowerPoint => &["pptx"],
            Self::Html => &["html", "htm", "xhtml"],
            Self::Picture => &["jpg", "jpeg", "png", "gif"],
            Self::PageAsset => &["css", "svg", "jpg", "jpeg", "png", "gif"],
        }
    }

    #[must_use]
    pub fn matches(self, name: &str) -> bool {
        let Some((_, extension)) = name.rsplit_once('.') else {
            return false;
        };
        self.extensions()
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
    }

    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        [
            Self::Pdf,
            Self::Word,
            Self::Excel,
            Self::PowerPoint,
            Self::Html,
            Self::Picture,
            Self::PageAsset,
        ]
        .into_iter()
        .find(|kind| kind.matches(name))
    }
}

impl Made {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Word => "docx",
            Self::Excel => "xlsx",
            Self::PowerPoint => "pptx",
            Self::Pictures => "jpg",
            Self::Html | Self::Report => "html",
            Self::Markdown => "md",
            Self::Text => "txt",
            Self::Pdf => "pdf",
        }
    }
}

impl Inputs {
    #[must_use]
    pub const fn allows(self, count: usize) -> bool {
        count >= self.least
            && match self.most {
                Some(most) => count <= most,
                None => true,
            }
    }
}
