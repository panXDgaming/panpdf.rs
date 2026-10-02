use crate::PenStep;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Figure {
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Triangle,
    RightTriangle,
    Diamond,
    Parallelogram,
    Trapezoid,
    Pentagon,
    Hexagon,
    Octagon,
    Star,
    Heart,
    Cross,
    BlockArrow,
    SpeechBubble,
    Cloud,
    Moon,
    Lightning,
}

impl Figure {
    pub const ALL: [Self; 19] = [
        Self::Rectangle,
        Self::RoundedRectangle,
        Self::Ellipse,
        Self::Triangle,
        Self::RightTriangle,
        Self::Diamond,
        Self::Parallelogram,
        Self::Trapezoid,
        Self::Pentagon,
        Self::Hexagon,
        Self::Octagon,
        Self::Star,
        Self::Heart,
        Self::Cross,
        Self::BlockArrow,
        Self::SpeechBubble,
        Self::Cloud,
        Self::Moon,
        Self::Lightning,
    ];

    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Rectangle => "rect",
            Self::RoundedRectangle => "roundrect",
            Self::Ellipse => "oval",
            Self::Triangle => "triangle",
            Self::RightTriangle => "righttriangle",
            Self::Diamond => "diamond",
            Self::Parallelogram => "parallelogram",
            Self::Trapezoid => "trapezoid",
            Self::Pentagon => "pentagon",
            Self::Hexagon => "hexagon",
            Self::Octagon => "octagon",
            Self::Star => "star",
            Self::Heart => "heart",
            Self::Cross => "cross",
            Self::BlockArrow => "blockarrow",
            Self::SpeechBubble => "bubble",
            Self::Cloud => "cloud",
            Self::Moon => "moon",
            Self::Lightning => "lightning",
        }
    }

    #[must_use]
    pub fn named(word: &str) -> Option<Self> {
        let word = word.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|figure| figure.keyword() == word)
            .or(match word.as_str() {
                "rectangle" | "square" => Some(Self::Rectangle),
                "roundedrect" | "roundedrectangle" => Some(Self::RoundedRectangle),
                "plus" => Some(Self::Cross),
                "arrowshape" => Some(Self::BlockArrow),
                "speech" | "speechbubble" | "callout" => Some(Self::SpeechBubble),
                "crescent" => Some(Self::Moon),
                "bolt" => Some(Self::Lightning),
                "rhombus" => Some(Self::Diamond),
                _ => None,
            })
    }

    #[must_use]
    pub fn steps(self, frame: [f64; 4], y_down: bool) -> Vec<PenStep> {
        let mut pen = Pen::new(frame, y_down);
        match self {
            Self::Rectangle => pen.polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            Self::RoundedRectangle => pen.rounded(1.0, None),
            Self::Ellipse => {
                pen.start((1.0, 0.5));
                pen.arc((0.5, 0.5), (0.5, 0.5), 0.0, 360.0);
            }
            Self::Triangle => pen.polygon(&[(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            Self::RightTriangle => pen.polygon(&[(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            Self::Diamond => pen.polygon(&[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)]),
            Self::Parallelogram => {
                pen.polygon(&[(0.25, 0.0), (1.0, 0.0), (0.75, 1.0), (0.0, 1.0)]);
            }
            Self::Trapezoid => pen.polygon(&[(0.2, 0.0), (0.8, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            Self::Pentagon => pen.polygon(&regular(5, 0.0)),
            Self::Hexagon => pen.polygon(&regular(6, 30.0)),
            Self::Octagon => pen.polygon(&regular(8, 22.5)),
            Self::Star => pen.polygon(&star(5, 0.382)),
            Self::Heart => heart(&mut pen),
            Self::Cross => {
                let (a, b) = (1.0 / 3.0, 2.0 / 3.0);
                pen.polygon(&[
                    (a, 0.0),
                    (b, 0.0),
                    (b, a),
                    (1.0, a),
                    (1.0, b),
                    (b, b),
                    (b, 1.0),
                    (a, 1.0),
                    (a, b),
                    (0.0, b),
                    (0.0, a),
                    (a, a),
                ]);
            }
            Self::BlockArrow => pen.polygon(&[
                (0.0, 0.3),
                (0.6, 0.3),
                (0.6, 0.0),
                (1.0, 0.5),
                (0.6, 1.0),
                (0.6, 0.7),
                (0.0, 0.7),
            ]),
            Self::SpeechBubble => {
                pen.rounded(0.78, Some([(0.35, 0.78), (0.12, 1.0), (0.22, 0.78)]));
            }
            Self::Cloud => cloud(&mut pen),
            Self::Moon => {
                pen.start(on_circle((0.5, 0.5), 0.5, 300.0));
                pen.arc((0.5, 0.5), (0.5, 0.5), 300.0, 60.0);
                let inner = (0.9, 0.5);
                let radius = 0.15_f64.hypot(0.5 * 60.0_f64.to_radians().sin());
                let low = (0.5 * 60.0_f64.to_radians().sin())
                    .atan2(-0.15)
                    .to_degrees();
                pen.arc(inner, (radius, radius), low, 360.0 - low);
            }
            Self::Lightning => pen.polygon(&[
                (0.55, 0.0),
                (0.15, 0.55),
                (0.45, 0.55),
                (0.3, 1.0),
                (0.85, 0.4),
                (0.55, 0.4),
                (0.75, 0.0),
            ]),
        }
        pen.steps
    }
}

#[must_use]
pub fn regular_polygon(sides: usize, turn: f64, frame: [f64; 4], y_down: bool) -> Vec<PenStep> {
    let mut pen = Pen::new(frame, y_down);
    pen.polygon(&regular(sides.clamp(3, 64), turn));
    pen.steps
}

#[must_use]
pub fn star_figure(points: usize, inner: f64, frame: [f64; 4], y_down: bool) -> Vec<PenStep> {
    let mut pen = Pen::new(frame, y_down);
    pen.polygon(&star(points.clamp(3, 64), inner.clamp(0.05, 0.95)));
    pen.steps
}

#[must_use]
pub fn turned(steps: &[PenStep], about: (f64, f64), degrees: f64) -> Vec<PenStep> {
    if degrees.rem_euclid(360.0).abs() < 1e-9 {
        return steps.to_vec();
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let turn = |(x, y): (f64, f64)| {
        let (dx, dy) = (x - about.0, y - about.1);
        (
            about.0 + dx.mul_add(cos, -dy * sin),
            about.1 + dx.mul_add(sin, dy * cos),
        )
    };
    steps
        .iter()
        .map(|step| match *step {
            PenStep::Move(point) => PenStep::Move(turn(point)),
            PenStep::Line(point) => PenStep::Line(turn(point)),
            PenStep::Curve(one, other, end) => PenStep::Curve(turn(one), turn(other), turn(end)),
        })
        .collect()
}

fn regular(sides: usize, turn: f64) -> Vec<(f64, f64)> {
    (0..sides)
        .map(|at| {
            #[expect(clippy::cast_precision_loss, reason = "a few corners")]
            let angle = turn - 90.0 + 360.0 * at as f64 / sides as f64;
            on_circle((0.5, 0.5), 0.5, angle)
        })
        .collect()
}

fn star(points: usize, inner: f64) -> Vec<(f64, f64)> {
    (0..points * 2)
        .map(|at| {
            #[expect(clippy::cast_precision_loss, reason = "a few corners")]
            let angle = -90.0 + 180.0 * at as f64 / points as f64;
            let reach = if at % 2 == 0 { 0.5 } else { 0.5 * inner };
            on_circle((0.5, 0.5), reach, angle)
        })
        .collect()
}

fn on_circle(centre: (f64, f64), radius: f64, degrees: f64) -> (f64, f64) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    (radius.mul_add(cos, centre.0), radius.mul_add(sin, centre.1))
}

fn heart(pen: &mut Pen) {
    pen.start((0.5, 0.25));
    pen.curve((0.5, 0.1), (0.35, 0.0), (0.22, 0.0));
    pen.curve((0.05, 0.0), (0.0, 0.15), (0.0, 0.3));
    pen.curve((0.0, 0.55), (0.3, 0.75), (0.5, 1.0));
    pen.curve((0.7, 0.75), (1.0, 0.55), (1.0, 0.3));
    pen.curve((1.0, 0.15), (0.95, 0.0), (0.78, 0.0));
    pen.curve((0.65, 0.0), (0.5, 0.1), (0.5, 0.25));
}

fn cloud(pen: &mut Pen) {
    const BUMPS: usize = 9;
    let centre = (0.5, 0.55);
    let (rx, ry): (f64, f64) = (0.4, 0.33);
    let corner = |at: usize| {
        #[expect(clippy::cast_precision_loss, reason = "nine bumps")]
        let angle = (-90.0 + 360.0 * at as f64 / BUMPS as f64).to_radians();
        let (sin, cos) = angle.sin_cos();
        (
            (rx.mul_add(cos, centre.0), ry.mul_add(sin, centre.1)),
            (cos, sin),
        )
    };
    let (first, _) = corner(0);
    pen.start(first);
    for at in 0..BUMPS {
        let (from, out_from) = corner(at);
        let (to, out_to) = corner((at + 1) % BUMPS);
        let bulge: f64 = 0.2;
        pen.curve(
            (
                bulge.mul_add(out_from.0, from.0),
                (bulge * 0.8).mul_add(out_from.1, from.1),
            ),
            (
                bulge.mul_add(out_to.0, to.0),
                (bulge * 0.8).mul_add(out_to.1, to.1),
            ),
            to,
        );
    }
}

struct Pen {
    frame: [f64; 4],
    y_down: bool,
    steps: Vec<PenStep>,
    at: (f64, f64),
}

impl Pen {
    const fn new(frame: [f64; 4], y_down: bool) -> Self {
        Self {
            frame,
            y_down,
            steps: Vec::new(),
            at: (0.0, 0.0),
        }
    }

    fn wide(&self) -> f64 {
        (self.frame[2] - self.frame[0]).abs()
    }

    fn high(&self) -> f64 {
        (self.frame[3] - self.frame[1]).abs()
    }

    fn place(&self, (u, v): (f64, f64)) -> (f64, f64) {
        let [left, top, right, bottom] = self.frame;
        let (x0, x1) = (left.min(right), left.max(right));
        let (y0, y1) = (top.min(bottom), top.max(bottom));
        let x = u.mul_add(x1 - x0, x0);
        let y = if self.y_down {
            v.mul_add(y1 - y0, y0)
        } else {
            (-v).mul_add(y1 - y0, y1)
        };
        (x, y)
    }

    fn start(&mut self, point: (f64, f64)) {
        self.at = point;
        self.steps.push(PenStep::Move(self.place(point)));
    }

    fn line(&mut self, point: (f64, f64)) {
        self.at = point;
        self.steps.push(PenStep::Line(self.place(point)));
    }

    fn curve(&mut self, one: (f64, f64), other: (f64, f64), end: (f64, f64)) {
        self.at = end;
        self.steps.push(PenStep::Curve(
            self.place(one),
            self.place(other),
            self.place(end),
        ));
    }

    fn polygon(&mut self, corners: &[(f64, f64)]) {
        if let Some((first, rest)) = corners.split_first() {
            self.start(*first);
            for corner in rest {
                self.line(*corner);
            }
            self.line(*first);
        }
    }

    fn arc(&mut self, centre: (f64, f64), (rx, ry): (f64, f64), from: f64, to: f64) {
        let sweep = to - from;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a handful of quarter turns"
        )]
        let pieces = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
        #[expect(clippy::cast_precision_loss, reason = "a handful of quarter turns")]
        let step = sweep / pieces as f64;
        let handle = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
        let point = |degrees: f64| {
            let (sin, cos) = degrees.to_radians().sin_cos();
            (
                (rx.mul_add(cos, centre.0), ry.mul_add(sin, centre.1)),
                (-rx * sin, ry * cos),
            )
        };
        for piece in 0..pieces {
            #[expect(clippy::cast_precision_loss, reason = "a handful of quarter turns")]
            let (a, b) = (
                (piece as f64).mul_add(step, from),
                ((piece + 1) as f64).mul_add(step, from),
            );
            let ((p0, d0), (p1, d1)) = (point(a), point(b));
            self.curve(
                (handle.mul_add(d0.0, p0.0), handle.mul_add(d0.1, p0.1)),
                ((-handle).mul_add(d1.0, p1.0), (-handle).mul_add(d1.1, p1.1)),
                p1,
            );
        }
    }

    fn rounded(&mut self, body: f64, tail: Option<[(f64, f64); 3]>) {
        let radius = 0.18 * self.wide().min(self.high() * body);
        let ru = if self.wide() > 0.0 {
            radius / self.wide()
        } else {
            0.0
        };
        let rv = if self.high() > 0.0 {
            radius / self.high()
        } else {
            0.0
        };
        self.start((ru, 0.0));
        self.line((1.0 - ru, 0.0));
        self.arc((1.0 - ru, rv), (ru, rv), 270.0, 360.0);
        self.line((1.0, body - rv));
        self.arc((1.0 - ru, body - rv), (ru, rv), 0.0, 90.0);
        if let Some(tail) = tail {
            for point in tail {
                self.line(point);
            }
        }
        self.line((ru, body));
        self.arc((ru, body - rv), (ru, rv), 90.0, 180.0);
        self.line((0.0, rv));
        self.arc((ru, rv), (ru, rv), 180.0, 270.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(steps: &[PenStep]) -> Vec<(f64, f64)> {
        steps.iter().flat_map(PenStep::points).collect()
    }

    fn ends(steps: &[PenStep]) -> Vec<(f64, f64)> {
        steps
            .iter()
            .map(|step| match *step {
                PenStep::Move(point) | PenStep::Line(point) | PenStep::Curve(_, _, point) => point,
            })
            .collect()
    }

    fn near(one: (f64, f64), other: (f64, f64)) -> bool {
        (one.0 - other.0).abs() < 1e-6 && (one.1 - other.1).abs() < 1e-6
    }

    #[test]
    fn every_figure_is_closed_and_inside_its_box() {
        let frame = [100.0, 50.0, 300.0, 150.0];
        for figure in Figure::ALL {
            let steps = figure.steps(frame, true);
            assert!(
                matches!(steps.first(), Some(PenStep::Move(_))),
                "{figure:?}"
            );
            let ends = ends(&steps);
            assert!(
                near(ends[0], *ends.last().unwrap()),
                "{figure:?} does not close"
            );
            for (x, y) in ends {
                assert!(
                    (99.999..=300.001).contains(&x) && (49.999..=150.001).contains(&y),
                    "{figure:?}: ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn a_triangle_points_up_whichever_way_y_runs() {
        let screen = Figure::Triangle.steps([0.0, 0.0, 10.0, 20.0], true);
        assert!(near(ends(&screen)[0], (5.0, 0.0)));
        let page = Figure::Triangle.steps([0.0, 0.0, 10.0, 20.0], false);
        assert!(near(ends(&page)[0], (5.0, 20.0)));
    }

    #[test]
    fn an_ellipse_in_a_square_is_a_circle() {
        let steps = Figure::Ellipse.steps([0.0, 0.0, 100.0, 100.0], true);
        for target in [(100.0, 50.0), (50.0, 100.0), (0.0, 50.0), (50.0, 0.0)] {
            assert!(
                ends(&steps).iter().any(|point| near(*point, target)),
                "{target:?}"
            );
        }
        let mut from = (100.0, 50.0);
        for step in &steps[1..] {
            if let PenStep::Curve(a, b, to) = *step {
                let mid = (
                    0.125 * (from.0 + to.0) + 0.375 * (a.0 + b.0),
                    0.125 * (from.1 + to.1) + 0.375 * (a.1 + b.1),
                );
                let reach = (mid.0 - 50.0).hypot(mid.1 - 50.0);
                assert!((reach - 50.0).abs() < 0.05, "{reach}");
                from = to;
            }
        }
    }

    #[test]
    fn a_star_has_its_points_and_its_waist() {
        let steps = star_figure(5, 0.4, [0.0, 0.0, 100.0, 100.0], true);
        assert_eq!(steps.len(), 11);
        let reaches: Vec<f64> = ends(&steps)
            .iter()
            .map(|(x, y)| (x - 50.0).hypot(y - 50.0))
            .collect();
        for (at, reach) in reaches.iter().take(10).enumerate() {
            let want = if at % 2 == 0 { 50.0 } else { 20.0 };
            assert!((reach - want).abs() < 1e-6, "{at}: {reach}");
        }
    }

    #[test]
    fn a_quarter_turn_goes_clockwise_on_a_screen() {
        let steps = Figure::Diamond.steps([0.0, 0.0, 10.0, 10.0], true);
        let turned = turned(&steps, (5.0, 5.0), 90.0);
        assert!(
            near(ends(&turned)[0], (10.0, 5.0)),
            "{:?}",
            ends(&turned)[0]
        );
        assert_eq!(points(&steps).len(), points(&turned).len());
    }

    #[test]
    fn keywords_name_their_figures_and_a_few_other_words_too() {
        for figure in Figure::ALL {
            assert_eq!(Figure::named(figure.keyword()), Some(figure));
        }
        assert_eq!(Figure::named("Square"), Some(Figure::Rectangle));
        assert_eq!(Figure::named("speech"), Some(Figure::SpeechBubble));
        assert_eq!(Figure::named("blob"), None);
    }
}
