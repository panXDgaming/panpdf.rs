use convert_files::{Input, Job, Outcome, Settings};
use pdf_bytes::ByteStore;
use pdf_edit::Command;
use pdf_edit::text_layer::TextLayer;
use pdf_session::Session;

pub use convert_files;

#[cfg(not(target_arch = "wasm32"))]
mod desktop;
pub mod given;

pub fn start(inputs: Vec<Input>, settings: &Settings) -> Result<Box<dyn Job>, String> {
    if given::asked(&inputs, settings) {
        return given::start(inputs, settings);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        desktop::start(inputs, settings)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (inputs, settings);
        Err(
            "in a browser the page runs Tesseract: ask this bundle to survey, draw, judge or write"
                .to_owned(),
        )
    }
}

pub(crate) fn write(
    stem: &str,
    source: &ByteStore,
    password: &[u8],
    layers: Vec<(usize, TextLayer)>,
    mut notes: Vec<String>,
) -> Result<Outcome, String> {
    let source = &editable(source, password, &mut notes)?;
    let mut session = Session::new(source.clone(), password);
    let mut kept = Vec::new();
    for (index, layer) in layers {
        let alone = Command::TextLayer {
            page_index: index,
            layer: layer.clone(),
            share_from: None,
        };
        match session.plan(&alone) {
            Ok(_) => kept.push((index, layer)),
            Err(e) => notes.push(format!("page {}: not written: {e}", index + 1)),
        }
    }
    let Some(&(first, _)) = kept.first() else {
        return Err(notes.join("; "));
    };
    let written = kept.len();
    let commands: Vec<Command> = kept
        .into_iter()
        .map(|(page_index, layer)| Command::TextLayer {
            page_index,
            layer,
            share_from: (page_index != first).then_some(first),
        })
        .collect();
    session.apply_each(&commands).map_err(|e| e.to_string())?;
    notes.push(format!("{written} pages made searchable"));
    Ok(Outcome {
        files: vec![(format!("{stem}-ocr.pdf"), session.source().to_vec())],
        notes,
    })
}

convert_files::export_files_tool!(crate::start);

fn editable(
    source: &ByteStore,
    password: &[u8],
    notes: &mut Vec<String>,
) -> Result<ByteStore, String> {
    if pdf_edit::Document::open_strict(source.clone(), pdf_syntax::XrefLimits::default()).is_ok() {
        return Ok(source.clone());
    }
    let bytes = source.to_vec();
    let compress = convert_pdfdoc::uses_object_streams(&bytes);
    let doc = convert_pdfdoc::load(
        bytes,
        &convert_pdfdoc::Load {
            password: password.to_vec(),
            recover: true,
            everything: false,
        },
    )
    .map_err(|e| format!("the file cannot be read as a PDF: {e}"))?;
    let protection = doc.kept.as_ref().map(|kept| convert_pdfdoc::Protection {
        dictionary: &kept.dictionary,
        security: &kept.security,
    });
    let written = convert_pdfdoc::write(&doc, protection.as_ref(), compress)?;
    notes.push(
        "the file was written again as one clean revision first, as it had small faults".to_owned(),
    );
    Ok(ByteStore::owning(
        pdf_bytes::SourceId::next_document(),
        written.bytes,
    ))
}
