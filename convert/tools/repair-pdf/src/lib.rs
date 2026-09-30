use std::collections::BTreeSet;

use convert_pdfdoc::{Dict, Doc, Load, Object, Value};
use convert_pdftool::{Job, Output, Tool};

pub static TOOL: Tool = Tool {
    name: "repair-pdf",
    extension: "pdf",
    inputs: 1,
    file_options: &[],
    flags: &[],
    help: "  --password PW   when the file is protected",
    run,
};

convert_pdftool::export_pdf_tool!(crate::TOOL);

#[derive(Debug, Default)]
pub struct Report {
    pub repairs: Vec<String>,
    pub pages: usize,
    pub pages_recovered: usize,
    pub catalog_made: bool,
    pub blank_page_made: bool,
    pub objects_dropped: usize,
}

pub fn repair(bytes: Vec<u8>, password: &[u8]) -> Result<(Vec<u8>, Report), String> {
    let mut doc = convert_pdfdoc::load(
        bytes,
        &Load {
            password: password.to_vec(),
            recover: true,
            everything: true,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut report = Report {
        repairs: std::mem::take(&mut doc.notes),
        ..Report::default()
    };
    if doc.root().is_none() {
        let mut catalog = Dict::new();
        catalog.set("Type", Value::name("Catalog"));
        let root = doc.add(Object::new(Value::Dict(catalog)));
        doc.trailer.set("Root", Value::Ref(root));
        report.catalog_made = true;
    }
    let (count, recovered, blank) = mend_pages(&mut doc)?;
    report.pages = count;
    report.pages_recovered = recovered;
    report.blank_page_made = blank;
    let written = convert_pdfdoc::write(&doc, None, false)?;
    report.objects_dropped = written.dropped;
    convert_pdfdoc::check_written(&written.bytes, b"", count)?;
    Ok((written.bytes, report))
}

fn mend_pages(doc: &mut Doc) -> Result<(usize, usize, bool), String> {
    let walked: Vec<u32> = doc.pages().iter().map(|p| p.number).collect();
    let walked_set: BTreeSet<u32> = walked.iter().copied().collect();
    let lost: Vec<u32> = doc
        .objects
        .iter()
        .filter(|(n, o)| !walked_set.contains(n) && o.dict().is_some_and(|d| d.is("Type", "Page")))
        .map(|(n, _)| *n)
        .collect();
    let has_tree = doc
        .catalog()
        .and_then(|c| c.get("Pages"))
        .and_then(Value::as_ref)
        .is_some_and(|n| doc.get(n).is_some());
    if lost.is_empty() && has_tree && !walked.is_empty() {
        return Ok((walked.len(), 0, false));
    }
    let mut pages: Vec<u32> = walked.iter().chain(&lost).copied().collect();
    let mut made_blank = false;
    if pages.is_empty() {
        let mut page = Dict::new();
        page.set("Type", Value::name("Page"));
        page.set("Resources", Value::Dict(Dict::new()));
        pages.push(doc.add(Object::new(Value::Dict(page))));
        made_blank = true;
    }
    for &page in &pages {
        let inherited = inherited(doc, page);
        if let Some(dict) = doc.get_mut(page).and_then(Object::dict_mut) {
            for (key, value) in inherited {
                if !dict.has(&key) {
                    dict.set(&key, value);
                }
            }
        }
    }
    let tree = doc.next_number();
    for &page in &pages {
        if let Some(dict) = doc.get_mut(page).and_then(Object::dict_mut) {
            dict.set("Parent", Value::Ref(tree));
            dict.set("Type", Value::name("Page"));
            if !dict.has("MediaBox") {
                dict.set(
                    "MediaBox",
                    Value::Array(vec![
                        Value::Int(0),
                        Value::Int(0),
                        Value::Int(612),
                        Value::Int(792),
                    ]),
                );
            }
        }
    }
    let mut node = Dict::new();
    node.set("Type", Value::name("Pages"));
    node.set(
        "Kids",
        Value::Array(pages.iter().map(|&n| Value::Ref(n)).collect()),
    );
    node.set(
        "Count",
        Value::Int(i64::try_from(pages.len()).unwrap_or(i64::MAX)),
    );
    doc.objects.insert(tree, Object::new(Value::Dict(node)));
    let catalog = doc
        .catalog_mut()
        .ok_or_else(|| "the catalogue is not a dictionary".to_owned())?;
    catalog.set("Pages", Value::Ref(tree));
    Ok((pages.len(), lost.len(), made_blank))
}

fn inherited(doc: &Doc, page: u32) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut at = doc
        .get(page)
        .and_then(Object::dict)
        .and_then(|d| d.get("Parent"))
        .and_then(Value::as_ref);
    while let Some(number) = at {
        if !seen.insert(number) {
            break;
        }
        let Some(dict) = doc.get(number).and_then(Object::dict) else {
            break;
        };
        for key in ["Resources", "MediaBox", "CropBox", "Rotate"] {
            if !out.iter().any(|(k, _)| k == key)
                && let Some(value) = dict.get(key)
            {
                out.push((key.to_owned(), value.clone()));
            }
        }
        at = dict.get("Parent").and_then(Value::as_ref);
    }
    out
}

pub fn run(job: &Job) -> Result<Output, String> {
    let (bytes, report) = repair(job.pdf()?.to_vec(), &job.password())?;
    let mut notes = Vec::new();
    if report.repairs.is_empty()
        && report.pages_recovered == 0
        && !report.catalog_made
        && !report.blank_page_made
    {
        notes.push("no damage was found; the file was written again as one clean revision".into());
    } else {
        notes.push(format!(
            "{} repairs made while reading",
            report.repairs.len()
        ));
        notes.extend(report.repairs.iter().take(20).map(|r| format!("  {r}")));
        if report.repairs.len() > 20 {
            notes.push(format!("  ... and {} more", report.repairs.len() - 20));
        }
    }
    if report.catalog_made {
        notes.push("the document catalogue was lost and a new one was made".into());
    }
    if report.blank_page_made {
        notes.push("no page was left in the file; one blank page was made so it opens".into());
    }
    if report.pages_recovered > 0 {
        notes.push(format!(
            "{} pages the page tree had lost were put back, after the others",
            report.pages_recovered
        ));
    }
    notes.push(format!(
        "{} pages; {} unused objects left out",
        report.pages, report.objects_dropped
    ));
    Ok(Output {
        main: bytes,
        attachments: Vec::new(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_pages(kids: &str) -> Vec<u8> {
        let content = "BT /F1 12 Tf 10 50 Td (Page) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Kids [{kids}] /Count 2 /MediaBox [0 0 200 100] /Resources << /Font << /F1 6 0 R >> >> >>"
            ),
            "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_owned(),
            format!("<< /Length 999 >>\nstream\n{content}\nendstream"),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        for (at, body) in objects.iter().enumerate() {
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", at + 1).as_bytes());
        }
        out.extend_from_slice(b"trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n12345\n%%EOF\n");
        out
    }

    #[test]
    fn a_file_without_a_cross_reference_is_rebuilt() {
        let (bytes, report) = repair(two_pages("3 0 R 4 0 R"), b"").unwrap();
        assert_eq!(report.pages, 2);
        assert!(!report.repairs.is_empty());
        let doc = convert_pdfdoc::load(bytes, &Load::default()).unwrap();
        assert_eq!(doc.pages().len(), 2);
        let page = doc.get(doc.pages()[0].number).unwrap();
        let contents = page
            .dict()
            .unwrap()
            .get("Contents")
            .unwrap()
            .as_ref()
            .unwrap();
        assert!(
            doc.get(contents)
                .unwrap()
                .stream
                .as_ref()
                .unwrap()
                .starts_with(b"BT")
        );
    }

    #[test]
    fn lost_pages_are_put_back_with_what_they_inherited() {
        let (bytes, report) = repair(two_pages("3 0 R 3 0 R"), b"").unwrap();
        assert_eq!(report.pages, 2);
        assert_eq!(report.pages_recovered, 1);
        let doc = convert_pdfdoc::load(bytes, &Load::default()).unwrap();
        assert_eq!(doc.pages().len(), 2);
        for page in doc.pages() {
            assert_eq!(page.media_box, [0.0, 0.0, 200.0, 100.0]);
            assert!(
                doc.get(page.number)
                    .unwrap()
                    .dict()
                    .unwrap()
                    .has("Resources")
            );
        }
    }
}
