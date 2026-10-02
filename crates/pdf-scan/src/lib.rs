#![forbid(unsafe_code)]

mod find;
mod flatten;
pub mod net;
pub mod quad;
mod shape;
mod snap;

pub use find::{Corners, find_sheet, find_sheet_in_colour};
pub use flatten::{clean, enhance, flatten, flattened_size, page_size, rgba_of};
pub use shape::{aspect_ratio, true_size};
pub use snap::snap_to_edges;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grey {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Rgba {
    #[must_use]
    pub fn grey(&self) -> Grey {
        let pixels = self
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                let (r, g, b) = (u32::from(p[0]), u32::from(p[1]), u32::from(p[2]));
                u8::try_from((r * 77 + g * 150 + b * 29) >> 8).unwrap_or(u8::MAX)
            })
            .collect();
        Grey {
            width: self.width,
            height: self.height,
            pixels,
        }
    }
}

#[must_use]
pub fn corners_in_photo(picture: &Rgba) -> Option<Corners> {
    let step = picture.width.max(picture.height).div_ceil(FOUND_IN).max(1);
    let small = picture.shrunk(step);
    #[allow(clippy::cast_precision_loss)]
    find_sheet_best(&small)
        .map(|c| c.map(|(x, y)| (x * step as f32, y * step as f32)))
        .map(|c| snap_to_edges(&picture.grey(), &c))
}

#[must_use]
pub fn find_sheet_best(picture: &Rgba) -> Option<Corners> {
    quad::carried()
        .and_then(|net| net.look_all_ways(picture))
        .filter(|found| found.sureness >= quad::SURE)
        .map(|found| found.corners)
        .or_else(|| net::find_sheet_best(picture))
}

pub const FOUND_IN: usize = 960;

impl Rgba {
    #[must_use]
    pub fn shrunk(&self, step: usize) -> Self {
        let step = step.max(1);
        let (w, h) = (self.width / step, self.height / step);
        let mut pixels = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0_u32; 3];
                for dy in 0..step {
                    let row = (y * step + dy) * self.width;
                    for dx in 0..step {
                        let at = (row + x * step + dx) * 4;
                        for (c, total) in sum.iter_mut().enumerate() {
                            *total += u32::from(self.pixels[at + c]);
                        }
                    }
                }
                #[allow(clippy::cast_possible_truncation)]
                let n = (step * step) as u32;
                for total in sum {
                    pixels.push(u8::try_from(total / n).unwrap_or(u8::MAX));
                }
                pixels.push(255);
            }
        }
        Self {
            width: w,
            height: h,
            pixels,
        }
    }
}
