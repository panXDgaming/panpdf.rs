use std::sync::Arc;

use convert_files::{Input, Job, Outcome, Settings};
use convert_raster::Image;
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
        let found = if self.crop {
            scanned(&photo, self.look)
        } else {
            None
        };
        let found = found.or_else(|| {
            if self.crop {
                whole(&photo, self.look)
            } else {
                None
            }
        });
        let (mut page, said) = found.unwrap_or_else(|| clean(&photo, self.look, false));
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

fn scanned(photo: &Image, look: Look) -> Option<(Image, String)> {
    let picture = rgba_of(photo);
    let (corners, finder) = choose_sheet(photo, &picture)?;
    let (w, h) = pdf_scan::true_size(&corners, (picture.width, picture.height));
    let flat = pdf_scan::flatten(&picture, &corners, at_most(w, h));
    drop(picture);
    let page = cleaned(&flat, look)?;
    let said = format!(
        "page found ({finder}), cut out at {} x {}",
        page.width, page.height
    );
    Some((page, said))
}

fn whole(photo: &Image, look: Look) -> Option<(Image, String)> {
    let angle = convert_raster::scan::skew(photo);
    let upright = if angle == 0.0 {
        photo.clone()
    } else {
        photo.turned(-angle, 255)
    };
    let (w, h) = at_most(upright.width as usize, upright.height as usize);
    let upright = if (w, h) == (upright.width as usize, upright.height as usize) {
        upright
    } else {
        upright.resized(u32::try_from(w).ok()?, u32::try_from(h).ok()?)
    };
    let page = cleaned(&rgba_of(&upright), look)?;
    let said = if angle == 0.0 {
        "the picture is the page".to_owned()
    } else {
        format!("the picture is the page, straightened by {angle:.2} degrees")
    };
    Some((page, said))
}

fn at_most(w: usize, h: usize) -> (usize, usize) {
    let longest = w.max(h).max(1);
    let most = MOST_SIDE as usize;
    if longest > most {
        ((w * most / longest).max(1), (h * most / longest).max(1))
    } else {
        (w.max(1), h.max(1))
    }
}

fn cleaned(flat: &pdf_scan::Rgba, look: Look) -> Option<Image> {
    let page = match look {
        Look::Colour | Look::Grey => pdf_scan::enhance(flat),
        Look::BlackWhite => pdf_scan::clean(flat),
        Look::Original => flat.clone(),
    };
    Some(Image {
        width: u32::try_from(page.width).ok()?,
        height: u32::try_from(page.height).ok()?,
        channels: 3,
        data: page
            .pixels
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect(),
    })
}

pub const EARLY: f32 = 0.9;

pub const SUPPORTED: f32 = 0.5;

#[must_use]
pub fn choose_sheet(
    photo: &Image,
    picture: &pdf_scan::Rgba,
) -> Option<(pdf_scan::Corners, &'static str)> {
    let (found, step) = candidates(photo, picture);
    let (_, corners, finder) = found
        .into_iter()
        .filter(|(support, ..)| *support >= SUPPORTED)
        .max_by(|a, b| a.0.total_cmp(&b.0))?;
    #[allow(clippy::cast_precision_loss)]
    let corners = corners.map(|(x, y)| (x * step as f32, y * step as f32));
    Some((pdf_scan::snap_to_edges(&picture.grey(), &corners), finder))
}

#[must_use]
pub fn candidates(
    photo: &Image,
    picture: &pdf_scan::Rgba,
) -> (Vec<(f32, pdf_scan::Corners, &'static str)>, usize) {
    let (small, step) = shrunk(picture);
    #[allow(clippy::cast_precision_loss)]
    let scale = step as f32;
    let mut found: Vec<(pdf_scan::Corners, &'static str)> = Vec::new();
    if let Some(c) = docquad(&small, EARLY) {
        found.push((c, "DocQuadNet"));
    }
    if let Some(c) = pdf_scan::net::find_sheet_best(&small) {
        found.push((c, "network"));
    }
    #[allow(clippy::cast_possible_truncation)]
    if let Some(q) = convert_raster::scan::find_page(photo) {
        found.push((
            q.map(|(x, y)| (x as f32 / scale, y as f32 / scale)),
            "edges",
        ));
    }
    let found = found
        .into_iter()
        .map(|(c, finder)| {
            let c = clockwise(c);
            (edge_support(&small, &c), c, finder)
        })
        .collect();
    (found, step)
}

fn shrunk(picture: &pdf_scan::Rgba) -> (pdf_scan::Rgba, usize) {
    let step = picture
        .width
        .max(picture.height)
        .div_ceil(pdf_scan::FOUND_IN)
        .max(1);
    (picture.shrunk(step), step)
}

fn docquad(small: &pdf_scan::Rgba, early: f32) -> Option<pdf_scan::Corners> {
    pdf_scan::quad::carried()
        .and_then(|net| {
            let upright = net.look(small, 0);
            if upright.as_ref().is_some_and(|f| f.sureness >= early) {
                return upright;
            }
            (1..4)
                .filter_map(|turns| net.look(small, turns))
                .chain(upright)
                .max_by(|a, b| a.sureness.total_cmp(&b.sureness))
        })
        .filter(|found| found.sureness >= pdf_scan::quad::SURE)
        .map(|found| found.corners)
}

#[must_use]
pub fn sheet_corners(picture: &pdf_scan::Rgba, early: f32) -> Option<pdf_scan::Corners> {
    let (small, step) = shrunk(picture);
    #[allow(clippy::cast_precision_loss)]
    docquad(&small, early)
        .or_else(|| pdf_scan::net::find_sheet_best(&small))
        .map(|c| c.map(|(x, y)| (x * step as f32, y * step as f32)))
        .map(|c| pdf_scan::snap_to_edges(&picture.grey(), &c))
}

fn clockwise(corners: pdf_scan::Corners) -> pdf_scan::Corners {
    let (mx, my) = corners
        .iter()
        .fold((0.0, 0.0), |(x, y), c| (x + c.0 / 4.0, y + c.1 / 4.0));
    let mut sorted = corners;
    sorted.sort_by(|a, b| {
        (a.1 - my)
            .atan2(a.0 - mx)
            .total_cmp(&(b.1 - my).atan2(b.0 - mx))
    });
    let first = (0..4)
        .min_by(|&a, &b| (sorted[a].0 + sorted[a].1).total_cmp(&(sorted[b].0 + sorted[b].1)))
        .unwrap_or(0);
    std::array::from_fn(|i| sorted[(first + i) % 4])
}

#[must_use]
pub fn edge_support(picture: &pdf_scan::Rgba, corners: &pdf_scan::Corners) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let (w, h) = (picture.width as f32, picture.height as f32);
    let diagonal = w.hypot(h);
    let area = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f32>()
        / 2.0;
    let convex = (0..4).all(|i| {
        let (a, b, c) = (corners[i], corners[(i + 1) % 4], corners[(i + 2) % 4]);
        (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0) > 0.0
    });
    if !convex || area < 0.1 * w * h {
        return 0.0;
    }
    let reach = (0.015 * diagonal).max(3.0);
    let border = 0.02 * diagonal;
    let on_border = |a: (f32, f32), b: (f32, f32)| {
        (a.0 < border && b.0 < border)
            || (a.0 > w - border && b.0 > w - border)
            || (a.1 < border && b.1 < border)
            || (a.1 > h - border && b.1 > h - border)
    };
    let patch = |x: f32, y: f32| -> Option<([f32; 3], f32)> {
        if x < 2.0 || y < 2.0 || x >= w - 2.0 || y >= h - 2.0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (cx, cy) = (x as usize, y as usize);
        let mut sum = [0.0_f32; 3];
        let (mut darkest, mut lightest) = (f32::MAX, f32::MIN);
        for py in cy - 2..=cy + 2 {
            for px in cx - 2..=cx + 2 {
                let at = (py * picture.width + px) * 4;
                let p = &picture.pixels[at..at + 3];
                for (s, v) in sum.iter_mut().zip(p) {
                    *s += f32::from(*v) / 25.0;
                }
                let grey =
                    0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2]);
                darkest = darkest.min(grey);
                lightest = lightest.max(grey);
            }
        }
        Some((sum, lightest - darkest))
    };
    let (mut total, mut sides) = (0.0_f32, 0_u32);
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        if on_border(a, b) {
            continue;
        }
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        if length < 0.05 * diagonal {
            return 0.0;
        }
        let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length);
        let mut differences: Vec<([f32; 3], f32)> = Vec::new();
        for k in 0..40 {
            #[allow(clippy::cast_precision_loss)]
            let t = 0.1 + 0.8 * (k as f32 + 0.5) / 40.0;
            let (x, y) = (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
            if let (Some((inside, spread)), Some((outside, _))) = (
                patch(x - nx * reach, y - ny * reach),
                patch(x + nx * reach, y + ny * reach),
            ) {
                differences.push((std::array::from_fn(|c| inside[c] - outside[c]), spread));
            }
        }
        if differences.len() < 10 {
            continue;
        }
        sides += 1;
        let usual: [f32; 3] = std::array::from_fn(|c| {
            let mut channel: Vec<f32> = differences.iter().map(|(d, _)| d[c]).collect();
            channel.sort_by(f32::total_cmp);
            channel[channel.len() / 2]
        });
        if usual.iter().all(|v| v.abs() < 20.0) {
            continue;
        }
        let agree = differences
            .iter()
            .filter(|(d, spread)| {
                *spread < 48.0 && d.iter().zip(&usual).all(|(v, u)| (v - u).abs() < 32.0)
            })
            .count();
        #[allow(clippy::cast_precision_loss)]
        {
            total += agree as f32 / differences.len() as f32;
        }
    }
    if sides < 2 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    {
        total / sides as f32
    }
}

fn rgba_of(image: &Image) -> pdf_scan::Rgba {
    let mut pixels = Vec::with_capacity(image.data.len() / image.channels.max(1) * 4);
    for p in image.data.chunks_exact(image.channels.max(1)) {
        match p {
            [v] | [v, _] => pixels.extend_from_slice(&[*v, *v, *v, 255]),
            [r, g, b, ..] => pixels.extend_from_slice(&[*r, *g, *b, 255]),
            [] => {}
        }
    }
    pdf_scan::Rgba {
        width: image.width as usize,
        height: image.height as usize,
        pixels,
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

    fn scanned_form() -> pdf_scan::Rgba {
        let (w, h) = (600_usize, 800_usize);
        let mut pixels = vec![250_u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let print =
                    (40..760).contains(&y) && y % 24 < 5 && (40..560).contains(&x) && x % 9 < 6;
                let rule = ((200..600).contains(&y) && (x == 120 || x == 480))
                    || ((120..=480).contains(&x) && (y == 200 || y == 600));
                if print || rule {
                    pixels[(y * w + x) * 4..(y * w + x) * 4 + 3].copy_from_slice(&[30, 30, 30]);
                }
            }
        }
        pdf_scan::Rgba {
            width: w,
            height: h,
            pixels,
        }
    }

    #[test]
    fn a_box_ruled_inside_a_page_is_not_taken_for_a_sheet() {
        let form = scanned_form();
        let ruled = [
            (120.0, 200.0),
            (480.0, 200.0),
            (480.0, 600.0),
            (120.0, 600.0),
        ];
        let support = edge_support(&form, &ruled);
        assert!(support < SUPPORTED, "{support}");
    }

    #[test]
    fn a_sheet_on_a_desk_is_borne_out_by_its_edges() {
        let (w, h) = (600_usize, 800_usize);
        let mut pixels = vec![0_u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let sheet = (100..700).contains(&y) && (100..500).contains(&x);
                let print =
                    sheet && (140..660).contains(&y) && y % 24 < 5 && (140..460).contains(&x);
                let v = if print {
                    30
                } else if sheet {
                    235
                } else {
                    70
                };
                pixels[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[v, v, v - 20, 255]);
            }
        }
        let desk = pdf_scan::Rgba {
            width: w,
            height: h,
            pixels,
        };
        let edges = [
            (100.0, 100.0),
            (500.0, 100.0),
            (500.0, 700.0),
            (100.0, 700.0),
        ];
        let support = edge_support(&desk, &edges);
        assert!(support > 0.9, "{support}");
        let inside = [
            (160.0, 160.0),
            (440.0, 160.0),
            (440.0, 640.0),
            (160.0, 640.0),
        ];
        assert!(edge_support(&desk, &inside) < SUPPORTED);
    }

    #[test]
    fn a_scanned_page_is_kept_whole() {
        let form = scanned_form();
        let photo = Image::from_rgba(600, 800, &form.pixels);
        let out = convert_files::run_to_end(
            start,
            vec![Input {
                name: "form.png".into(),
                bytes: photo.to_png(None).unwrap(),
            }],
            &Settings::parse("look=grey"),
        )
        .unwrap();
        assert!(
            out.notes
                .iter()
                .any(|n| n.contains("the picture is the page")),
            "{:?}",
            out.notes
        );
    }
}
