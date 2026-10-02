use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdf_edit::image_file::ImageFile;
use pdf_paint::Matrix;
use pdf_session::PageView;

use crate::desk;

pub const MOST_BYTES: u64 = 64 * 1024 * 1024;

pub const DEFAULT_WIDTH: f64 = 200.0;

const POINTS_A_PIXEL: f64 = 0.75;

const MOST_OF_THE_PAGE: f64 = 0.8;

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Attachment(Option<usize>),
    File(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub page: usize,
    pub left: f64,
    pub top: f64,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub source: Source,
}

pub fn read_file(path: &Path) -> Result<Arc<[u8]>, String> {
    let facts = std::fs::metadata(path)
        .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
    if !facts.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if facts.len() > MOST_BYTES {
        return Err(format!(
            "{} is too large to place: the most is {} MB",
            path.display(),
            MOST_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
    Ok(Arc::from(bytes))
}

pub fn size_of(bytes: &[u8]) -> Result<(f64, f64), String> {
    let image = ImageFile::read(bytes).map_err(|error| {
        format!(
            "this is not a picture that can be placed: {}",
            error.reason()
        )
    })?;
    let (across, down) = image.upright();
    if across == 0 || down == 0 {
        return Err("this picture has no size".to_owned());
    }
    Ok((f64::from(across), f64::from(down)))
}

pub fn fit(asked: &Asked, pixels: (f64, f64), page: [f64; 2]) -> Result<[f64; 4], String> {
    let (across, down) = pixels;
    let aspect = across / down;
    let (width, height) = match (asked.width, asked.height) {
        (Some(width), Some(height)) => {
            if width / height > aspect {
                (height * aspect, height)
            } else {
                (width, width / aspect)
            }
        }
        (Some(width), None) => (width, width / aspect),
        (None, Some(height)) => (height * aspect, height),
        (None, None) => {
            let width = (across * POINTS_A_PIXEL)
                .min(DEFAULT_WIDTH)
                .min(page[0] * MOST_OF_THE_PAGE);
            let tall = page[1] * MOST_OF_THE_PAGE;
            if width / aspect > tall {
                (tall * aspect, tall)
            } else {
                (width, width / aspect)
            }
        }
    };
    let area = [
        asked.left,
        asked.top,
        asked.left + width,
        asked.top + height,
    ];
    if !(width > 0.0 && height > 0.0) {
        return Err("the picture would have no size: give a width or height above 0".to_owned());
    }
    let slack = 0.5;
    if area[0] < -slack
        || area[1] < -slack
        || area[2] > page[0] + slack
        || area[3] > page[1] + slack
    {
        return Err(format!(
            "the picture would be {width:.0} x {height:.0} pt at left {:.0}, top {:.0}, which \
             runs off the page, {:.0} x {:.0} pt: move it or make it smaller",
            asked.left, asked.top, page[0], page[1]
        ));
    }
    Ok(area)
}

#[must_use]
pub fn said(page: usize, [left, top, right, bottom]: [f64; 4]) -> String {
    format!(
        "Placed the picture on page {}, {:.0} x {:.0} pt with its top-left corner at left {left:.0}, \
         top {top:.0}, as one step undo takes back. objects with action list names it.",
        page + 1,
        right - left,
        bottom - top
    )
}

#[must_use]
pub fn said_close(page: usize, region: [f64; 4], (width, height): (u32, u32)) -> String {
    format!(
        "Page {}, the part [{:.0}, {:.0}, {:.0}, {:.0}] of it, drawn {width} x {height} pixels.",
        page + 1,
        region[0],
        region[1],
        region[2],
        region[3]
    )
}

pub fn placement(view: &PageView, [x0, y0, x1, y1]: [f64; 4]) -> Result<Matrix, String> {
    let device = desk::device(view)?;
    let on_screen = Matrix {
        a: x1 - x0,
        b: 0.0,
        c: 0.0,
        d: y0 - y1,
        e: x0,
        f: y1,
    };
    device
        .matrix
        .inverse()
        .map(|back| back.multiply(on_screen))
        .ok_or_else(|| "this page has no size".to_owned())
}

#[cfg(test)]
pub(crate) mod tests;
