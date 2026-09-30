use std::sync::Arc;

use convert_pdf_canvas::{Canvas, FontBook};
use convert_structure::FontProvider;
use convert_structure::bytes_tool::{Files, Fonts, Input, Settings};

pub mod numfmt;
pub mod render;
pub mod xlsx;

convert_wasm::export_bytes_tool!(crate::run);

fn options(settings: &Settings, name: &str) -> Result<render::Options, String> {
    let mut options = render::Options {
        file_name: name.to_owned(),
        ..render::Options::default()
    };
    if let Some(fit) = settings.get("fit") {
        options.fit = Some(match fit {
            "width" => render::Fit::Width,
            "page" => render::Fit::Page,
            "none" | "actual" => render::Fit::None,
            other => return Err(format!("fit={other}: use width, page or none")),
        });
    }
    if let Some(paper) = settings.get("paper") {
        options.paper = Some(match paper.to_ascii_lowercase().as_str() {
            "a4" => (595.0, 842.0),
            "a3" => (842.0, 1191.0),
            "a5" => (420.0, 595.0),
            "letter" => (612.0, 792.0),
            "legal" => (612.0, 1008.0),
            other => return Err(format!("paper={other}: use A4, A3, A5, Letter or Legal")),
        });
    }
    if let Some(o) = settings.get("orientation") {
        options.landscape = Some(match o {
            "landscape" => true,
            "portrait" => false,
            other => return Err(format!("orientation={other}: use portrait or landscape")),
        });
    }
    options.grid = matches!(settings.get("grid"), Some("1" | "true" | "yes"));
    Ok(options)
}

pub fn convert(
    bytes: &[u8],
    name: &str,
    settings: &Settings,
    fonts: Option<Arc<dyn FontProvider>>,
    notes: &mut Vec<String>,
) -> Result<Vec<u8>, String> {
    let options = options(settings, name)?;
    let package = convert_office_read::Package::open(bytes).map_err(|e| e.to_string())?;
    let book = xlsx::read(&package)?;
    let mut canvas = Canvas::new();
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    canvas.set_title(stem);
    let mut faces = FontBook::new(fonts);
    notes.extend(book.notes.iter().map(|n| format!("{name}: {n}")));
    let mut pages = 0;
    let mut images = render::Images::default();
    let mut printing = render::Printing {
        canvas: &mut canvas,
        fonts: &mut faces,
        images: &mut images,
        package: &package,
        book: &book,
        options: &options,
        notes,
    };
    for sheet in book.sheets.iter().filter(|s| !s.hidden) {
        pages += render::sheet_pages(&mut printing, sheet);
    }
    if pages == 0 {
        canvas.add_page(595.0, 842.0);
        notes.push(format!("{name}: the workbook has nothing to print"));
    }
    if faces.missing() > 0 {
        notes.push(format!(
            "{name}: {} characters had no font on this machine and show as boxes",
            faces.missing()
        ));
    }
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
