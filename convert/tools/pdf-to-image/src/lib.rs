use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use convert_files::{Input, Job, Outcome, Settings};
use convert_raster::Image;
use pdf_bytes::{ByteStore, SourceId};
use pdf_content::FontProvider;

pub use convert_files;

static FONTS: OnceLock<Arc<dyn FontProvider>> = OnceLock::new();

pub fn use_fonts(fonts: Arc<dyn FontProvider>) {
    let _ = FONTS.set(fonts);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Jpeg,
    Png,
}

impl Format {
    const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }
}

struct Document {
    stem: String,
    source: ByteStore,
    pages: Vec<usize>,
    digits: usize,
}

struct Work {
    extract: bool,
    format: Format,
    dpi: f64,
    quality: u8,
    password: Vec<u8>,
    documents: Vec<Document>,
    steps: Vec<(usize, usize)>,
    next: usize,
    files: Vec<(String, Vec<u8>)>,
    notes: Vec<String>,
    seen: HashSet<(usize, u32, u16)>,
    copied: usize,
    decoded: usize,
}

fn page_count(source: &ByteStore, password: &[u8]) -> Result<usize, String> {
    pdf_content::count_pages_recovering(
        source,
        pdf_content::PageContentLimits::default(),
        pdf_content::RecoverLimits::default(),
        password,
    )
    .map(|r| r.into_parts().0)
    .map_err(|e| format!("the file cannot be read as a PDF: {e}"))
}

pub fn start(inputs: Vec<Input>, settings: &Settings) -> Result<Box<dyn Job>, String> {
    let extract = match settings.text("mode", "pages") {
        "pages" => false,
        "extract" | "pictures" | "images" => true,
        other => return Err(format!("mode: '{other}' is not pages or extract")),
    };
    let format = match settings
        .text("format", if extract { "png" } else { "jpg" })
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => Format::Jpeg,
        "png" => Format::Png,
        other => return Err(format!("format: '{other}' is not jpg or png")),
    };
    let dpi = settings.number("dpi", 150.0)?;
    if !(10.0..=1200.0).contains(&dpi) {
        return Err(format!("dpi: {dpi} is not between 10 and 1200"));
    }
    let quality = settings.number("quality", 85.0)?;
    if !(1.0..=100.0).contains(&quality) {
        return Err(format!("quality: {quality} is not between 1 and 100"));
    }
    let quality = convert_raster::to_u8(quality);
    let password = settings.text("password", "").as_bytes().to_vec();
    let asked = settings.pages()?;
    if inputs.is_empty() {
        return Err("no PDF was given".to_owned());
    }
    let mut documents = Vec::new();
    let mut steps = Vec::new();
    for input in inputs {
        let stem = input.stem().to_owned();
        let source = ByteStore::owning(SourceId::next_document(), input.bytes);
        let count = page_count(&source, &password).map_err(|e| format!("{}: {e}", input.name))?;
        let pages: Vec<usize> = if asked.is_empty() {
            (0..count).collect()
        } else {
            if let Some(p) = asked.iter().find(|&&p| p >= count) {
                return Err(format!(
                    "{}: page {} was asked for and the file has {count} pages",
                    input.name,
                    p + 1
                ));
            }
            asked.clone()
        };
        let at = documents.len();
        steps.extend(pages.iter().map(|&p| (at, p)));
        documents.push(Document {
            stem,
            source,
            pages,
            digits: count.to_string().len(),
        });
    }
    Ok(Box::new(Work {
        extract,
        format,
        dpi,
        quality,
        password,
        documents,
        steps,
        next: 0,
        files: Vec::new(),
        notes: Vec::new(),
        seen: HashSet::new(),
        copied: 0,
        decoded: 0,
    }))
}

convert_files::export_files_tool!(crate::start);

impl Work {
    fn encode(&self, image: &Image, dpi: Option<f64>) -> Result<Vec<u8>, String> {
        match self.format {
            Format::Jpeg => image.to_jpeg(self.quality),
            Format::Png => image.to_png(dpi),
        }
    }

    fn page(&mut self, document: usize, page: usize) -> Result<(), String> {
        let doc = &self.documents[document];
        let view = pdf_session::interpret_page_for_display(
            &doc.source,
            page,
            &self.password,
            None,
            FONTS.get().cloned(),
        )
        .map_err(|e| format!("page {}: {e}", page + 1))?;
        let name = format!("{}-{:0width$}", doc.stem, page + 1, width = doc.digits);
        if !self.extract {
            let options = pdf_render::RenderOptions {
                scale: self.dpi / 72.0,
                ..pdf_render::RenderOptions::default()
            };
            let (canvas, _) =
                pdf_render::render_page_layers(&view.layers(), &view.program.geometry, options)
                    .map_err(|e| format!("page {}: {e}", page + 1))?;
            let image = Image {
                width: canvas.width,
                height: canvas.height,
                channels: 3,
                data: canvas.to_rgb8(),
            };
            let bytes = self.encode(&image, Some(self.dpi))?;
            self.files
                .push((format!("{name}.{}", self.format.extension()), bytes));
            return Ok(());
        }
        let mut number = 0;
        for image in convert_raster::paint::images(&view.graph) {
            let key = (
                document,
                image.reference.object_number(),
                image.reference.generation(),
            );
            if !self.seen.insert(key) {
                continue;
            }
            number += 1;
            let stem = format!("{name}-{number}");
            if let Some(jpeg) = convert_raster::paint::jpeg_bytes(image, &doc.source) {
                self.files.push((format!("{stem}.jpg"), jpeg.to_vec()));
                self.copied += 1;
                continue;
            }
            match convert_raster::paint::pixels(image) {
                Some(pixels) => {
                    let bytes = self.encode(&pixels, None)?;
                    self.files
                        .push((format!("{stem}.{}", self.format.extension()), bytes));
                    self.decoded += 1;
                }
                None => self.notes.push(format!(
                    "page {}: a picture could not be decoded and was left out",
                    page + 1
                )),
            }
        }
        Ok(())
    }
}

impl Job for Work {
    fn total(&self) -> usize {
        self.steps.len()
    }

    fn step(&mut self) -> Result<(), String> {
        let Some(&(document, page)) = self.steps.get(self.next) else {
            return Ok(());
        };
        self.next += 1;
        if let Err(why) = self.page(document, page) {
            self.notes.push(why);
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Outcome, String> {
        let mut notes = self.notes;
        if self.extract {
            notes.push(format!(
                "{} pictures: {} JPEG copied as stored, {} decoded",
                self.copied + self.decoded,
                self.copied,
                self.decoded
            ));
            if self.files.is_empty() {
                notes.push("no pictures were found on these pages".to_owned());
            }
        } else {
            let pages: usize = self.documents.iter().map(|d| d.pages.len()).sum();
            if self.files.len() < pages {
                notes.push(format!(
                    "{} of {pages} pages could not be drawn",
                    pages - self.files.len()
                ));
            }
        }
        Ok(Outcome {
            files: self.files,
            notes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pdf_with_picture() -> Vec<u8> {
        let content = b"q 100 0 0 50 0 0 cm /Im1 Do Q";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 50] /Contents 4 0 R /Resources << /XObject << /Im1 5 0 R >> >> >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), String::from_utf8_lossy(content)),
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 6 >>\nstream\n\u{ff}\u{0}\u{0}\u{0}\u{0}\u{ff}\nendstream".to_owned(),
        ];
        let mut pdf: Vec<u8> = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            let bytes: Vec<u8> = body
                .chars()
                .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
                .collect();
            pdf.extend_from_slice(&bytes);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let start = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for o in offsets {
            pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn run(settings: &str) -> Outcome {
        convert_files::run_to_end(
            start,
            vec![Input {
                name: "doc.pdf".into(),
                bytes: pdf_with_picture(),
            }],
            &Settings::parse(settings),
        )
        .unwrap()
    }

    #[test]
    fn a_page_becomes_a_jpeg_of_its_size() {
        let out = run("dpi=144");
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.files[0].0, "doc-1.jpg");
        let back = pdf_paint::picture::jpeg_pixels(&out.files[0].1).unwrap();
        assert_eq!((back.width, back.height), (200, 100));
        let left = &back.samples[(50 * 200 + 50) * 3..][..3];
        assert!(left[0] > 200 && left[2] < 60, "{left:?}");
    }

    #[test]
    fn pictures_are_extracted_in_their_own_grid() {
        let out = run("mode=extract");
        assert_eq!(out.files.len(), 1, "{:?}", out.notes);
        assert_eq!(out.files[0].0, "doc-1-1.png");
    }
}
