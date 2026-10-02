use std::fmt::Write as _;

use crate::page::{Page, Rgb, num};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    Axial {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
    Radial {
        fx: f32,
        fy: f32,
        fr: f32,
        cx: f32,
        cy: f32,
        r: f32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<(f32, Rgb)>,
}

fn colour(c: Rgb, grey: bool) -> String {
    if grey {
        num(c.0)
    } else {
        format!("{} {} {}", num(c.0), num(c.1), num(c.2))
    }
}

impl Gradient {
    fn whole_stops(&self) -> Vec<(f32, Rgb)> {
        let mut stops: Vec<(f32, Rgb)> = Vec::with_capacity(self.stops.len() + 2);
        let mut last = 0.0_f32;
        for &(offset, c) in &self.stops {
            let offset = if offset.is_finite() {
                offset.clamp(0.0, 1.0).max(last)
            } else {
                last
            };
            last = offset;
            stops.push((offset, c));
        }
        match (stops.first().copied(), stops.last().copied()) {
            (Some(first), Some(end)) => {
                if first.0 > 0.0 {
                    stops.insert(0, (0.0, first.1));
                }
                if end.0 < 1.0 {
                    stops.push((1.0, end.1));
                }
            }
            _ => stops = vec![(0.0, Rgb::BLACK), (1.0, Rgb::BLACK)],
        }
        if stops.len() == 1 {
            stops.push((1.0, stops[0].1));
        }
        stops
    }

    fn function(&self, grey: bool) -> String {
        let stops = self.whole_stops();
        let mut parts = Vec::new();
        let mut bounds = Vec::new();
        for pair in stops.windows(2) {
            let ((a, ca), (b, cb)) = (pair[0], pair[1]);
            if b <= a && stops.len() > 2 {
                continue;
            }
            if !parts.is_empty() {
                bounds.push(a);
            }
            parts.push(format!(
                "<< /FunctionType 2 /Domain [0 1] /C0 [{}] /C1 [{}] /N 1 >>",
                colour(ca, grey),
                colour(cb, grey)
            ));
        }
        if parts.len() == 1 {
            return parts.remove(0);
        }
        let mut out = String::from("<< /FunctionType 3 /Domain [0 1] /Functions [");
        out.push_str(&parts.join(" "));
        out.push_str("] /Bounds [");
        let bounds: Vec<String> = bounds.iter().map(|b| num(*b)).collect();
        out.push_str(&bounds.join(" "));
        out.push_str("] /Encode [");
        for k in 0..parts.len() {
            if k > 0 {
                out.push(' ');
            }
            out.push_str("0 1");
        }
        out.push_str("] >>");
        out
    }

    pub(crate) fn shading(&self, grey: bool) -> String {
        let (kind, coords) = match self.kind {
            GradientKind::Axial { x1, y1, x2, y2 } => (2, vec![x1, y1, x2, y2]),
            GradientKind::Radial {
                fx,
                fy,
                fr,
                cx,
                cy,
                r,
            } => (3, vec![fx, fy, fr.max(0.0), cx, cy, r.max(0.0)]),
        };
        let coords: Vec<String> = coords.into_iter().map(num).collect();
        format!(
            "<< /ShadingType {kind} /ColorSpace {} /Coords [{}] /Function {} /Extend [true true] >>",
            if grey { "/DeviceGray" } else { "/DeviceRGB" },
            coords.join(" "),
            self.function(grey)
        )
    }
}

fn matrix_text(m: [f32; 6]) -> String {
    let parts: Vec<String> = m.iter().map(|v| num(*v)).collect();
    parts.join(" ")
}

#[derive(Clone, Debug)]
pub(crate) struct SoftMask {
    pub shading: String,
    pub matrix: [f32; 6],
    pub bbox: [f32; 4],
}

impl Page {
    fn pattern(&mut self, gradient: &Gradient, matrix: [f32; 6]) -> usize {
        let text = format!(
            "<< /Type /Pattern /PatternType 2 /Shading {} /Matrix [{}] >>",
            gradient.shading(false),
            matrix_text(matrix)
        );
        if let Some(found) = self.patterns.iter().position(|p| *p == text) {
            return found;
        }
        self.patterns.push(text);
        self.patterns.len() - 1
    }

    pub fn set_fill_gradient(&mut self, gradient: &Gradient, matrix: [f32; 6]) -> &mut Self {
        let n = self.pattern(gradient, matrix);
        self.raw(&format!("/Pattern cs /P{n} scn"))
    }

    pub fn set_stroke_gradient(&mut self, gradient: &Gradient, matrix: [f32; 6]) -> &mut Self {
        let n = self.pattern(gradient, matrix);
        self.raw(&format!("/Pattern CS /P{n} SCN"))
    }

    pub fn set_soft_mask(
        &mut self,
        alpha: &Gradient,
        matrix: [f32; 6],
        bbox: [f32; 4],
    ) -> &mut Self {
        self.soft_masks.push(SoftMask {
            shading: alpha.shading(true),
            matrix,
            bbox,
        });
        let n = self.soft_masks.len() - 1;
        self.raw(&format!("/SM{n} gs"))
    }

    pub fn set_join(&mut self, join: u8) -> &mut Self {
        self.raw(&format!("{} j", join.min(2)))
    }

    pub fn set_miter_limit(&mut self, limit: f32) -> &mut Self {
        self.raw(&format!("{} M", num(limit.max(1.0))))
    }
}

pub(crate) fn soft_mask_form(mask: &SoftMask) -> (String, String) {
    let b = mask.bbox;
    let dict = format!(
        "/Type /XObject /Subtype /Form /BBox [{} {} {} {}] /Group << /S /Transparency /CS /DeviceGray >> /Resources << /Shading << /Sh0 {} >> >>",
        num(b[0]),
        num(b[1]),
        num(b[2]),
        num(b[3]),
        mask.shading
    );
    let mut content = String::new();
    let _ = write!(content, "q {} cm /Sh0 sh Q", matrix_text(mask.matrix));
    (dict, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_become_one_function_or_a_stitched_one() {
        let two = Gradient {
            kind: GradientKind::Axial {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 0.0,
            },
            stops: vec![(0.0, Rgb::BLACK), (1.0, Rgb::WHITE)],
        };
        assert!(two.shading(false).contains("/FunctionType 2"));
        let hard = Gradient {
            stops: vec![
                (0.2, Rgb::BLACK),
                (0.5, Rgb::WHITE),
                (0.5, Rgb(1.0, 0.0, 0.0)),
                (0.4, Rgb::BLACK),
            ],
            ..two
        };
        let text = hard.shading(false);
        assert!(text.contains("/FunctionType 3"));
        assert!(text.contains("/Bounds [0.2 0.5]"), "{text}");
    }
}
