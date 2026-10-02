use crate::model::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    crop: [f64; 4],
    rotate: u16,
}

impl Shown {
    #[must_use]
    pub fn new(crop: [f64; 4], rotate: u16) -> Self {
        let crop = [
            crop[0].min(crop[2]),
            crop[1].min(crop[3]),
            crop[0].max(crop[2]),
            crop[1].max(crop[3]),
        ];
        Self {
            crop,
            rotate: rotate % 360,
        }
    }

    #[must_use]
    pub fn of(geometry: &pdf_content::PageGeometry) -> Self {
        Self::new(geometry.crop_box, geometry.rotate)
    }

    #[must_use]
    pub fn size(&self) -> (f64, f64) {
        let w = self.crop[2] - self.crop[0];
        let h = self.crop[3] - self.crop[1];
        if self.rotate == 90 || self.rotate == 270 {
            (h, w)
        } else {
            (w, h)
        }
    }

    #[must_use]
    pub fn point(&self, x: f64, y: f64) -> (f64, f64) {
        let [x0, y0, x1, y1] = self.crop;
        match self.rotate {
            90 => (y - y0, x - x0),
            180 => (x1 - x, y - y0),
            270 => (y1 - y, x1 - x),
            _ => (x - x0, y1 - y),
        }
    }

    #[must_use]
    pub fn rect(&self, bounds: [f64; 4]) -> Rect {
        let (ax, ay) = self.point(bounds[0], bounds[1]);
        let (bx, by) = self.point(bounds[2], bounds[3]);
        Rect::new(ax, ay, bx, by)
    }

    #[must_use]
    pub fn page(&self) -> Rect {
        let (w, h) = self.size();
        Rect::new(0.0, 0.0, w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upright_page_flips_y() {
        let shown = Shown::new([0.0, 0.0, 600.0, 800.0], 0);
        assert_eq!(shown.point(10.0, 790.0), (10.0, 10.0));
        assert_eq!(shown.size(), (600.0, 800.0));
    }

    #[test]
    fn quarter_turns_keep_corners_on_the_page() {
        for rotate in [90, 180, 270] {
            let shown = Shown::new([0.0, 0.0, 600.0, 800.0], rotate);
            let (w, h) = shown.size();
            for (x, y) in [(0.0, 0.0), (600.0, 0.0), (0.0, 800.0), (600.0, 800.0)] {
                let (sx, sy) = shown.point(x, y);
                assert!(
                    (0.0..=w).contains(&sx) && (0.0..=h).contains(&sy),
                    "{rotate}: {sx},{sy}"
                );
            }
        }
        let shown = Shown::new([0.0, 0.0, 600.0, 800.0], 90);
        assert_eq!(shown.point(0.0, 400.0).1, 0.0);
    }
}
