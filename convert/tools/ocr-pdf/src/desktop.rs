use std::sync::atomic::AtomicBool;

use convert_files::{Input, Job, Outcome, Settings};
use pdf_bytes::{ByteStore, SourceId};
use pdf_edit::text_layer::TextLayer;
use pdf_paint::PaintAtomKind;

struct Work {
    stem: String,
    source: ByteStore,
    password: Vec<u8>,
    engine: pdf_ocr::Tesseract,
    languages: Vec<String>,
    force: bool,
    pages: Vec<usize>,
    next: usize,
    layers: Vec<(usize, TextLayer)>,
    confidence: Vec<f32>,
    had_text: usize,
    notes: Vec<String>,
}

pub fn start(mut inputs: Vec<Input>, settings: &Settings) -> Result<Box<dyn Job>, String> {
    if inputs.len() != 1 {
        return Err("ocr-pdf takes one PDF at a time".to_owned());
    }
    let input = inputs.remove(0);
    let engine = pdf_ocr::Tesseract::locate().map_err(|e| {
        format!("{e}: install Tesseract, or name it with PANPDF_TESSERACT (and its languages with PANPDF_TESSDATA)")
    })?;
    let languages: Vec<String> = settings
        .text("languages", "lao+tha+eng")
        .split(['+', ',', ' '])
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    let password = settings.text("password", "").as_bytes().to_vec();
    let stem = input.stem().to_owned();
    let source = ByteStore::owning(SourceId::next_document(), input.bytes);
    let count = pdf_content::count_pages_recovering(
        &source,
        pdf_content::PageContentLimits::default(),
        pdf_content::RecoverLimits::default(),
        &password,
    )
    .map_err(|e| format!("the file cannot be read as a PDF: {e}"))?
    .into_parts()
    .0;
    let asked = settings.pages()?;
    if let Some(p) = asked.iter().find(|&&p| p >= count) {
        return Err(format!(
            "page {} was asked for and the file has {count} pages",
            p + 1
        ));
    }
    let pages = if asked.is_empty() {
        (0..count).collect()
    } else {
        asked
    };
    Ok(Box::new(Work {
        stem,
        source,
        password,
        engine,
        languages,
        force: settings.flag("force"),
        pages,
        next: 0,
        layers: Vec::new(),
        confidence: Vec::new(),
        had_text: 0,
        notes: Vec::new(),
    }))
}

impl Work {
    fn page(&mut self, index: usize) -> Result<(), String> {
        let view = pdf_session::interpret_page_for_display(
            &self.source,
            index,
            &self.password,
            None,
            None,
        )
        .map_err(|e| format!("page {}: {e}", index + 1))?;
        let has_text = view
            .graph
            .atoms
            .iter()
            .any(|a| matches!(a.kind, PaintAtomKind::Text(_)));
        if has_text && !self.force {
            self.had_text += 1;
            return Ok(());
        }
        let cancel = AtomicBool::new(false);
        let reading = pdf_ocr::read_page(
            &view.layers(),
            &view.program.geometry,
            &self.engine,
            &self.languages,
            &cancel,
        )
        .map_err(|e| format!("page {}: {e}", index + 1))?;
        if reading.layer.words.is_empty() {
            self.notes
                .push(format!("page {}: no words were recognised", index + 1));
            return Ok(());
        }
        if let Some(c) = reading.confidence {
            self.confidence.push(c);
        }
        self.layers.push((index, reading.layer));
        Ok(())
    }
}

impl Job for Work {
    fn total(&self) -> usize {
        self.pages.len()
    }

    fn step(&mut self) -> Result<(), String> {
        let Some(&index) = self.pages.get(self.next) else {
            return Ok(());
        };
        self.next += 1;
        if let Err(why) = self.page(index) {
            self.notes.push(why);
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Outcome, String> {
        let mut notes = self.notes;
        if self.layers.is_empty() {
            notes.push(format!(
                "nothing to write: {} pages already had text{}",
                self.had_text,
                if self.had_text > 0 {
                    " (force=true reads them anyway)"
                } else {
                    ""
                }
            ));
            return Err(notes.join("; "));
        }
        let confidence = if self.confidence.is_empty() {
            String::new()
        } else {
            let mean = self.confidence.iter().sum::<f32>() / self.confidence.len() as f32;
            format!(", mean confidence {mean:.0}%")
        };
        let mut outcome =
            crate::write(&self.stem, &self.source, &self.password, self.layers, notes)?;
        if let Some(last) = outcome.notes.last_mut() {
            last.push_str(&format!(
                "{confidence}; {} pages already had text",
                self.had_text
            ));
        }
        Ok(outcome)
    }
}
