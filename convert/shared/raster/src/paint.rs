use std::collections::HashMap;

use pdf_paint::{ColorSpace, ImagePaint};

use crate::{Image, to_u8};

#[must_use]
pub fn jpeg_bytes<'a>(image: &ImagePaint, source: &'a pdf_bytes::ByteStore) -> Option<&'a [u8]> {
    let codec = image.codec.as_ref()?;
    if codec.value != pdf_syntax::ImageCodec::Dct
        || image.image_mask.value
        || image.soft_mask.is_some()
        || image.mask.is_some()
        || image.bits_per_component.value != 8
    {
        return None;
    }
    let components = image.components();
    let plain_space = match &image.color_space.as_ref()?.value {
        ColorSpace::DeviceGray | ColorSpace::DeviceRgb => true,
        ColorSpace::IccBased(icc) => matches!(icc.components.value, 1 | 3),
        _ => false,
    };
    if !plain_space || !(components == 1 || components == 3) {
        return None;
    }
    let plain_decode = image
        .decode
        .value
        .chunks(2)
        .all(|pair| pair.len() == 2 && pair[0] == 0.0 && pair[1] == 1.0);
    if !plain_decode {
        return None;
    }
    let bytes = source.resolve(image.encoded_data_span).ok()?;
    bytes.starts_with(&[0xff, 0xd8]).then_some(bytes)
}

#[must_use]
pub fn pixels(image: &ImagePaint) -> Option<Image> {
    let (width, height) = (image.width.value, image.height.value);
    if width == 0 || height == 0 {
        return None;
    }
    let count = width as usize * height as usize;
    let mut sample = [0.0f64; 32];
    if image.image_mask.value {
        let mut data = Vec::with_capacity(count);
        for y in 0..height {
            for x in 0..width {
                let painted = image.sample(x, y, 0).is_some_and(|v| v < 0.5);
                data.push(if painted { 0 } else { 255 });
            }
        }
        return Some(Image {
            width,
            height,
            channels: 1,
            data,
        });
    }
    let space = &image.color_space.as_ref()?.value;
    let n = image.components();
    if n == 0 || n > sample.len() {
        return None;
    }
    enum Way {
        Grey,
        Rgb,
        Engine,
    }
    let way = match space {
        ColorSpace::DeviceGray | ColorSpace::CalGray(_) => Way::Grey,
        ColorSpace::DeviceRgb | ColorSpace::CalRgb(_) => Way::Rgb,
        ColorSpace::IccBased(_) => match n {
            1 => Way::Grey,
            3 => Way::Rgb,
            _ => Way::Engine,
        },
        _ => Way::Engine,
    };
    let channels = if matches!(way, Way::Grey) { 1 } else { 3 };
    let mut data = Vec::with_capacity(count * channels);
    let mut cache: HashMap<[u64; 4], [u8; 3]> = HashMap::new();
    for y in 0..height {
        for x in 0..width {
            if !image.sample_pixel(x, y, &mut sample[..n]) {
                sample[..n].iter_mut().for_each(|s| *s = 0.0);
            }
            let s = &sample[..n];
            match way {
                Way::Grey => data.push(to_u8(s[0] * 255.0)),
                Way::Rgb => data.extend(s.iter().take(3).map(|v| to_u8(v * 255.0))),
                Way::Engine => {
                    let convert = || {
                        pdf_render::components_to_rgb(space, s)
                            .map_or([0, 0, 0], |(c, _)| c.map(|v| to_u8(v * 255.0)))
                    };
                    let rgb = if n <= 4 {
                        let mut key = [0u64; 4];
                        for (k, v) in key.iter_mut().zip(s) {
                            *k = v.to_bits();
                        }
                        *cache.entry(key).or_insert_with(convert)
                    } else {
                        convert()
                    };
                    data.extend_from_slice(&rgb);
                }
            }
        }
    }
    Some(Image {
        width,
        height,
        channels,
        data,
    })
}

#[must_use]
pub fn images(graph: &pdf_paint::PaintGraph) -> Vec<&ImagePaint> {
    fn walk<'a>(graph: &'a pdf_paint::PaintGraph, depth: usize, out: &mut Vec<&'a ImagePaint>) {
        for atom in &graph.atoms {
            match &atom.kind {
                pdf_paint::PaintAtomKind::Image(image) => out.push(image),
                pdf_paint::PaintAtomKind::TransparencyGroup(group) if depth < 8 => {
                    walk(&group.graph, depth + 1, out);
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(graph, 0, &mut out);
    out
}
