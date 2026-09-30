use convert_pdfdoc::{Load, LoadError};
use convert_pdftool::{Job, Output, Tool};

pub static TOOL: Tool = Tool {
    name: "unlock-pdf",
    extension: "pdf",
    inputs: 1,
    file_options: &[],
    flags: &[],
    help: "  --password PW   the password that opens the file (not needed when it opens without one)",
    run,
};

convert_pdftool::export_pdf_tool!(crate::TOOL);

pub fn run(job: &Job) -> Result<Output, String> {
    let bytes = job.pdf()?.to_vec();
    let compress = convert_pdfdoc::uses_object_streams(&bytes);
    let doc = convert_pdfdoc::load(
        bytes,
        &Load {
            password: job.password(),
            recover: true,
            everything: false,
        },
    )
    .map_err(|e| match e {
        LoadError::Password if job.password().is_empty() => {
            "the file needs its password to be opened: give it with --password".to_owned()
        }
        LoadError::Password => "that password does not open the file".to_owned(),
        other => other.to_string(),
    })?;
    let mut notes = Vec::new();
    if !doc.was_encrypted {
        notes.push("the file was not protected; it is written again unchanged in content".into());
    }
    if doc.recovered {
        notes.push(format!(
            "the file's cross-reference was damaged and was rebuilt to read it ({} repairs)",
            doc.notes.len()
        ));
    }
    if convert_pdfdoc::is_signed(&doc) {
        notes.push("the file's digital signatures do not survive unlocking it".into());
    }
    let pages = doc.pages().len();
    let written = convert_pdfdoc::write(&doc, None, compress)?;
    convert_pdfdoc::check_written(&written.bytes, b"", pages)?;
    notes.push(format!(
        "unlocked: {pages} pages, no password, no restrictions"
    ));
    Ok(Output {
        main: written.bytes,
        attachments: Vec::new(),
        notes,
    })
}
