use std::sync::Arc;

use convert_files::{Input, Job, Outcome, Settings};
use pdf_bytes::{ByteStore, SourceId};
use pdf_edit::Command;
use pdf_edit::image_file::ImageFile;
use pdf_paint::Matrix;
use pdf_session::Session;

pub use convert_files;

pub const A4: [f64; 2] = [595.28, 841.89];
pub const LETTER: [f64; 2] = [612.0, 792.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub paper: Option<[f64; 2]>,
    pub landscape: Option<bool>,
    pub margin: f64,
}

impl Layout {
    pub fn from_settings(settings: &Settings) -> Result<Self, String> {
        let paper = match settings.text("size", "a4").to_ascii_lowercase().as_str() {
            "a4" => Some(A4),
            "letter" | "us-letter" => Some(LETTER),
            "fit" | "same" | "picture" => None,
            other => return Err(format!("size: '{other}' is not a4, letter or fit")),
        };
        let landscape = match settings.text("orientation", "auto") {
            "auto" => None,
            "portrait" => Some(false),
            "landscape" => Some(true),
            other => {
                return Err(format!(
                    "orientation: '{other}' is not auto, portrait or landscape"
                ));
            }
        };
        let margin = match settings.text("margin", "none") {
            "none" | "0" => 0.0,
            "small" => 20.0,
            "big" | "large" => 50.0,
            other => other
                .parse::<f64>()
                .ok()
                .filter(|m| (0.0..=200.0).contains(m))
                .ok_or_else(|| format!("margin: '{other}' is not none, small, big or points"))?,
        };
        Ok(Self {
            paper,
            landscape,
            margin,
        })
    }

    #[must_use]
    pub fn place(&self, size: [f64; 2]) -> ([f64; 2], [f64; 4]) {
        let m = self.margin;
        let Some(paper) = self.paper else {
            let page = [size[0] + 2.0 * m, size[1] + 2.0 * m];
            return (page, [m, m, size[0], size[1]]);
        };
        let wide = self.landscape.unwrap_or(size[0] > size[1]);
        let page = if wide { [paper[1], paper[0]] } else { paper };
        let room = [(page[0] - 2.0 * m).max(1.0), (page[1] - 2.0 * m).max(1.0)];
        let scale = (room[0] / size[0]).min(room[1] / size[1]);
        let (w, h) = (size[0] * scale, size[1] * scale);
        (page, [(page[0] - w) / 2.0, (page[1] - h) / 2.0, w, h])
    }
}

pub struct Builder {
    layout: Layout,
    session: Option<Session>,
    pages: usize,
}

impl Builder {
    #[must_use]
    pub const fn new(layout: Layout) -> Self {
        Self {
            layout,
            session: None,
            pages: 0,
        }
    }

    pub fn add(&mut self, picture: Arc<[u8]>) -> Result<(), String> {
        let picture = if convert_gif::Gif::is_gif(&picture) {
            let gif = convert_gif::Gif::read(&picture).map_err(|e| e.to_string())?;
            Arc::from(gif.to_png())
        } else {
            picture
        };
        ImageFile::read(&picture).map_err(|e| e.reason().to_owned())?;
        let size = pdf_session::pictures::page_for(&picture).map_err(|e| e.to_string())?;
        let (page, [x, y, w, h]) = self.layout.place(size);
        let placement = Matrix {
            a: w,
            b: 0.0,
            c: 0.0,
            d: h,
            e: x,
            f: y,
        };
        let mut commands = Vec::with_capacity(2);
        if let Some(session) = self.session.as_mut() {
            commands.push(Command::AddBlankPage {
                beside: self.pages - 1,
                before: false,
                size: page,
            });
            commands.push(Command::PlaceNewImage {
                page_index: self.pages,
                placement,
                file: picture,
            });
            session.apply_each(&commands).map_err(|e| e.to_string())?;
        } else {
            let blank = pdf_session::blank_document(page).map_err(|e| e.to_string())?;
            let mut session =
                Session::new(ByteStore::owning(SourceId::next_document(), blank), b"");
            session
                .apply_each(&[Command::PlaceNewImage {
                    page_index: 0,
                    placement,
                    file: picture,
                }])
                .map_err(|e| e.to_string())?;
            self.session = Some(session);
        }
        self.pages += 1;
        Ok(())
    }

    #[must_use]
    pub const fn pages(&self) -> usize {
        self.pages
    }

    pub fn finish(self) -> Result<Vec<u8>, String> {
        let session = self
            .session
            .ok_or_else(|| "no picture was made into a page".to_owned())?;
        let built = session.source().to_vec();
        let document = convert_pdfdoc::load(
            built,
            &convert_pdfdoc::Load {
                recover: true,
                ..convert_pdfdoc::Load::default()
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(convert_pdfdoc::write(&document, None, true)?.bytes)
    }
}

struct Work {
    inputs: Vec<Input>,
    next: usize,
    merge: bool,
    layout: Layout,
    builder: Builder,
    files: Vec<(String, Vec<u8>)>,
    notes: Vec<String>,
    first_stem: String,
}

pub fn start(inputs: Vec<Input>, settings: &Settings) -> Result<Box<dyn Job>, String> {
    if inputs.is_empty() {
        return Err("no picture was given".to_owned());
    }
    let layout = Layout::from_settings(settings)?;
    let merge = !matches!(settings.get("merge"), Some("false" | "0" | "no" | "off"));
    let first_stem = inputs[0].stem().to_owned();
    Ok(Box::new(Work {
        inputs,
        next: 0,
        merge,
        layout,
        builder: Builder::new(layout),
        files: Vec::new(),
        notes: Vec::new(),
        first_stem,
    }))
}

#[cfg(feature = "browser")]
convert_files::export_files_tool!(crate::start);

impl Job for Work {
    fn total(&self) -> usize {
        self.inputs.len()
    }

    fn step(&mut self) -> Result<(), String> {
        let Some(input) = self.inputs.get_mut(self.next) else {
            return Ok(());
        };
        self.next += 1;
        let name = input.name.clone();
        let stem = input.stem().to_owned();
        let bytes: Arc<[u8]> = Arc::from(std::mem::take(&mut input.bytes));
        if convert_gif::Gif::is_gif(&bytes)
            && convert_gif::Gif::read(&bytes).is_ok_and(|g| g.frames > 1)
        {
            self.notes
                .push(format!("{name}: an animation; its first frame is used"));
        }
        if self.merge {
            if let Err(why) = self.builder.add(bytes) {
                self.notes.push(format!("{name}: left out: {why}"));
            }
        } else {
            let mut one = Builder::new(self.layout);
            match one.add(bytes).and_then(|()| one.finish()) {
                Ok(pdf) => self.files.push((format!("{stem}.pdf"), pdf)),
                Err(why) => self.notes.push(format!("{name}: left out: {why}")),
            }
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Outcome, String> {
        let mut files = self.files;
        let mut notes = self.notes;
        if self.merge {
            let pages = self.builder.pages();
            let pdf = self.builder.finish().map_err(|why| {
                if notes.is_empty() {
                    why
                } else {
                    notes.join("; ")
                }
            })?;
            notes.push(format!("{pages} pages"));
            files.push((format!("{}.pdf", self.first_stem), pdf));
        } else if files.is_empty() {
            return Err(notes.join("; "));
        }
        Ok(Outcome { files, notes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let pixels: Vec<u8> = (0..width * height * 3).map(|i| (i % 251) as u8).collect();
        pdf_edit::png::write((width, height), &pixels, None).unwrap()
    }

    fn tiny_gif() -> Vec<u8> {
        let mut gif = b"GIF89a\x28\x00\x28\x00\x80\x00\x00".to_vec();
        gif.extend_from_slice(&[255, 0, 0, 0, 0, 0]);
        gif.extend_from_slice(&[0x21, 0xF9, 4, 1, 0, 0, 1, 0]);
        gif.extend_from_slice(&[0x2C, 0, 0, 0, 0, 2, 0, 1, 0, 0]);
        gif.extend_from_slice(&[2, 2, 0x44, 0x0A, 0, 0x3B]);
        gif
    }

    #[test]
    fn a_gif_is_read_as_its_first_frame() {
        let gif = convert_gif::Gif::read(&tiny_gif()).unwrap();
        assert_eq!((gif.width, gif.height), (40, 40));
        assert_eq!(&gif.indices[..3], [0, 1, 1]);
        assert_eq!(gif.transparent, Some(1));
        let png = gif.to_png();
        assert!(ImageFile::read(&png).is_ok());
    }

    #[test]
    fn a_wide_picture_goes_on_a_landscape_page_centred() {
        let layout = Layout {
            paper: Some(A4),
            landscape: None,
            margin: 0.0,
        };
        let (page, [x, y, w, h]) = layout.place([200.0, 100.0]);
        assert_eq!(page, [A4[1], A4[0]]);
        assert!((w - A4[1]).abs() < 1e-6 && (h - A4[1] / 2.0).abs() < 1e-6);
        assert!(x.abs() < 1e-6 && (y - (A4[0] - h) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn pictures_become_pages() {
        let jpeg = convert_raster::Image::filled(300, 200, 3, 90)
            .to_jpeg(80)
            .unwrap();
        let out = convert_files::run_to_end(
            start,
            vec![
                Input {
                    name: "a.png".into(),
                    bytes: png(40, 80),
                },
                Input {
                    name: "b.jpg".into(),
                    bytes: jpeg.clone(),
                },
                Input {
                    name: "c.txt".into(),
                    bytes: b"not a picture".to_vec(),
                },
                Input {
                    name: "d.gif".into(),
                    bytes: tiny_gif(),
                },
            ],
            &Settings::parse("size=a4\nmargin=small"),
        )
        .unwrap();
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.files[0].0, "a.pdf");
        let pdf = &out.files[0].1;
        let store = ByteStore::owning(SourceId::next_document(), pdf.clone());
        let pages = pdf_content::page_geometries_with_password(
            &store,
            pdf_content::PageContentLimits::default(),
            b"",
        )
        .unwrap();
        assert_eq!(pages.len(), 3);
        assert!(pdf.windows(jpeg.len()).any(|w| w == jpeg.as_slice()));
        assert!(out.notes.iter().any(|n| n.starts_with("c.txt: left out")));
    }
}
