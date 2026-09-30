use std::sync::Arc;

use convert_files::{Input, Job, Outcome, Settings};
use convert_raster::scan::{Look, clean};
use image_to_pdf::{Builder, Layout};

pub use convert_files;

const MOST_SIDE: u32 = 3508;

struct Work {
    inputs: Vec<Input>,
    next: usize,
    look: Look,
    crop: bool,
    quality: u8,
    builder: Builder,
    notes: Vec<String>,
    first_stem: String,
}

pub fn start(inputs: Vec<Input>, settings: &Settings) -> Result<Box<dyn Job>, String> {
    if inputs.is_empty() {
        return Err("no photograph was given".to_owned());
    }
    let look = match settings.text("look", "colour") {
        "colour" | "color" => Look::Colour,
        "grey" | "gray" => Look::Grey,
        "bw" | "black-white" => Look::BlackWhite,
        "original" | "none" => Look::Original,
        other => {
            return Err(format!(
                "look: '{other}' is not colour, grey, bw or original"
            ));
        }
    };
    let crop = !matches!(settings.get("crop"), Some("false" | "0" | "no" | "off"));
    let quality = settings.number("quality", 80.0)?;
    if !(1.0..=100.0).contains(&quality) {
        return Err(format!("quality: {quality} is not between 1 and 100"));
    }
    let layout = Layout::from_settings(settings)?;
    let first_stem = inputs[0].stem().to_owned();
    Ok(Box::new(Work {
        inputs,
        next: 0,
        look,
        crop,
        quality: convert_raster::to_u8(quality),
        builder: Builder::new(layout),
        notes: Vec::new(),
        first_stem,
    }))
}

convert_files::export_files_tool!(crate::start);

impl Work {
    fn page(&mut self, bytes: &[u8]) -> Result<String, String> {
        let photo = convert_raster::decode(bytes)?;
        let (mut page, said) = clean(&photo, self.look, self.crop);
        let longest = page.width.max(page.height);
        if longest > MOST_SIDE {
            let scale = f64::from(MOST_SIDE) / f64::from(longest);
            page = page.resized(
                convert_raster::to_u32((f64::from(page.width) * scale).round()),
                convert_raster::to_u32((f64::from(page.height) * scale).round()),
            );
        }
        if self.look != Look::Colour || page.is_grey() {
            page = page.grey();
        }
        let jpeg = page.to_jpeg(self.quality)?;
        self.builder.add(Arc::from(jpeg))?;
        Ok(said)
    }
}

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
        let bytes = std::mem::take(&mut input.bytes);
        match self.page(&bytes) {
            Ok(said) => self.notes.push(format!("{name}: {said}")),
            Err(why) => self.notes.push(format!("{name}: left out: {why}")),
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Outcome, String> {
        let mut notes = self.notes;
        let pages = self.builder.pages();
        if pages == 0 {
            return Err(notes.join("; "));
        }
        let pdf = self.builder.finish()?;
        notes.push(format!("{pages} pages"));
        Ok(Outcome {
            files: vec![(format!("{}.pdf", self.first_stem), pdf)],
            notes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convert_raster::Image;

    #[test]
    fn a_photo_of_a_page_on_a_desk_becomes_a_page() {
        let (w, h) = (600u32, 450u32);
        let mut photo = Image::filled(w, h, 3, 50);
        for y in 60..400 {
            for x in 150..450 {
                let at = ((y * w + x) * 3) as usize;
                let ink = (y % 40 < 4) && (170..430).contains(&x);
                let v = if ink { 20 } else { 225 };
                photo.data[at..at + 3].copy_from_slice(&[v, v, v - 10]);
            }
        }
        let jpeg = photo.to_jpeg(90).unwrap();
        let out = convert_files::run_to_end(
            start,
            vec![Input {
                name: "desk.jpg".into(),
                bytes: jpeg,
            }],
            &Settings::parse("look=grey"),
        )
        .unwrap();
        assert_eq!(out.files.len(), 1);
        assert!(
            out.notes.iter().any(|n| n.contains("page found")),
            "{:?}",
            out.notes
        );
    }
}
