use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::font::Face;
use crate::image::Picture;
use crate::page::{Page, num};
use crate::{CanvasError, Cids};

struct Objects {
    bodies: Vec<Option<Vec<u8>>>,
}

impl Objects {
    fn reserve(&mut self) -> usize {
        self.bodies.push(None);
        self.bodies.len()
    }

    fn set(&mut self, id: usize, body: Vec<u8>) {
        self.bodies[id - 1] = Some(body);
    }

    fn add(&mut self, body: Vec<u8>) -> usize {
        let id = self.reserve();
        self.set(id, body);
        id
    }

    fn dict(&mut self, text: String) -> usize {
        self.add(text.into_bytes())
    }

    fn stream(&mut self, dict: &str, data: &[u8]) -> usize {
        let mut body = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.add(body)
    }

    fn deflated(&mut self, dict: &str, data: &[u8]) -> usize {
        let packed = pdf_syntax::deflate_zlib(data);
        self.stream(&format!("{dict} /Filter /FlateDecode"), &packed)
    }
}

fn text_string(text: &str) -> String {
    let mut out = String::from("<FEFF");
    for unit in text.encode_utf16() {
        let _ = write!(out, "{unit:04X}");
    }
    out.push('>');
    out
}

fn name(text: &str) -> String {
    let mut out = String::from("/");
    for b in text.bytes() {
        if b.is_ascii_graphic() && !b"()<>[]{}/%#".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "#{b:02X}");
        }
    }
    out
}

fn subset_tag(seed: &str, glyphs: &[(u16, String, i32)]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |b: u8| {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    };
    seed.bytes().for_each(&mut feed);
    for (gid, _, _) in glyphs {
        gid.to_be_bytes().into_iter().for_each(&mut feed);
    }
    (0..6)
        .map(|k| char::from(b'A' + ((hash >> (k * 5)) % 26) as u8))
        .collect()
}

fn to_unicode<'a>(codes: impl Iterator<Item = (u16, &'a str)>) -> String {
    let entries: Vec<String> = codes
        .filter(|(_, meaning)| !meaning.is_empty())
        .map(|(code, meaning)| {
            let mut entry = format!("<{code:04X}> <");
            for unit in meaning.encode_utf16() {
                let _ = write!(entry, "{unit:04X}");
            }
            entry.push('>');
            entry
        })
        .collect();
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in entries.chunks(100) {
        let _ = writeln!(out, "{} beginbfchar", chunk.len());
        for entry in chunk {
            out.push_str(entry);
            out.push('\n');
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

fn embed_font(objects: &mut Objects, face: &Face, cids: &Cids) -> Result<usize, CanvasError> {
    let metrics = &face.metrics;
    let cff = face.cff();
    let glyphs: Vec<(u16, String, i32)> = if cff.is_some() {
        cids.fixed.values().cloned().collect()
    } else {
        cids.glyphs.clone()
    };
    let gids: BTreeSet<u16> = glyphs.iter().map(|(gid, _, _)| *gid).collect();
    let (program, file_key) = if let Some((_, data)) = cff {
        let (subset, _) = pdf_font::subset::subset_cff(data, &gids)
            .map_err(|error| CanvasError::Subset(format!("{error:?}")))?;
        (
            objects.deflated("/Subtype /CIDFontType0C", &subset),
            "/FontFile3",
        )
    } else {
        let subset = pdf_font::subset::subset_truetype(face.truetype(), &gids)
            .map_err(|error| CanvasError::Subset(format!("{error:?}")))?;
        (
            objects.deflated(
                &format!("/Length1 {}", subset.program.len()),
                &subset.program,
            ),
            "/FontFile2",
        )
    };
    let scale = 1000.0 / f32::from(metrics.units_per_em.max(1));
    let unit = |v: i32| num(v as f32 * scale);
    let base = {
        let raw = if metrics.postscript_name.is_empty() {
            metrics.family.replace(' ', "")
        } else {
            metrics.postscript_name.clone()
        };
        let clean: String = raw.chars().filter(char::is_ascii_graphic).collect();
        let clean = if clean.is_empty() {
            "Font".to_owned()
        } else {
            clean
        };
        format!("{}+{clean}", subset_tag(&clean, &glyphs))
    };
    let mut flags = 4;
    if metrics.italic {
        flags |= 64;
    }
    let descriptor = objects.dict(format!(
        "<< /Type /FontDescriptor /FontName {} /Flags {flags} /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV {} {file_key} {program} 0 R >>",
        name(&base),
        unit(metrics.bbox[0]),
        unit(metrics.bbox[1]),
        unit(metrics.bbox[2]),
        unit(metrics.bbox[3]),
        num(metrics.italic_angle as f32),
        unit(metrics.ascent),
        unit(metrics.descent),
        unit(metrics.cap_height),
        if metrics.bold { 120 } else { 80 },
    ));
    let (descendant, unicode) = if cff.is_some() {
        let mut widths = String::from("[");
        for (code, (_, _, width)) in &cids.fixed {
            let _ = write!(widths, "{code} [{}] ", num(*width as f32 * scale));
        }
        widths.push(']');
        let descendant = objects.dict(format!(
            "<< /Type /Font /Subtype /CIDFontType0 /BaseFont {} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {descriptor} 0 R /DW 0 /W {widths} >>",
            name(&base)
        ));
        let codes = cids
            .fixed
            .iter()
            .map(|(code, (_, text, _))| (*code, text.as_str()));
        (descendant, to_unicode(codes))
    } else {
        let mut map = vec![0_u8; (glyphs.len() + 1) * 2];
        for (index, (gid, _, _)) in glyphs.iter().enumerate() {
            map[(index + 1) * 2..(index + 2) * 2].copy_from_slice(&gid.to_be_bytes());
        }
        let cid_to_gid = objects.deflated("", &map);
        let mut widths = String::from("[1 [");
        for (_, _, width) in &glyphs {
            let _ = write!(widths, "{} ", num(*width as f32 * scale));
        }
        widths.push_str("]]");
        let descendant = objects.dict(format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont {} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {descriptor} 0 R /DW 0 /W {widths} /CIDToGIDMap {cid_to_gid} 0 R >>",
            name(&base)
        ));
        let codes = glyphs
            .iter()
            .enumerate()
            .map(|(index, (_, text, _))| (u16::try_from(index + 1).unwrap_or(0), text.as_str()));
        (descendant, to_unicode(codes))
    };
    let unicode = objects.deflated("", unicode.as_bytes());
    Ok(objects.dict(format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont {} /Encoding /Identity-H /DescendantFonts [{descendant} 0 R] /ToUnicode {unicode} 0 R >>",
        name(&base)
    )))
}

fn embed_image(objects: &mut Objects, picture: &Picture) -> usize {
    let mask = picture.alpha.as_ref().map(|alpha| {
        objects.stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode",
                picture.width, picture.height
            ),
            alpha,
        )
    });
    let space = picture.palette.as_ref().map_or_else(
        || picture.space.name().to_owned(),
        |palette| {
            let mut hex = String::with_capacity(palette.len() * 2);
            for b in palette {
                let _ = write!(hex, "{b:02X}");
            }
            format!(
                "[/Indexed /DeviceRGB {} <{hex}>]",
                (palette.len() / 3).clamp(1, 256) - 1
            )
        },
    );
    let mut dict = format!(
        "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {space} /BitsPerComponent 8 /Filter {}",
        picture.width,
        picture.height,
        if picture.jpeg {
            "/DCTDecode"
        } else {
            "/FlateDecode"
        }
    );
    if picture.inverted {
        dict.push_str(" /Decode [1 0 1 0 1 0 1 0]");
    }
    if let Some(mask) = mask {
        let _ = write!(dict, " /SMask {mask} 0 R");
    }
    if let Some(key) = picture.key {
        let _ = write!(dict, " /Mask [{key} {key}]");
    }
    objects.stream(&dict, &picture.data)
}

pub(crate) fn document(
    pages: &[Page],
    faces: &[Face],
    cids: &[Cids],
    used_fonts: &BTreeSet<usize>,
    images: &[Picture],
    title: Option<&str>,
    language: Option<&str>,
) -> Result<Vec<u8>, CanvasError> {
    let mut objects = Objects { bodies: Vec::new() };
    let catalog = objects.reserve();
    let tree = objects.reserve();
    let mut font_objects = BTreeMap::new();
    for &font in used_fonts {
        font_objects.insert(font, embed_font(&mut objects, &faces[font], &cids[font])?);
    }
    let used_images: BTreeSet<usize> = pages
        .iter()
        .flat_map(|p| p.images_used.iter().copied())
        .collect();
    let mut image_objects = BTreeMap::new();
    for &image in &used_images {
        image_objects.insert(image, embed_image(&mut objects, &images[image]));
    }
    let mut kids = Vec::with_capacity(pages.len());
    for page in pages {
        let contents = objects.deflated("", page.content.as_bytes());
        let mut resources = String::from("<<");
        if !page.fonts_used.is_empty() {
            resources.push_str(" /Font <<");
            for font in &page.fonts_used {
                let _ = write!(resources, " /F{font} {} 0 R", font_objects[font]);
            }
            resources.push_str(" >>");
        }
        if !page.images_used.is_empty() {
            resources.push_str(" /XObject <<");
            for image in &page.images_used {
                let _ = write!(resources, " /Im{image} {} 0 R", image_objects[image]);
            }
            resources.push_str(" >>");
        }
        if !page.alphas.is_empty() || !page.soft_masks.is_empty() {
            resources.push_str(" /ExtGState <<");
            for (fill, stroke) in &page.alphas {
                let _ = write!(
                    resources,
                    " /A{fill}_{stroke} << /Type /ExtGState /ca {} /CA {} >>",
                    num(f32::from(*fill) / 255.0),
                    num(f32::from(*stroke) / 255.0)
                );
            }
            for (n, mask) in page.soft_masks.iter().enumerate() {
                let (dict, content) = crate::gradient::soft_mask_form(mask);
                let form = objects.stream(&dict, content.as_bytes());
                let _ = write!(
                    resources,
                    " /SM{n} << /Type /ExtGState /SMask << /Type /Mask /S /Luminosity /G {form} 0 R >> >>"
                );
            }
            resources.push_str(" >>");
        }
        if !page.patterns.is_empty() {
            resources.push_str(" /Pattern <<");
            for (n, pattern) in page.patterns.iter().enumerate() {
                let _ = write!(resources, " /P{n} {pattern}");
            }
            resources.push_str(" >>");
        }
        resources.push_str(" >>");
        let mut annots = String::new();
        if !page.links.is_empty() {
            annots.push_str(" /Annots [");
            for (rect, uri) in &page.links {
                let mut literal = String::from("(");
                for c in uri.chars() {
                    match c {
                        '(' | ')' | '\\' => {
                            literal.push('\\');
                            literal.push(c);
                        }
                        c if c.is_ascii() && !c.is_ascii_control() => literal.push(c),
                        c => {
                            let mut buffer = [0_u8; 4];
                            for b in c.encode_utf8(&mut buffer).bytes() {
                                let _ = write!(literal, "%{b:02X}");
                            }
                        }
                    }
                }
                literal.push(')');
                let _ = write!(
                    annots,
                    "<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] /A << /S /URI /URI {literal} >> >> ",
                    num(rect[0]),
                    num(rect[1]),
                    num(rect[2]),
                    num(rect[3])
                );
            }
            annots.push(']');
        }
        kids.push(objects.dict(format!(
            "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 {} {}] /Resources {resources} /Contents {contents} 0 R{annots} >>",
            num(page.width),
            num(page.height)
        )));
    }
    let kid_list: Vec<String> = kids.iter().map(|k| format!("{k} 0 R")).collect();
    objects.set(
        tree,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kid_list.join(" "),
            kids.len()
        )
        .into_bytes(),
    );
    let mut catalog_dict = format!("<< /Type /Catalog /Pages {tree} 0 R");
    if let Some(language) = language {
        let _ = write!(catalog_dict, " /Lang {}", text_string(language));
    }
    if title.is_some() {
        catalog_dict.push_str(" /ViewerPreferences << /DisplayDocTitle true >>");
    }
    catalog_dict.push_str(" >>");
    objects.set(catalog, catalog_dict.into_bytes());
    let mut info = String::from("<< /Producer (PanPDF)");
    if let Some(title) = title {
        let _ = write!(info, " /Title {}", text_string(title));
    }
    info.push_str(" >>");
    let info = objects.dict(info);

    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.bodies.len());
    for (index, body) in objects.bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body.as_deref().unwrap_or(b"null"));
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let mut table = format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.bodies.len() + 1
    );
    for offset in offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    let _ = write!(
        table,
        "trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.bodies.len() + 1
    );
    out.extend_from_slice(table.as_bytes());
    Ok(out)
}
