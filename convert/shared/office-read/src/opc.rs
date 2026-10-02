use crate::xml::{self, Element};
use crate::zip::{Zip, ZipError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rel {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub external: bool,
}

impl Rel {
    #[must_use]
    pub fn is(&self, suffix: &str) -> bool {
        self.kind
            .rsplit('/')
            .next()
            .is_some_and(|last| last == suffix)
    }
}

#[must_use]
pub fn rels_name(part: &str) -> String {
    let part = part.trim_start_matches('/');
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

#[must_use]
pub fn resolve(part: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or(target);
    let mut segments: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        let part = part.trim_start_matches('/');
        let mut dir: Vec<&str> = part.split('/').collect();
        dir.pop();
        dir
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    let joined = segments.join("/");
    percent_decode(&joined)
}

fn percent_decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(value) = text
                .get(at + 1..at + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageError {
    Zip(ZipError),
    Xml { part: String, error: xml::XmlError },
    Missing(String),
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Zip(error) => error.fmt(f),
            Self::Xml { part, error } => write!(f, "{part}: {error}"),
            Self::Missing(part) => write!(f, "the file has no part '{part}'"),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<ZipError> for PackageError {
    fn from(error: ZipError) -> Self {
        Self::Zip(error)
    }
}

#[derive(Clone, Debug)]
pub struct Package<'a> {
    pub zip: Zip<'a>,
}

impl<'a> Package<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, PackageError> {
        Ok(Self {
            zip: Zip::read(data)?,
        })
    }

    #[must_use]
    pub fn has(&self, part: &str) -> bool {
        self.zip.entry(part).is_some()
    }

    pub fn bytes(&self, part: &str) -> Result<Vec<u8>, PackageError> {
        self.zip
            .get(part)
            .ok_or_else(|| PackageError::Missing(part.to_owned()))?
            .map_err(PackageError::from)
    }

    pub fn xml(&self, part: &str) -> Result<Element, PackageError> {
        let bytes = self.bytes(part)?;
        xml::parse_bytes(&bytes).map_err(|error| PackageError::Xml {
            part: part.to_owned(),
            error,
        })
    }

    #[must_use]
    pub fn rels(&self, part: &str) -> Vec<Rel> {
        let name = if part.is_empty() {
            "_rels/.rels".to_owned()
        } else {
            rels_name(part)
        };
        let Ok(root) = self.xml(&name) else {
            return Vec::new();
        };
        root.children_named("Relationship")
            .map(|rel| {
                let external = rel
                    .attr("TargetMode")
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("External"));
                let target = rel.attr("Target").unwrap_or_default();
                Rel {
                    id: rel.attr("Id").unwrap_or_default().to_owned(),
                    kind: rel.attr("Type").unwrap_or_default().to_owned(),
                    target: if external {
                        target.to_owned()
                    } else {
                        resolve(part, target)
                    },
                    external,
                }
            })
            .collect()
    }

    #[must_use]
    pub fn main_part(&self) -> Option<String> {
        self.rels("")
            .into_iter()
            .find(|rel| rel.is("officeDocument") && !rel.external)
            .map(|rel| rel.target)
    }

    #[must_use]
    pub fn content_type(&self, part: &str) -> Option<String> {
        let types = self.xml("[Content_Types].xml").ok()?;
        let wanted = format!("/{}", part.trim_start_matches('/'));
        if let Some(found) = types.children_named("Override").find(|o| {
            o.attr("PartName")
                .is_some_and(|p| p.eq_ignore_ascii_case(&wanted))
        }) {
            return found.attr("ContentType").map(str::to_owned);
        }
        let extension = part.rsplit_once('.')?.1;
        types
            .children_named("Default")
            .find(|d| {
                d.attr("Extension")
                    .is_some_and(|e| e.eq_ignore_ascii_case(extension))
            })
            .and_then(|d| d.attr("ContentType"))
            .map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_targets() {
        assert_eq!(
            resolve("ppt/slides/slide1.xml", "../media/image1.png"),
            "ppt/media/image1.png"
        );
        assert_eq!(
            resolve("xl/workbook.xml", "worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            resolve("xl/workbook.xml", "/xl/styles.xml"),
            "xl/styles.xml"
        );
        assert_eq!(resolve("", "word/document.xml"), "word/document.xml");
        assert_eq!(resolve("a/b.xml", "c%20d.png"), "a/c d.png");
        assert_eq!(
            rels_name("ppt/slides/slide1.xml"),
            "ppt/slides/_rels/slide1.xml.rels"
        );
    }

    #[test]
    fn package_relationships() {
        let mut writer = convert_zip::ZipWriter::new();
        let add = |w: &mut convert_zip::ZipWriter, name: &str, text: &str| {
            w.add(name, text.as_bytes(), convert_zip::Method::Deflated)
                .unwrap();
        };
        add(
            &mut writer,
            "_rels/.rels",
            r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        );
        add(
            &mut writer,
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="rId1" Type="x/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId9" Type="x/hyperlink" Target="https://a.b/" TargetMode="External"/></Relationships>"#,
        );
        add(
            &mut writer,
            "[Content_Types].xml",
            r#"<Types><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="wb"/></Types>"#,
        );
        let bytes = writer.finish().unwrap();
        let package = Package::open(&bytes).unwrap();
        assert_eq!(package.main_part().as_deref(), Some("xl/workbook.xml"));
        let rels = package.rels("xl/workbook.xml");
        assert_eq!(rels[0].target, "xl/worksheets/sheet1.xml");
        assert!(rels[0].is("worksheet"));
        assert!(rels[1].external);
        assert_eq!(rels[1].target, "https://a.b/");
        assert_eq!(
            package.content_type("xl/workbook.xml").as_deref(),
            Some("wb")
        );
        assert_eq!(
            package.content_type("xl/x.xml").as_deref(),
            Some("application/xml")
        );
        assert!(package.rels("xl/none.xml").is_empty());
    }
}
