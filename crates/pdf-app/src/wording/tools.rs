use pdf_convert::{Choice, Group, Kind, Made, Setting, Tool};

use crate::tools::{Missing, Trouble};

use super::edits::count;
use super::facts::bytes_said;
use super::{Lang, SEP};

#[derive(Clone, Debug, PartialEq)]
pub enum Tools {
    HomeTile,
    HomeTileHelp,
    AllTools,
    RoomTitle,
    RoomPromise,
    ToolCount(usize),
    SearchHint,
    AllGroups,
    NothingMatches,
    BackToStart,
    BackToTools,
    GroupName(Group),
    GroupChip(Group),
    Name(Tool),
    Entry(Tool),
    Blurb(Tool),
    Go(Tool),
    Note(Tool),
    FileHeading,
    FilesHeading,
    SettingsHeading,
    MoreSettings,
    Older,
    Newer,
    Choose(Kind),
    ChangeFile,
    ChooseFile,
    AddFiles,
    RemoveFile,
    DropHint,
    Detail {
        pages: Option<usize>,
        bytes: u64,
        unsaved: bool,
    },
    NotRead {
        name: String,
        why: String,
    },
    LeftOut(String),
    Missing(Missing),
    NothingUploaded,
    Label(Setting),
    Help(Setting),
    Hint(Setting),
    Unit(Setting),
    RepeatPassword,
    RepeatHint,
    Choice(Choice),
    Explains(Choice),
    PagesAll,
    PagesThis,
    PagesLast,
    PagesFirst,
    PagesEvery,
    PresetScreen,
    PresetNormal,
    PresetPrint,
    Dpi(u32),
    ChoosePicture,
    ClearDrawing,
    SignHere,
    TypefaceStandard,
    ChooseFilesTitle,
    SaveResultTitle,
    ChooseSignatureTitle,
    Converting,
    Working,
    StaysHere,
    Stop,
    Stopping,
    Starting,
    WorkingOnIt,
    Writing,
    FileOf {
        index: usize,
        of: usize,
    },
    PageOf {
        done: usize,
        total: usize,
    },
    PictureOf {
        done: usize,
        total: usize,
    },
    Percent(u32),
    Ready(Made),
    FilesReady,
    DoneIn {
        files: usize,
        tenths: u32,
    },
    Size(u64),
    Shrank {
        from: u64,
        to: u64,
    },
    AndMore(usize),
    SavedIn(String),
    NotSaved(String),
    SaveAs,
    OpenInPanPdf,
    OpenIt,
    ShowInFolder,
    StartOver,
    WhatItDid,
    ThatDidNotWork,
    NothingMade,
    Stopped,
    NeedsPasswordTitle,
    NeedsPassword,
    PasswordOfOlder,
    PasswordOfNewer,
    PasswordDidNotOpen,
    TryAgain,
    Details,
    BackToSettings,
    Trouble(Trouble),
}

impl Tools {
    #[must_use]
    pub fn say(&self, lang: Lang) -> String {
        match lang {
            Lang::English => self.english(),
        }
    }

    #[must_use]
    pub fn said(&self, lang: Lang) -> Option<String> {
        let said = self.say(lang);
        (!said.is_empty()).then_some(said)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "a table of sentences: its length is how many there are"
    )]
    fn english(&self) -> String {
        match self {
            Self::HomeTile | Self::RoomTitle | Self::BackToTools => "Tools".to_owned(),
            Self::HomeTileHelp => "Convert, compress, sign and protect PDFs".to_owned(),
            Self::AllTools => "All tools\u{2026}".to_owned(),
            Self::RoomPromise => {
                "Everything runs on this computer. Your files are never uploaded.".to_owned()
            }
            Self::ToolCount(many) => count(*many, "tool"),
            Self::SearchHint => "Search tools".to_owned(),
            Self::AllGroups => "All".to_owned(),
            Self::NothingMatches => "No tool matches that".to_owned(),
            Self::BackToStart => "Back to the start".to_owned(),
            Self::GroupName(group) => group_name(*group).to_owned(),
            Self::GroupChip(group) => group_chip(*group).to_owned(),
            Self::Name(tool) => name(*tool).to_owned(),
            Self::Entry(tool) => format!("{}\u{2026}", name(*tool)),
            Self::Blurb(tool) => blurb(*tool).to_owned(),
            Self::Go(tool) => go(*tool).to_owned(),
            Self::Note(tool) => note(*tool).to_owned(),
            Self::FileHeading => "File".to_owned(),
            Self::FilesHeading => "Files".to_owned(),
            Self::SettingsHeading => "Settings".to_owned(),
            Self::MoreSettings => "More settings".to_owned(),
            Self::Older => "Older".to_owned(),
            Self::Newer => "Newer".to_owned(),
            Self::Choose(kind) => choose(*kind).to_owned(),
            Self::ChangeFile => "Change\u{2026}".to_owned(),
            Self::ChooseFile => "Choose\u{2026}".to_owned(),
            Self::AddFiles => "Add files\u{2026}".to_owned(),
            Self::RemoveFile => "Remove from the list".to_owned(),
            Self::DropHint => "or drop files anywhere in this window".to_owned(),
            Self::Detail {
                pages,
                bytes,
                unsaved,
            } => {
                let mut parts = Vec::new();
                if let Some(pages) = pages {
                    parts.push(count(*pages, "page"));
                }
                parts.push(bytes_said(*bytes));
                if *unsaved {
                    parts.push("includes your unsaved changes".to_owned());
                }
                parts.join(SEP)
            }
            Self::NotRead { name, why } => format!("{name} could not be read: {why}"),
            Self::LeftOut(names) => format!("Left out, not the kind of file this takes: {names}"),
            Self::Missing(missing) => missing_words(*missing).to_owned(),
            Self::NothingUploaded => "Done on this computer. Nothing is uploaded.".to_owned(),
            Self::Label(setting) => label(*setting).to_owned(),
            Self::Help(setting) => help(*setting).to_owned(),
            Self::Hint(setting) => hint(*setting).to_owned(),
            Self::Unit(setting) => unit(*setting).to_owned(),
            Self::RepeatPassword => "Repeat the password".to_owned(),
            Self::RepeatHint => "Type it again".to_owned(),
            Self::Choice(choice) => choice_label(*choice).to_owned(),
            Self::Explains(choice) => explains(*choice).to_owned(),
            Self::PagesAll => "All pages".to_owned(),
            Self::PagesThis => "This page".to_owned(),
            Self::PagesLast => "Last page".to_owned(),
            Self::PagesFirst => "First page".to_owned(),
            Self::PagesEvery => "Every page".to_owned(),
            Self::PresetScreen => "Screen".to_owned(),
            Self::PresetNormal => "Normal".to_owned(),
            Self::PresetPrint => "Print".to_owned(),
            Self::Dpi(dpi) => format!("{dpi} dpi"),
            Self::ChoosePicture => "Choose a picture\u{2026}".to_owned(),
            Self::ClearDrawing => "Clear".to_owned(),
            Self::SignHere => "Sign here with the mouse or a pen".to_owned(),
            Self::TypefaceStandard => "Standard".to_owned(),
            Self::ChooseFilesTitle => "Choose files".to_owned(),
            Self::SaveResultTitle => "Save the result as".to_owned(),
            Self::ChooseSignatureTitle => "Choose the picture of the signature".to_owned(),
            Self::Converting => "Converting\u{2026}".to_owned(),
            Self::Working => "Working\u{2026}".to_owned(),
            Self::StaysHere => "This happens on this computer.".to_owned(),
            Self::Stop => "Stop".to_owned(),
            Self::Stopping => "Stopping\u{2026}".to_owned(),
            Self::Starting => "Starting\u{2026}".to_owned(),
            Self::WorkingOnIt => "Working on it\u{2026}".to_owned(),
            Self::Writing => "Writing the result\u{2026}".to_owned(),
            Self::FileOf { index, of } => format!("File {} of {of}", index + 1),
            Self::PageOf { done, total } => format!("Page {done} of {total}"),
            Self::PictureOf { done, total } => format!("Picture {done} of {total}"),
            Self::Percent(percent) => format!("{percent}%"),
            Self::Ready(made) => ready(*made).to_owned(),
            Self::FilesReady => "Your files are ready".to_owned(),
            Self::DoneIn { files, tenths } => format!(
                "{}, done on this computer in {}.{} s",
                count(*files, "file"),
                tenths / 10,
                tenths % 10
            ),
            Self::Size(bytes) => bytes_said(*bytes),
            Self::Shrank { from, to } => {
                format!("{} \u{2192} {}", bytes_said(*from), bytes_said(*to))
            }
            Self::AndMore(more) => format!("and {more} more"),
            Self::SavedIn(folder) => format!("Saved in {folder}"),
            Self::NotSaved(why) => format!("Not saved: {why}"),
            Self::SaveAs => "Save as\u{2026}".to_owned(),
            Self::OpenInPanPdf => "Open in PanPDF".to_owned(),
            Self::OpenIt => "Open".to_owned(),
            Self::ShowInFolder => "Show in folder".to_owned(),
            Self::StartOver => "Start over".to_owned(),
            Self::WhatItDid => "What the tool did".to_owned(),
            Self::ThatDidNotWork => "That did not work".to_owned(),
            Self::NothingMade => "Nothing was made.".to_owned(),
            Self::Stopped => "Stopped".to_owned(),
            Self::NeedsPasswordTitle => "This PDF needs its password".to_owned(),
            Self::NeedsPassword => "Type the password that opens it. It is used here, on this computer, and nowhere else.".to_owned(),
            Self::PasswordOfOlder => "Password of the older file".to_owned(),
            Self::PasswordOfNewer => "Password of the newer file".to_owned(),
            Self::PasswordDidNotOpen => "That password did not open it.".to_owned(),
            Self::TryAgain => "Try again".to_owned(),
            Self::Details => "Details".to_owned(),
            Self::BackToSettings => "Back to the settings".to_owned(),
            Self::Trouble(trouble) => trouble_words(*trouble).to_owned(),
        }
    }
}

const fn group_name(group: Group) -> &'static str {
    match group {
        Group::FromPdf => "Convert from PDF",
        Group::ToPdf => "Convert to PDF",
        Group::Optimize => "Optimize",
        Group::Security => "PDF security",
    }
}

const fn group_chip(group: Group) -> &'static str {
    match group {
        Group::FromPdf => "From PDF",
        Group::ToPdf => "To PDF",
        Group::Optimize => "Optimize",
        Group::Security => "Security",
    }
}

const fn name(tool: Tool) -> &'static str {
    match tool {
        Tool::PdfToWord => "PDF to Word",
        Tool::PdfToExcel => "PDF to Excel",
        Tool::PdfToPowerPoint => "PDF to PowerPoint",
        Tool::PdfToImage => "PDF to JPG",
        Tool::PdfToHtml => "PDF to HTML",
        Tool::PdfToMarkdown => "PDF to Markdown",
        Tool::PdfToText => "PDF to Text",
        Tool::PdfToPdfA => "PDF to PDF/A",
        Tool::WordToPdf => "Word to PDF",
        Tool::ExcelToPdf => "Excel to PDF",
        Tool::PowerPointToPdf => "PowerPoint to PDF",
        Tool::ImageToPdf => "JPG to PDF",
        Tool::ScanToPdf => "Scan to PDF",
        Tool::HtmlToPdf => "HTML to PDF",
        Tool::Compress => "Compress PDF",
        Tool::Repair => "Repair PDF",
        Tool::Ocr => "OCR PDF",
        Tool::Unlock => "Unlock PDF",
        Tool::Sign => "Sign PDF",
        Tool::Redact => "Redact PDF",
        Tool::Compare => "Compare PDF",
        Tool::Protect => "Protect PDF",
    }
}

const fn blurb(tool: Tool) -> &'static str {
    match tool {
        Tool::PdfToWord => "Turn PDFs into DOCX you can edit, tables and pictures kept.",
        Tool::PdfToExcel => "Pull the tables out of a PDF into a spreadsheet.",
        Tool::PdfToPowerPoint => "Every page becomes a slide you can edit.",
        Tool::PdfToImage => "Every page as a picture, or the pictures inside the PDF.",
        Tool::PdfToHtml => "A web page from a PDF, text you can search and link.",
        Tool::PdfToMarkdown => "Headings, lists and tables as Markdown text.",
        Tool::PdfToText => "All the words of a PDF as plain text.",
        Tool::PdfToPdfA => "The archive format courts and archives ask for.",
        Tool::WordToPdf => "Turn DOCX documents into PDF, every script set right.",
        Tool::ExcelToPdf => "Turn XLSX spreadsheets into PDF pages.",
        Tool::PowerPointToPdf => "Turn PPTX slide decks into PDF.",
        Tool::ImageToPdf => "Turn JPG, PNG and GIF pictures into a PDF.",
        Tool::ScanToPdf => "Phone photos of paper become clean, straight PDF pages.",
        Tool::HtmlToPdf => "Turn a saved web page into a PDF.",
        Tool::Compress => "Make a PDF smaller while it still looks right.",
        Tool::Repair => "Recover what can be read from a damaged PDF.",
        Tool::Ocr => "Make a scanned PDF searchable: its words become text you can find and copy.",
        Tool::Unlock => "Remove the password from a PDF you may open.",
        Tool::Sign => "Add your signature: drawn, typed or a picture.",
        Tool::Redact => "Remove words for good, not just cover them.",
        Tool::Compare => "See what changed between two versions of a PDF.",
        Tool::Protect => "Lock a PDF with a password (AES-256).",
    }
}

const fn go(tool: Tool) -> &'static str {
    match tool {
        Tool::PdfToWord => "Convert to Word",
        Tool::PdfToExcel => "Convert to Excel",
        Tool::PdfToPowerPoint => "Convert to PowerPoint",
        Tool::PdfToImage => "Convert to JPG",
        Tool::PdfToHtml => "Convert to HTML",
        Tool::PdfToMarkdown => "Convert to Markdown",
        Tool::PdfToText => "Convert to Text",
        Tool::PdfToPdfA => "Convert to PDF/A",
        Tool::WordToPdf
        | Tool::ExcelToPdf
        | Tool::PowerPointToPdf
        | Tool::ImageToPdf
        | Tool::HtmlToPdf => "Convert to PDF",
        Tool::ScanToPdf => "Make the PDF",
        Tool::Compress => "Compress PDF",
        Tool::Repair => "Repair PDF",
        Tool::Ocr => "Make it searchable",
        Tool::Unlock => "Unlock PDF",
        Tool::Sign => "Sign PDF",
        Tool::Redact => "Redact PDF",
        Tool::Compare => "Compare PDF",
        Tool::Protect => "Protect PDF",
    }
}

const fn note(tool: Tool) -> &'static str {
    match tool {
        Tool::PdfToPdfA => {
            "Makes PDF/A-2b: the pages look the same in every reader, fonts inside, no encryption or scripts."
        }
        Tool::WordToPdf => {
            "Fonts the document names but this computer does not have are replaced by ones of the same kind."
        }
        Tool::HtmlToPdf => {
            "Choose the .html file together with its pictures and stylesheets, all at once, so the page is drawn with them. A web address is never fetched."
        }
        Tool::Compress => {
            "A password-protected file stays protected after compressing, with the same password."
        }
        Tool::Protect => {
            "Without the password the file cannot be opened, and PanPDF cannot recover it."
        }
        Tool::Redact => {
            "The words are taken out of the file itself, not painted over. Check the result before you share it."
        }
        Tool::PdfToWord
        | Tool::PdfToExcel
        | Tool::PdfToPowerPoint
        | Tool::PdfToImage
        | Tool::PdfToHtml
        | Tool::PdfToMarkdown
        | Tool::PdfToText
        | Tool::ExcelToPdf
        | Tool::PowerPointToPdf
        | Tool::ImageToPdf
        | Tool::ScanToPdf
        | Tool::Repair
        | Tool::Ocr
        | Tool::Unlock
        | Tool::Sign
        | Tool::Compare => "",
    }
}

const fn choose(kind: Kind) -> &'static str {
    match kind {
        Kind::Pdf => "Choose a PDF",
        Kind::Word => "Choose a Word document (.docx)",
        Kind::Excel => "Choose an Excel workbook (.xlsx)",
        Kind::PowerPoint => "Choose a PowerPoint deck (.pptx)",
        Kind::Html | Kind::PageAsset => "Choose the web page (.html) and its pictures",
        Kind::Picture => "Choose pictures (JPG, PNG or GIF)",
    }
}

const fn missing_words(missing: Missing) -> &'static str {
    match missing {
        Missing::AFile => "Choose a file to begin",
        Missing::TwoFiles => "Choose the older file and the newer file",
        Missing::TheNewerFile => "Choose the newer file",
        Missing::TheWebPage => "Add the web page itself, an .html file",
        Missing::WordsToRemove => "Type the words to remove, or apply the file's own marks",
        Missing::APassword => "Type a password to lock the file with",
        Missing::PasswordsDiffer => "The two passwords are not the same",
        Missing::TheDrawing => "Draw your signature first",
        Missing::TheName => "Type your name first",
        Missing::ThePicture => "Choose the picture of your signature first",
    }
}

const fn ready(made: Made) -> &'static str {
    match made {
        Made::Word => "Your Word document is ready",
        Made::Excel => "Your Excel workbook is ready",
        Made::PowerPoint => "Your PowerPoint deck is ready",
        Made::Pictures => "Your pictures are ready",
        Made::Html => "Your web page is ready",
        Made::Markdown => "Your Markdown file is ready",
        Made::Text => "Your text file is ready",
        Made::Pdf => "Your PDF is ready",
        Made::Report => "Your comparison is ready",
    }
}

const fn label(setting: Setting) -> &'static str {
    match setting {
        Setting::Pages | Setting::SignPages => "Pages",
        Setting::Password | Setting::NewPassword => "Password",
        Setting::SecondPassword => "Password of the newer file",
        Setting::WhatToTake => "What to take",
        Setting::PictureFormat => "Format",
        Setting::Resolution => "Quality",
        Setting::PictureQuality | Setting::ScanQuality => "JPEG quality",
        Setting::Fit => "Fit",
        Setting::Paper => "Paper",
        Setting::Grid => "Print gridlines",
        Setting::SheetOrientation | Setting::Orientation => "Orientation",
        Setting::HiddenSlides => "Include hidden slides",
        Setting::PageSize | Setting::ScanPageSize => "Page size",
        Setting::Margin => "Margin",
        Setting::Merge => "One PDF for all the pictures",
        Setting::Crop => "Find the page edges",
        Setting::Look => "Colour",
        Setting::Level => "Compression",
        Setting::Languages => "Languages of the text",
        Setting::ReadPagesWithText => "Read pages that already have text",
        Setting::Signature => "Signature",
        Setting::Drawing => "Draw your signature",
        Setting::TypedName => "Your name",
        Setting::SignaturePicture => "Signature picture",
        Setting::Place => "Place",
        Setting::SignatureAt => "Exact place",
        Setting::SignatureWidth => "Width",
        Setting::TypeFace => "Typeface",
        Setting::Flatten => "Leave the unsigned version out",
        Setting::Search => "Words to remove",
        Setting::MatchCase => "Match capital letters",
        Setting::UseMarks => "Apply the file's own redaction marks",
        Setting::BoxColour => "Box colour",
        Setting::Areas => "Areas",
        Setting::ReportAs => "Show the result as",
        Setting::LeavePicturesOut => "Leave the pictures out",
        Setting::ComparisonResolution => "Resolution of the pictures",
        Setting::Forbid => "Forbid",
        Setting::OwnerPassword => "Owner password",
        Setting::FilePassword => "Password of the file",
    }
}

const fn help(setting: Setting) -> &'static str {
    match setting {
        Setting::Pages => "For example 1,3-5. Leave it empty for every page.",
        Setting::Password => "Only if the PDF asks for one.",
        Setting::SecondPassword => "Only if the newer PDF asks for one.",
        Setting::Crop => "Straightens the paper and crops the table around it.",
        Setting::SignaturePicture => "A PNG or JPG; a PNG with a clear background looks best.",
        Setting::SignatureAt => {
            "x, y, width and height in points from the bottom left of the page. Leave it empty to use Place."
        }
        Setting::Flatten => "One revision: the unsigned document is not left inside the file.",
        Setting::Search => {
            "Separate several with a comma. Every place they appear is removed from the file, not painted over."
        }
        Setting::UseMarks => "Marks someone left with the Redact tool of another program.",
        Setting::ReadPagesWithText => {
            "Pages that already have text are otherwise left as they are."
        }
        Setting::Forbid => {
            "Binds anyone without the owner password. With none given, one is made at random and not kept."
        }
        Setting::OwnerPassword => {
            "Needed to lift the limits above. Leave it empty if you do not need it."
        }
        Setting::FilePassword => "Only if the PDF you are protecting already has a password.",
        Setting::SignPages
        | Setting::NewPassword
        | Setting::WhatToTake
        | Setting::PictureFormat
        | Setting::Resolution
        | Setting::PictureQuality
        | Setting::Fit
        | Setting::Paper
        | Setting::Grid
        | Setting::SheetOrientation
        | Setting::HiddenSlides
        | Setting::PageSize
        | Setting::ScanPageSize
        | Setting::Orientation
        | Setting::Margin
        | Setting::Merge
        | Setting::Look
        | Setting::ScanQuality
        | Setting::Level
        | Setting::Languages
        | Setting::Signature
        | Setting::Drawing
        | Setting::TypedName
        | Setting::Place
        | Setting::SignatureWidth
        | Setting::TypeFace
        | Setting::MatchCase
        | Setting::BoxColour
        | Setting::Areas
        | Setting::ReportAs
        | Setting::LeavePicturesOut
        | Setting::ComparisonResolution => "",
    }
}

const fn hint(setting: Setting) -> &'static str {
    match setting {
        Setting::Pages => "e.g. 1, 3-5",
        Setting::SignPages => "or e.g. 2-3",
        Setting::Password | Setting::SecondPassword | Setting::FilePassword => {
            "Only if the PDF asks for one"
        }
        Setting::NewPassword => "Type a password",
        Setting::TypedName => "Type your name",
        Setting::SignatureAt => "x, y, width, height",
        Setting::Search => "e.g. an account number, a name",
        Setting::OwnerPassword => "Optional",
        Setting::WhatToTake
        | Setting::PictureFormat
        | Setting::Resolution
        | Setting::PictureQuality
        | Setting::Fit
        | Setting::Paper
        | Setting::Grid
        | Setting::SheetOrientation
        | Setting::HiddenSlides
        | Setting::PageSize
        | Setting::ScanPageSize
        | Setting::Orientation
        | Setting::Margin
        | Setting::Merge
        | Setting::Crop
        | Setting::Look
        | Setting::ScanQuality
        | Setting::Level
        | Setting::Languages
        | Setting::ReadPagesWithText
        | Setting::Signature
        | Setting::Drawing
        | Setting::SignaturePicture
        | Setting::Place
        | Setting::SignatureWidth
        | Setting::TypeFace
        | Setting::Flatten
        | Setting::MatchCase
        | Setting::UseMarks
        | Setting::BoxColour
        | Setting::Areas
        | Setting::ReportAs
        | Setting::LeavePicturesOut
        | Setting::ComparisonResolution
        | Setting::Forbid => "",
    }
}

const fn unit(setting: Setting) -> &'static str {
    match setting {
        Setting::SignatureWidth => " pt",
        Setting::ComparisonResolution => " dpi",
        Setting::Pages
        | Setting::Password
        | Setting::SecondPassword
        | Setting::WhatToTake
        | Setting::PictureFormat
        | Setting::Resolution
        | Setting::PictureQuality
        | Setting::Fit
        | Setting::Paper
        | Setting::Grid
        | Setting::SheetOrientation
        | Setting::HiddenSlides
        | Setting::PageSize
        | Setting::ScanPageSize
        | Setting::Orientation
        | Setting::Margin
        | Setting::Merge
        | Setting::Crop
        | Setting::Look
        | Setting::ScanQuality
        | Setting::Level
        | Setting::Languages
        | Setting::ReadPagesWithText
        | Setting::Signature
        | Setting::Drawing
        | Setting::TypedName
        | Setting::SignaturePicture
        | Setting::SignPages
        | Setting::Place
        | Setting::SignatureAt
        | Setting::TypeFace
        | Setting::Flatten
        | Setting::Search
        | Setting::MatchCase
        | Setting::UseMarks
        | Setting::BoxColour
        | Setting::Areas
        | Setting::ReportAs
        | Setting::LeavePicturesOut
        | Setting::NewPassword
        | Setting::Forbid
        | Setting::OwnerPassword
        | Setting::FilePassword => "",
    }
}

const fn choice_label(choice: Choice) -> &'static str {
    match choice {
        Choice::EveryPage => "Pages as pictures",
        Choice::PicturesInside => "Extract pictures",
        Choice::Jpg => "JPG",
        Choice::Png => "PNG",
        Choice::SameAsPicture => "Same as the picture",
        Choice::A4 => "A4",
        Choice::Letter => "US Letter",
        Choice::A3 => "A3",
        Choice::A5 => "A5",
        Choice::Legal => "US Legal",
        Choice::AsInWorkbook => "As in the workbook",
        Choice::ColumnsOnOnePage => "Fit columns on one page",
        Choice::SheetOnOnePage => "One sheet per page",
        Choice::ActualSize => "Actual size",
        Choice::AutoOrientation => "Auto",
        Choice::Portrait => "Portrait",
        Choice::Landscape => "Landscape",
        Choice::NoMargin => "None",
        Choice::SmallMargin => "Small",
        Choice::BigMargin => "Big",
        Choice::Colour => "Colour",
        Choice::Grey => "Grey",
        Choice::BlackAndWhite => "Black & white",
        Choice::AsTaken => "As taken",
        Choice::Extreme => "Extreme",
        Choice::Recommended => "Recommended",
        Choice::Low => "Low",
        Choice::Drawn => "Draw",
        Choice::Typed => "Type",
        Choice::FromPicture => "Picture",
        Choice::BottomRight => "Bottom right",
        Choice::BottomLeft => "Bottom left",
        Choice::BottomCentre => "Bottom centre",
        Choice::TopRight => "Top right",
        Choice::TopLeft => "Top left",
        Choice::Centre => "Centre",
        Choice::Black => "Black",
        Choice::White => "White",
        Choice::HtmlReport => "Report with pictures",
        Choice::TextReport => "Text report",
        Choice::Print => "Printing",
        Choice::PrintHighQuality => "Printing at full quality",
        Choice::Copy => "Copying text",
        Choice::Modify => "Changing the file",
        Choice::Annotate => "Adding notes",
        Choice::FillForms => "Filling in forms",
        Choice::Assemble => "Rearranging pages",
    }
}

const fn explains(choice: Choice) -> &'static str {
    match choice {
        Choice::EveryPage => "Every page becomes one picture.",
        Choice::PicturesInside => "The pictures inside the PDF, at their own quality.",
        Choice::AsInWorkbook => "Print areas, page breaks and scaling the author chose.",
        Choice::ColumnsOnOnePage => "Every column on the page, rows run on.",
        Choice::SheetOnOnePage => "Each sheet shrunk onto a single page.",
        Choice::ActualSize => "No shrinking: a wide sheet runs over as many pages as it needs.",
        Choice::Extreme => "Smallest file. Pictures lose some detail.",
        Choice::Recommended => "Much smaller, looks the same on screen and on paper.",
        Choice::Low => "Lossless: every picture kept exactly. Smaller savings.",
        Choice::HtmlReport => "A web page: every changed page side by side, changes marked.",
        Choice::TextReport => "A plain list of every change, page by page.",
        Choice::Jpg
        | Choice::Png
        | Choice::SameAsPicture
        | Choice::A4
        | Choice::Letter
        | Choice::A3
        | Choice::A5
        | Choice::Legal
        | Choice::AutoOrientation
        | Choice::Portrait
        | Choice::Landscape
        | Choice::NoMargin
        | Choice::SmallMargin
        | Choice::BigMargin
        | Choice::Colour
        | Choice::Grey
        | Choice::BlackAndWhite
        | Choice::AsTaken
        | Choice::Drawn
        | Choice::Typed
        | Choice::FromPicture
        | Choice::BottomRight
        | Choice::BottomLeft
        | Choice::BottomCentre
        | Choice::TopRight
        | Choice::TopLeft
        | Choice::Centre
        | Choice::Black
        | Choice::White
        | Choice::Print
        | Choice::PrintHighQuality
        | Choice::Copy
        | Choice::Modify
        | Choice::Annotate
        | Choice::FillForms
        | Choice::Assemble => "",
    }
}

const fn trouble_words(trouble: Trouble) -> &'static str {
    match trouble {
        Trouble::NotAPdf => {
            "This file is not a PDF, or it is empty or too damaged to open. If it is a PDF, try Repair PDF first."
        }
        Trouble::Damaged => {
            "This file is damaged in a way the tool could not get past. Try Repair PDF first, then use the repaired file here."
        }
        Trouble::NotAWordDocument => {
            "This file is not a Word document (.docx). An older .doc file has to be saved as .docx in Word first."
        }
        Trouble::NotAWorkbook => {
            "This file is not an Excel workbook (.xlsx). An older .xls file has to be saved as .xlsx in Excel first."
        }
        Trouble::NotAPresentation => {
            "This file is not a PowerPoint presentation (.pptx). An older .ppt file has to be saved as .pptx in PowerPoint first."
        }
        Trouble::PageNotThere => {
            "A page asked for is not in the file. Check the pages, or leave them empty for every page."
        }
        Trouble::NoMatches => {
            "The words you searched for are not in this PDF, so nothing was changed. Check the spelling, or search for other words."
        }
        Trouble::NothingToRedact => "There is nothing to remove yet. Type the words to search for.",
        Trouble::NoPictures => {
            "There are no pictures on the pages chosen, so there was nothing to save. To save whole pages as pictures, choose Pages as pictures."
        }
        Trouble::PictureUnreadable => {
            "A picture is damaged, or in a form this tool cannot read. Save it again as JPEG or PNG and try once more."
        }
        Trouble::Unexpected => {
            "The tool stopped unexpectedly on this file. The details below say where."
        }
        Trouble::Other => {
            "Something went wrong and nothing was made. What the tool said is under Details."
        }
    }
}

#[cfg(test)]
mod tests;
