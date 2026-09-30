use std::collections::HashSet;

use convert_files::{Input, Job, Outcome, Settings, human_size};
use convert_pdfdoc::{Doc, Load, Object, Value};
use pdf_bytes::{ByteStore, SourceId};
use pdf_paint::ColorSpace;
#[cfg(test)]
use pdf_paint::PaintAtomKind;

pub use convert_files;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub name: &'static str,
    pub dpi: Option<f64>,
    pub quality: u8,
}

pub const LOW: Level = Level {
    name: "low",
    dpi: None,
    quality: 0,
};
pub const RECOMMENDED: Level = Level {
    name: "recommended",
    dpi: Some(150.0),
    quality: 75,
};
pub const EXTREME: Level = Level {
    name: "extreme",
    dpi: Some(100.0),
    quality: 50,
};

struct Work {
    name: String,
    stem: String,
    original: Vec<u8>,
    source: ByteStore,
    document: Doc,
    level: Level,
    password: Vec<u8>,
    pages: usize,
    next: usize,
    seen: HashSet<u32>,
    pictures: usize,
    notes: Vec<String>,
}

pub fn start(mut inputs: Vec<Input>, settings: &Settings) -> Result<Box<dyn Job>, String> {
    if inputs.len() != 1 {
        return Err("compress-pdf takes one PDF at a time".to_owned());
    }
    let input = inputs.remove(0);
    let level = match settings.text("level", "recommended") {
        "low" | "less" => LOW,
        "recommended" | "medium" => RECOMMENDED,
        "extreme" | "high" => EXTREME,
        other => {
            return Err(format!(
                "level: '{other}' is not low, recommended or extreme"
            ));
        }
    };
    let password = settings.text("password", "").as_bytes().to_vec();
    let mut seed = convert_files::take_random();
    if !seed.is_empty() {
        let seeded = convert_pdfdoc::seed_random(&seed);
        seed.fill(0);
        seeded.map_err(|e| format!("the random bytes handed over were refused ({e})"))?;
    }
    let document = convert_pdfdoc::load(
        input.bytes.clone(),
        &Load {
            password: password.clone(),
            recover: true,
            everything: false,
        },
    )
    .map_err(|e| e.to_string())?;
    document.can_protect()?;
    let pages = document.pages().len();
    let source = ByteStore::owning(SourceId::next_document(), input.bytes.clone());
    Ok(Box::new(Work {
        stem: input.stem().to_owned(),
        name: input.name,
        original: input.bytes,
        source,
        document,
        level,
        password,
        pages: if level.dpi.is_some() { pages } else { 0 },
        next: 0,
        seen: HashSet::new(),
        pictures: 0,
        notes: Vec::new(),
    }))
}

convert_files::export_files_tool!(crate::start);

fn space_for(original: &ColorSpace, channels: usize, kept: Option<&Value>) -> Value {
    let same = match original {
        ColorSpace::DeviceGray | ColorSpace::CalGray(_) => channels == 1,
        ColorSpace::DeviceRgb | ColorSpace::CalRgb(_) => channels == 3,
        ColorSpace::IccBased(icc) => icc.components.value == channels,
        _ => false,
    };
    match (same, kept) {
        (true, Some(value)) => value.clone(),
        _ if channels == 1 => Value::name("DeviceGray"),
        _ => Value::name("DeviceRGB"),
    }
}

impl Work {
    fn page(&mut self, index: usize) -> Result<(), String> {
        let Some(level_dpi) = self.level.dpi else {
            return Ok(());
        };
        let view = pdf_session::interpret_page_for_display(
            &self.source,
            index,
            &self.password,
            None,
            None,
        )
        .map_err(|e| format!("page {}: {e}", index + 1))?;
        for image in convert_raster::paint::images(&view.graph) {
            let id = image.reference.object_number();
            if !self.seen.insert(id) {
                continue;
            }
            let Some(object) = self.document.get(id) else {
                continue;
            };
            let (Some(dict), Some(data)) = (object.dict(), object.stream.as_ref()) else {
                continue;
            };
            let Some(space) = image.color_space.as_ref().map(|s| &s.value) else {
                continue;
            };
            let plain = matches!(
                space,
                ColorSpace::DeviceGray
                    | ColorSpace::DeviceRgb
                    | ColorSpace::DeviceCmyk
                    | ColorSpace::CalGray(_)
                    | ColorSpace::CalRgb(_)
                    | ColorSpace::IccBased(_)
            ) && matches!(image.components(), 1 | 3 | 4);
            if image.image_mask.value
                || !plain
                || image.bits_per_component.value < 8
                || dict.has("Mask")
            {
                continue;
            }
            let (w, h) = (image.width.value, image.height.value);
            let m = &image.state.ctm.value;
            let inches = (m.a.hypot(m.b) / 72.0, m.c.hypot(m.d) / 72.0);
            if inches.0 <= 1e-6 || inches.1 <= 1e-6 {
                continue;
            }
            let dpi = (f64::from(w) / inches.0).min(f64::from(h) / inches.1);
            let factor = (level_dpi / dpi).min(1.0);
            let is_jpeg = last_filter(dict).is_some_and(|f| f == b"DCTDecode");
            if is_jpeg && factor > 0.85 {
                continue;
            }
            if factor >= 0.95 && data.len() < 32 * 1024 {
                continue;
            }
            if image.components() == 4
                && image.soft_mask.as_ref().is_some_and(|m| m.matte.is_some())
            {
                continue;
            }
            let Some(mut pixels) = convert_raster::paint::pixels(image) else {
                continue;
            };
            if factor < 0.95 {
                pixels = pixels.resized(
                    convert_raster::to_u32((f64::from(w) * factor).round()).max(1),
                    convert_raster::to_u32((f64::from(h) * factor).round()).max(1),
                );
            }
            let Ok(jpeg) = pixels.to_jpeg(self.level.quality) else {
                continue;
            };
            if jpeg.len() * 10 > data.len() * 9 {
                continue;
            }
            let mut dict = dict.clone();
            let colour = space_for(space, pixels.channels, dict.get("ColorSpace"));
            dict.set("Width", Value::Int(i64::from(pixels.width)));
            dict.set("Height", Value::Int(i64::from(pixels.height)));
            dict.set("BitsPerComponent", Value::Int(8));
            dict.set("ColorSpace", colour);
            dict.set("Filter", Value::name("DCTDecode"));
            dict.remove("DecodeParms");
            dict.remove("Decode");
            self.document.objects.insert(id, Object::stream(dict, jpeg));
            self.pictures += 1;
        }
        Ok(())
    }
}

fn last_filter(dict: &convert_pdfdoc::Dict) -> Option<&[u8]> {
    match dict.get("Filter")? {
        Value::Name(name) => Some(name),
        Value::Array(items) => match items.last()? {
            Value::Name(name) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

fn tidy(document: &mut Doc, drop_private: bool) -> usize {
    let mut deflated = 0;
    if drop_private {
        for page in document.pages() {
            if let Some(dict) = document.get_mut(page.number).and_then(Object::dict_mut) {
                dict.remove("Thumb");
            }
        }
        for object in document.objects.values_mut() {
            if let Some(dict) = object.dict_mut() {
                dict.remove("PieceInfo");
            }
        }
    }
    for object in document.objects.values_mut() {
        let Some(data) = object.stream.as_mut() else {
            continue;
        };
        let Some(dict) = object.value.as_dict_mut() else {
            continue;
        };
        if dict.has("Filter") || data.len() < 64 {
            continue;
        }
        let packed = convert_pdfdoc::deflate(data);
        if packed.len() < data.len() {
            *data = packed;
            dict.set("Filter", Value::name("FlateDecode"));
            deflated += 1;
        }
    }
    deflated
}

impl Job for Work {
    fn total(&self) -> usize {
        self.pages
    }

    fn step(&mut self) -> Result<(), String> {
        let index = self.next;
        self.next += 1;
        if let Err(why) = self.page(index) {
            self.notes
                .push(format!("{why}; its pictures were left as they are"));
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<Outcome, String> {
        let deflated = tidy(&mut self.document, self.level.dpi.is_some());
        let merged = self.document.dedupe();
        let protection = self
            .document
            .kept
            .as_ref()
            .map(|kept| convert_pdfdoc::Protection {
                dictionary: &kept.dictionary,
                security: &kept.security,
            });
        let written = convert_pdfdoc::write(&self.document, protection.as_ref(), true)?.bytes;
        let before = self.original.len();
        let mut notes = self.notes;
        notes.extend(self.document.notes.iter().take(5).cloned());
        let (bytes, after) = if written.len() < before {
            let after = written.len();
            (written, after)
        } else {
            notes.push(
                "the file is already as small as this level makes it; it is returned as it was"
                    .to_owned(),
            );
            (self.original, before)
        };
        let saved = 100.0 * (1.0 - after as f64 / before.max(1) as f64);
        notes.push(format!(
            "{}: {} -> {} ({saved:.1}% smaller), level {}: {} pictures re-encoded, {} duplicate objects merged, {} streams deflated",
            self.name,
            human_size(before),
            human_size(after),
            self.level.name,
            self.pictures,
            merged,
            deflated
        ));
        Ok(Outcome {
            files: vec![(format!("{}-compressed.pdf", self.stem), bytes)],
            notes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heavy() -> Vec<u8> {
        let (w, h) = (600usize, 600usize);
        let mut samples = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                samples.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 100]);
            }
        }
        let content = b"q 72 0 0 72 100 100 cm /Im1 Do Q";
        let mut pdf: Vec<u8> = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        let mut object = |pdf: &mut Vec<u8>, body: &[u8]| {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
            pdf.extend_from_slice(body);
            pdf.extend_from_slice(b"\nendobj\n");
        };
        object(&mut pdf, b"<< /Type /Catalog /Pages 2 0 R >>");
        object(&mut pdf, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        object(&mut pdf, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R /Resources << /XObject << /Im1 5 0 R >> >> >>");
        let mut body = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        body.extend_from_slice(content);
        body.extend_from_slice(b"\nendstream");
        object(&mut pdf, &body);
        let mut body = format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n", samples.len()).into_bytes();
        body.extend_from_slice(&samples);
        body.extend_from_slice(b"\nendstream");
        object(&mut pdf, &body);
        let start = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
        );
        for o in &offsets {
            pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
                offsets.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn compress(level: &str) -> Outcome {
        convert_files::run_to_end(
            start,
            vec![Input {
                name: "heavy.pdf".into(),
                bytes: heavy(),
            }],
            &Settings::parse(&format!("level={level}")),
        )
        .unwrap()
    }

    #[test]
    fn each_level_is_smaller_than_the_one_before() {
        let before = heavy().len();
        let low = compress("low").files[0].1.len();
        let recommended = compress("recommended").files[0].1.len();
        let extreme = compress("extreme").files[0].1.len();
        assert!(low < before, "low {low} of {before}");
        assert!(
            recommended < low / 5,
            "recommended {recommended}, low {low}"
        );
        assert!(
            extreme < recommended,
            "extreme {extreme}, recommended {recommended}"
        );
        let out = compress("recommended").files.remove(0).1;
        let source = ByteStore::owning(SourceId::next_document(), out);
        let view = pdf_session::interpret_page_for_display(&source, 0, b"", None, None).unwrap();
        let images: Vec<_> = view
            .graph
            .atoms
            .iter()
            .filter_map(|a| match &a.kind {
                PaintAtomKind::Image(i) => Some((i.width.value, i.height.value)),
                _ => None,
            })
            .collect();
        assert_eq!(images, vec![(150, 150)]);
    }
}
