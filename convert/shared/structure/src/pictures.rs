use pdf_paint::{PaintAtomKind, PaintGraph};
use pdf_session::PageView;

use crate::geometry::Shown;
use crate::model::{Picture, PictureFormat, Rect};

const LEAST_SIDE: f64 = 8.0;
const MOST_SCALE: f64 = 200.0 / 72.0;
const MOST_PIXELS: f64 = 4.0e6;

#[must_use]
pub fn read(view: &PageView, shown: &Shown, text: &[Rect]) -> (Vec<Picture>, Vec<String>) {
    let mut pictures = Vec::new();
    let mut notes = Vec::new();
    let page = shown.page();
    for atom in &view.graph.atoms {
        let PaintAtomKind::Image(image) = &atom.kind else {
            continue;
        };
        let Some(bounds) = atom.kind.user_bounds() else {
            continue;
        };
        let frame = shown.rect(bounds);
        let visible = Rect::new(
            frame.x0.max(0.0),
            frame.y0.max(0.0),
            frame.x1.min(page.x1),
            frame.y1.min(page.y1),
        );
        if visible.width() < LEAST_SIDE || visible.height() < LEAST_SIDE {
            continue;
        }
        let native = (f64::from(image.width.value), f64::from(image.height.value));
        let mut scale = (native.0 / frame.width())
            .max(native.1 / frame.height())
            .clamp(1.0, MOST_SCALE);
        let pixels = visible.area() * scale * scale;
        if pixels > MOST_PIXELS {
            scale *= (MOST_PIXELS / pixels).sqrt();
        }
        match draw(view, vec![atom.clone()], bounds, scale) {
            Ok((data, size)) => {
                let covered_text = text
                    .iter()
                    .filter(|rect| visible.overlap(rect) > 0.5 * rect.area())
                    .count();
                let background = visible.area() >= 0.6 * page.area() && covered_text > 0;
                pictures.push(Picture {
                    frame: visible,
                    format: PictureFormat::Png,
                    data,
                    pixels: size,
                    stored: None,
                    background,
                });
            }
            Err(why) => notes.push(format!("a picture could not be drawn: {why}")),
        }
    }
    (pictures, notes)
}

fn draw(
    view: &PageView,
    atoms: Vec<pdf_paint::PaintAtom>,
    bounds: [f64; 4],
    scale: f64,
) -> Result<(Vec<u8>, (u32, u32)), String> {
    let limits = pdf_render::RenderLimits {
        max_pixels: 1 << 31,
        ..pdf_render::RenderLimits::default()
    };
    let geometry = &view.program.geometry;
    let device = pdf_render::DeviceTransform::for_page(geometry, scale, limits)
        .map_err(|e| e.to_string())?;
    let corners = [
        (bounds[0], bounds[1]),
        (bounds[2], bounds[1]),
        (bounds[0], bounds[3]),
        (bounds[2], bounds[3]),
    ];
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for (x, y) in corners {
        let p = device.matrix.transform(pdf_paint::Point { x, y });
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let clamp = |v: f64, most: u32| -> u32 {
        let v = v.clamp(0.0, f64::from(most));
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            v as u32
        }
    };
    let region = [
        clamp(x0.floor(), device.width),
        clamp(y0.floor(), device.height),
        clamp(x1.ceil(), device.width),
        clamp(y1.ceil(), device.height),
    ];
    let graph = PaintGraph {
        atoms,
        ..PaintGraph::default()
    };
    let options = pdf_render::RenderOptions { scale, limits };
    let (canvas, _) =
        pdf_render::render_region(&graph, geometry, options, region).map_err(|e| e.to_string())?;
    let png = pdf_edit::png::write((canvas.width, canvas.height), &canvas.to_rgb8(), None)
        .map_err(str::to_owned)?;
    Ok((png, (canvas.width, canvas.height)))
}

#[must_use]
pub fn drawings(
    view: &PageView,
    shown: &Shown,
    text: &[Rect],
    tables: &[Rect],
) -> (Vec<Picture>, Vec<String>) {
    let page = shown.page();
    struct Shape {
        index: usize,
        rect: Rect,
        user: [f64; 4],
        curved: bool,
        thin: bool,
        marked: bool,
    }
    let mut shapes: Vec<Shape> = Vec::new();
    let drawn_by_annotations = view
        .annotations
        .iter()
        .filter(|annotation| {
            matches!(
                annotation.subtype.as_deref(),
                Some(
                    b"Square" | b"Circle" | b"Line" | b"PolyLine" | b"Polygon" | b"Ink" | b"Stamp"
                )
            )
        })
        .flat_map(|annotation| &annotation.graph.atoms);
    let own = view.graph.atoms.len();
    let sources: Vec<&pdf_paint::PaintAtom> = view
        .graph
        .atoms
        .iter()
        .chain(drawn_by_annotations)
        .collect();
    for (index, atom) in sources.iter().enumerate() {
        let curved = match &atom.kind {
            PaintAtomKind::Path(paint) => paint
                .path
                .segments
                .iter()
                .any(|s| matches!(s, pdf_paint::PathSegment::CubicTo { .. })),
            PaintAtomKind::Shading(_) => true,
            _ => continue,
        };
        let bounds = match &atom.kind {
            PaintAtomKind::Path(paint) => painted_bounds(paint),
            other => other.user_bounds(),
        };
        let Some(user) = bounds else {
            continue;
        };
        let rect = shown.rect(user);
        if rect.area() >= 0.5 * page.area() || tables.iter().any(|t| t.contains(rect.center())) {
            continue;
        }
        let thin = rect.width().min(rect.height()) <= 2.5;
        shapes.push(Shape {
            index,
            rect,
            user,
            curved,
            thin,
            marked: index >= own,
        });
    }
    if shapes.len() > 5000 {
        return (
            Vec::new(),
            vec![format!("{} shapes: drawings not gathered", shapes.len())],
        );
    }
    let mut parent: Vec<usize> = (0..shapes.len()).collect();
    fn root(parent: &mut [usize], mut a: usize) -> usize {
        while parent[a] != a {
            parent[a] = parent[parent[a]];
            a = parent[a];
        }
        a
    }
    for i in 0..shapes.len() {
        let grown = Rect::new(
            shapes[i].rect.x0 - 4.0,
            shapes[i].rect.y0 - 4.0,
            shapes[i].rect.x1 + 4.0,
            shapes[i].rect.y1 + 4.0,
        );
        for (j, other) in shapes.iter().enumerate().skip(i + 1) {
            let r = &other.rect;
            if r.x0 <= grown.x1 && r.x1 >= grown.x0 && r.y0 <= grown.y1 && r.y1 >= grown.y0 {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                if a != b {
                    parent[b] = a;
                }
            }
        }
    }
    let mut clusters: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for i in 0..shapes.len() {
        let r = root(&mut parent, i);
        clusters.entry(r).or_default().push(i);
    }
    let mut pictures = Vec::new();
    let mut notes = Vec::new();
    for members in clusters.values() {
        let rect = members
            .iter()
            .skip(1)
            .fold(shapes[members[0]].rect, |r, &m| r.union(&shapes[m].rect));
        let curved = members.iter().any(|&m| shapes[m].curved);
        let solid = members.iter().any(|&m| !shapes[m].thin);
        let marked = members.iter().any(|&m| shapes[m].marked);
        let drawing = if marked {
            rect.width().max(rect.height()) >= 20.0
        } else {
            rect.width() >= 20.0 && rect.height() >= 20.0 && solid && (members.len() >= 3 || curved)
        };
        if !drawing {
            continue;
        }
        let covered: f64 = text.iter().map(|t| t.overlap(&rect)).sum();
        if covered > 0.3 * rect.area() {
            continue;
        }
        let mut atoms: Vec<usize> = members.iter().map(|&m| shapes[m].index).collect();
        atoms.sort_unstable();
        let user = members
            .iter()
            .skip(1)
            .fold(shapes[members[0]].user, |u, &m| {
                let v = shapes[m].user;
                [
                    u[0].min(v[0]),
                    u[1].min(v[1]),
                    u[2].max(v[2]),
                    u[3].max(v[3]),
                ]
            });
        let mut scale = 2.0_f64;
        let pixels = rect.area() * scale * scale;
        if pixels > MOST_PIXELS {
            scale *= (MOST_PIXELS / pixels).sqrt();
        }
        let chosen: Vec<pdf_paint::PaintAtom> = atoms.iter().map(|&i| sources[i].clone()).collect();
        match draw(view, chosen, user, scale) {
            Ok((data, size)) => pictures.push(Picture {
                frame: rect,
                format: PictureFormat::Png,
                data,
                pixels: size,
                stored: None,
                background: false,
            }),
            Err(why) => notes.push(format!("a drawing could not be drawn: {why}")),
        }
    }
    (pictures, notes)
}

fn painted_bounds(paint: &pdf_paint::PathPaint) -> Option<[f64; 4]> {
    use pdf_paint::{PathSegment, Point};
    let ctm = paint.state.ctm.value;
    let mut points: Vec<Point> = Vec::new();
    let mut current = Point { x: 0.0, y: 0.0 };
    for segment in &paint.path.segments {
        match segment {
            PathSegment::MoveTo { point, .. } | PathSegment::LineTo { point, .. } => {
                points.push(*point);
                current = *point;
            }
            PathSegment::CubicTo {
                control_1,
                control_2,
                end,
                ..
            } => {
                for step in 1..=16 {
                    let t = f64::from(step) / 16.0;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    points.push(Point {
                        x: a * current.x + b * control_1.x + c * control_2.x + d * end.x,
                        y: a * current.y + b * control_1.y + c * control_2.y + d * end.y,
                    });
                }
                current = *end;
            }
            PathSegment::Rectangle {
                origin,
                width,
                height,
                ..
            } => {
                points.push(*origin);
                points.push(Point {
                    x: origin.x + width,
                    y: origin.y + height,
                });
                points.push(Point {
                    x: origin.x + width,
                    y: origin.y,
                });
                points.push(Point {
                    x: origin.x,
                    y: origin.y + height,
                });
                current = *origin;
            }
            PathSegment::ClosePath { .. } => {}
        }
    }
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for point in points {
        let placed = ctm.transform(point);
        if placed.x.is_finite() && placed.y.is_finite() {
            bounds[0] = bounds[0].min(placed.x);
            bounds[1] = bounds[1].min(placed.y);
            bounds[2] = bounds[2].max(placed.x);
            bounds[3] = bounds[3].max(placed.y);
        }
    }
    if !(bounds[0] <= bounds[2] && bounds[1] <= bounds[3]) {
        return None;
    }
    if paint.stroke {
        let scale = (ctm.a * ctm.d - ctm.b * ctm.c).abs().sqrt();
        let half = (paint.state.line_width.value * scale).max(1.0) / 2.0;
        bounds = [
            bounds[0] - half,
            bounds[1] - half,
            bounds[2] + half,
            bounds[3] + half,
        ];
    }
    Some(bounds)
}
