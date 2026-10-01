use std::collections::HashMap;
use std::sync::Arc;

use convert_office_read::{Element, Package};
use convert_pdf_canvas::{Canvas, FontBook};
use convert_structure::FontProvider;
use convert_structure::bytes_tool::{Files, Fonts, Input, Settings};

pub mod slides;

pub use convert_drawingml::theme;

convert_wasm::export_bytes_tool!(crate::run);

type Master = (Option<Element>, theme::Theme, HashMap<String, String>);

fn clr_map(master: Option<&Element>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(m) = master.and_then(|m| m.child("clrMap")) {
        for (k, v) in &m.attrs {
            map.insert(k.clone(), v.clone());
        }
    }
    if map.is_empty() {
        for (k, v) in [
            ("bg1", "lt1"),
            ("tx1", "dk1"),
            ("bg2", "lt2"),
            ("tx2", "dk2"),
        ] {
            map.insert(k.to_owned(), v.to_owned());
        }
    }
    map
}

pub fn convert(
    bytes: &[u8],
    name: &str,
    settings: &Settings,
    fonts: Option<Arc<dyn FontProvider>>,
    notes: &mut Vec<String>,
) -> Result<Vec<u8>, String> {
    let package = Package::open(bytes).map_err(|e| e.to_string())?;
    let main = package
        .main_part()
        .unwrap_or_else(|| "ppt/presentation.xml".to_owned());
    let presentation = package.xml(&main).map_err(|e| e.to_string())?;
    if presentation.local() != "presentation" {
        return Err("this is not a PowerPoint presentation".to_owned());
    }
    let size = presentation.child("sldSz");
    let width = size
        .and_then(|s| s.attr("cx"))
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(9_144_000.0)
        / 12_700.0;
    let height = size
        .and_then(|s| s.attr("cy"))
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(6_858_000.0)
        / 12_700.0;
    let rels = package.rels(&main);
    let include_hidden = matches!(settings.get("hidden"), Some("1" | "true" | "yes"));
    let mut canvas = Canvas::new();
    canvas.set_title(name.rsplit_once('.').map_or(name, |(s, _)| s));
    let mut faces = FontBook::new(fonts);
    let mut images = HashMap::new();
    let default_text = presentation.child("defaultTextStyle").cloned();
    let mut masters: HashMap<String, Master> = HashMap::new();
    let list: Vec<&Element> = presentation
        .child("sldIdLst")
        .map(|l| l.children_named("sldId").collect())
        .unwrap_or_default();
    let mut printed = 0;
    let mut skipped = 0;
    for entry in list {
        let Some(rel) = entry
            .attr("r:id")
            .or_else(|| entry.attr("id"))
            .and_then(|id| rels.iter().find(|r| r.id == id))
        else {
            continue;
        };
        let Ok(slide) = package.xml(&rel.target) else {
            notes.push(format!("{}: unreadable, left out", rel.target));
            continue;
        };
        if slide.attr("show") == Some("0") && !include_hidden {
            skipped += 1;
            continue;
        }
        let layout_part = package
            .rels(&rel.target)
            .into_iter()
            .find(|r| r.is("slideLayout"))
            .map(|r| r.target);
        let layout = layout_part.as_deref().and_then(|p| package.xml(p).ok());
        let master_part = layout_part
            .as_deref()
            .and_then(|p| package.rels(p).into_iter().find(|r| r.is("slideMaster")))
            .map(|r| r.target)
            .unwrap_or_default();
        let (master, theme, map) = masters
            .entry(master_part.clone())
            .or_insert_with(|| {
                let master = package.xml(&master_part).ok();
                let theme_root = package
                    .rels(&master_part)
                    .into_iter()
                    .find(|r| r.is("theme"))
                    .and_then(|r| package.xml(&r.target).ok());
                let theme = theme::Theme::read(theme_root.as_ref());
                let map = clr_map(master.as_ref());
                (master, theme, map)
            })
            .clone();
        let parts = slides::Parts {
            slide,
            slide_part: rel.target.clone(),
            layout,
            layout_part: layout_part.unwrap_or_default(),
            master,
            master_part,
            theme,
            map,
        };
        let page = canvas.add_page(width, height);
        let mut drawer = slides::Slide {
            canvas: &mut canvas,
            fonts: &mut faces,
            package: &package,
            images: &mut images,
            default_text: default_text.as_ref(),
            parts: &parts,
            page,
            height,
            notes,
        };
        drawer.draw();
        printed += 1;
    }
    if printed == 0 {
        canvas.add_page(width, height);
        notes.push(format!("{name}: no slide to print"));
    }
    if skipped > 0 {
        notes.push(format!("{name}: {skipped} hidden slide(s) left out"));
    }
    if faces.missing() > 0 {
        notes.push(format!(
            "{name}: {} characters had no font on this machine and show as boxes",
            faces.missing()
        ));
    }
    notes.sort();
    notes.dedup();
    canvas.finish().map_err(|e| e.to_string())
}

pub fn run(
    inputs: &[Input],
    settings: &Settings,
    fonts: &Fonts,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Files, String> {
    let fonts = fonts.layout_provider();
    let mut files = Files::default();
    let mut errors = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        if !progress(index, inputs.len()) {
            return Err("cancelled".to_owned());
        }
        let stem = input
            .name
            .rsplit_once('.')
            .map_or(input.name.as_str(), |(s, _)| s);
        match convert(
            &input.bytes,
            &input.name,
            settings,
            fonts.clone(),
            &mut files.notes,
        ) {
            Ok(pdf) => files.files.push((format!("{stem}.pdf"), pdf)),
            Err(e) => errors.push(format!("{}: {e}", input.name)),
        }
    }
    progress(inputs.len(), inputs.len());
    if files.files.is_empty() {
        return Err(errors.join("\n"));
    }
    files.notes.extend(errors);
    Ok(files)
}
