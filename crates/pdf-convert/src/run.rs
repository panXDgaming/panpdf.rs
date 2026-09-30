mod bytes;
mod collect;
mod encode;
mod failure;
mod files;
mod jobs;
mod names;
mod pages;
mod random;
mod running;
mod watch;

#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::Arc;
use std::sync::Once;
use std::sync::atomic::AtomicBool;

use convert_files::Start;
use convert_pdftool::Tool as Engine;
use convert_structure::bytes_tool::Run;
use pdf_content::FontProvider;

use crate::catalogue::{Tool, Values};
use watch::Watch;

pub use failure::Failure;
pub use running::{Running, start};

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Input {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl Input {
    #[must_use]
    pub fn new(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            bytes,
        }
    }
}

impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Input")
            .field("name", &self.name)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Context {
    pub fonts: Option<Arc<dyn FontProvider>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Starting,
    File { index: usize, of: usize },
    Step { done: usize, total: usize },
    Busy,
    Writing,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub files: Vec<(String, Vec<u8>)>,
    pub notes: Vec<String>,
}

impl Outcome {
    #[must_use]
    pub fn made_nothing(&self) -> bool {
        self.files.is_empty()
    }
}

impl fmt::Debug for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let files: Vec<(&str, usize)> = self
            .files
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.len()))
            .collect();
        f.debug_struct("Outcome")
            .field("files", &files)
            .field("notes", &self.notes)
            .finish()
    }
}

static FONTS: Once = Once::new();

pub fn init(fonts: Option<Arc<dyn FontProvider>>) {
    FONTS.call_once(|| {
        compare_pdf::set_fonts(fonts.clone());
        sign_pdf::set_fonts(fonts.clone());
        redact_pdf::set_fonts(fonts.clone());
        if let Some(fonts) = fonts {
            pdf_to_image::use_fonts(fonts);
        }
    });
}

enum Runner {
    Pages(pages::Convert),
    OneJob(Start),
    EachFile(Start),
    EachDocument(Run),
    AllAtOnce(Run),
    Job(&'static Engine, &'static str),
    Compare(&'static Engine),
}

const fn runner(tool: Tool) -> Runner {
    match tool {
        Tool::PdfToWord => Runner::Pages(pdf_to_word::convert),
        Tool::PdfToExcel => Runner::Pages(pdf_to_excel::convert),
        Tool::PdfToPowerPoint => Runner::Pages(pdf_to_powerpoint::convert),
        Tool::PdfToHtml => Runner::Pages(pdf_to_html::convert),
        Tool::PdfToMarkdown => Runner::Pages(pdf_to_markdown::convert),
        Tool::PdfToText => Runner::Pages(pdf_to_text::convert),
        Tool::PdfToImage => Runner::OneJob(pdf_to_image::start),
        Tool::ImageToPdf => Runner::OneJob(image_to_pdf::start),
        Tool::ScanToPdf => Runner::OneJob(scan_to_pdf::start),
        Tool::Compress => Runner::EachFile(compress_pdf::start),
        Tool::Ocr => Runner::EachFile(ocr_pdf::start),
        Tool::WordToPdf => Runner::EachDocument(word_to_pdf::run),
        Tool::ExcelToPdf => Runner::EachDocument(excel_to_pdf::run),
        Tool::PowerPointToPdf => Runner::EachDocument(powerpoint_to_pdf::run),
        Tool::HtmlToPdf => Runner::AllAtOnce(html_to_pdf::run),
        Tool::Unlock => Runner::Job(&unlock_pdf::TOOL, "-unlocked"),
        Tool::Repair => Runner::Job(&repair_pdf::TOOL, "-repaired"),
        Tool::PdfToPdfA => Runner::Job(&pdf_to_pdfa::TOOL, "-pdfa"),
        Tool::Sign => Runner::Job(&sign_pdf::TOOL, "-signed"),
        Tool::Redact => Runner::Job(&redact_pdf::TOOL, "-redacted"),
        Tool::Protect => Runner::Job(&protect_pdf::TOOL, "-protected"),
        Tool::Compare => Runner::Compare(&compare_pdf::TOOL),
    }
}

pub fn run(
    tool: Tool,
    inputs: Vec<Input>,
    values: &Values,
    context: &Context,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Outcome, Failure> {
    let mut watch = Watch::new(progress, cancel);
    watch.check()?;
    watch.report(Progress::Starting);
    check_inputs(tool, &inputs)?;
    check_values(tool, values)?;
    if context.fonts.is_some() {
        init(context.fonts.clone());
    }
    random::seed_generator();
    let watch = &mut watch;
    match runner(tool) {
        Runner::Pages(convert) => {
            pages::convert_all(convert, tool, inputs, values, context.fonts.as_ref(), watch)
        }
        Runner::OneJob(start) => files::one_job(start, tool, inputs, values, watch),
        Runner::EachFile(start) => files::each_file(start, tool, inputs, values, watch),
        Runner::EachDocument(run) => {
            bytes::each_document(run, tool, inputs, values, context, watch)
        }
        Runner::AllAtOnce(run) => bytes::all_at_once(run, tool, inputs, values, context, watch),
        Runner::Job(engine, suffix) => jobs::each_file(engine, suffix, tool, inputs, values, watch),
        Runner::Compare(engine) => jobs::compare(engine, tool, inputs, values, watch),
    }
}

fn check_inputs(tool: Tool, inputs: &[Input]) -> Result<(), Failure> {
    let wanted = tool.inputs();
    if inputs.is_empty() {
        return Err(Failure::BadInput("no file was given".to_owned()));
    }
    if wanted.allows(inputs.len()) {
        return Ok(());
    }
    let words = match (wanted.least, wanted.most) {
        (least, Some(most)) if least == most => format!("exactly {least}"),
        (least, Some(most)) => format!("{least} to {most}"),
        (least, None) => format!("at least {least}"),
    };
    Err(Failure::BadInput(format!(
        "{} takes {words} file{}, and {} {} given",
        tool.slug(),
        if wanted.most == Some(1) { "" } else { "s" },
        inputs.len(),
        if inputs.len() == 1 { "was" } else { "were" }
    )))
}

fn check_values(tool: Tool, values: &Values) -> Result<(), Failure> {
    match values.faults(tool).first() {
        Some(fault) => Err(Failure::BadInput(fault.to_string())),
        None => Ok(()),
    }
}
