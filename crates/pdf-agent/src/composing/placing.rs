use std::collections::BTreeMap;

use pdf_content::PageGeometry;
use pdf_edit::{Command, ParagraphLayout, PenStroke};
use pdf_paint::Matrix;

use super::Mark;
use crate::desk::{steps_in_user_space_with, to_user_with};

struct Surface {
    shown: Matrix,
    size: [f64; 2],
}

fn of_a_page(geometry: &PageGeometry) -> Option<Surface> {
    let device =
        pdf_render::DeviceTransform::for_page(geometry, 1.0, pdf_render::RenderLimits::default())
            .ok()?;
    Some(Surface {
        shown: device.matrix,
        size: [f64::from(device.width), f64::from(device.height)],
    })
}

fn blank(size: [f64; 2]) -> Surface {
    Surface {
        shown: Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            e: 0.0,
            f: size[1],
        },
        size,
    }
}

pub fn as_commands(
    marks: &[Mark],
    page: &dyn Fn(usize) -> Option<PageGeometry>,
) -> Result<Vec<Command>, String> {
    let mut put_in: BTreeMap<usize, Surface> = BTreeMap::new();
    let mut commands = Vec::with_capacity(marks.len());
    for mark in marks {
        match mark {
            Mark::NewPage { after } => {
                let size = match put_in.get(after) {
                    Some(surface) => surface.size,
                    None => page(*after)
                        .as_ref()
                        .and_then(of_a_page)
                        .map_or([595.276, 841.89], |surface| surface.size),
                };
                let later: Vec<usize> = put_in.range(after + 1..).map(|(at, _)| *at).collect();
                for at in later.into_iter().rev() {
                    if let Some(surface) = put_in.remove(&at) {
                        put_in.insert(at + 1, surface);
                    }
                }
                put_in.insert(after + 1, blank(size));
                commands.push(Command::AddBlankPage {
                    beside: *after,
                    before: false,
                    size,
                });
            }
            Mark::Text {
                page: at,
                area,
                text,
                style,
            } => {
                let frame = on(&put_in, *at, page, |surface| {
                    to_user_with(&surface.shown, *area)
                })?;
                commands.push(Command::PlaceNewText {
                    page_index: *at,
                    frame,
                    text: text.replace("\r\n", "\n"),
                    family: style.family.clone(),
                    size: style.size,
                    bold: style.bold,
                    italic: style.italic,
                    fill: style.colour,
                    paragraph: ParagraphLayout::default(),
                });
            }
            Mark::Shape {
                page: at,
                steps,
                stroke,
                fill,
            } => {
                let steps = on(&put_in, *at, page, |surface| {
                    steps_in_user_space_with(&surface.shown, steps)
                })?;
                commands.push(Command::DrawPath {
                    page_index: *at,
                    steps,
                    closed: false,
                    stroke: stroke.map(|(colour, width)| PenStroke::pen(colour, width)),
                    fill: *fill,
                });
            }
        }
    }
    Ok(commands)
}

fn on<T>(
    put_in: &BTreeMap<usize, Surface>,
    at: usize,
    page: &dyn Fn(usize) -> Option<PageGeometry>,
    with: impl FnOnce(&Surface) -> Result<T, String>,
) -> Result<T, String> {
    if let Some(surface) = put_in.get(&at) {
        return with(surface);
    }
    let surface = page(at)
        .as_ref()
        .and_then(of_a_page)
        .ok_or_else(|| format!("page {} has no size this program can write on", at + 1))?;
    with(&surface)
}
