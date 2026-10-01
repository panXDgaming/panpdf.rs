#![forbid(unsafe_code)]

use convert_structure::model::Style;
use convert_structure::script::Script;
use convert_xml::Writer;
use convert_zip::{Method, ZipError, ZipWriter};

#[must_use]
pub fn twips(points: f64) -> i64 {
    rounded(points * 20.0)
}

#[must_use]
pub fn half_points(points: f64) -> i64 {
    rounded(points * 2.0).max(2)
}

#[must_use]
pub fn emu(points: f64) -> i64 {
    rounded(points * 12_700.0)
}

fn rounded(value: f64) -> i64 {
    if !value.is_finite() {
        return 0;
    }
    let value = value.round().clamp(-9.0e15, 9.0e15);
    #[expect(clippy::cast_possible_truncation)]
    {
        value as i64
    }
}

#[must_use]
pub fn hex(color: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

pub mod types {
    pub const RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
    pub const XML: &str = "application/xml";
    pub const CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
    pub const APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
}

pub mod rel {
    pub const OFFICE_DOCUMENT: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    pub const CORE: &str =
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
    pub const APP: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
    pub const IMAGE: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
    pub const STYLES: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
    pub const NUMBERING: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
    pub const SETTINGS: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
    pub const THEME: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
    pub const WORKSHEET: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
    pub const SHARED_STRINGS: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
    pub const SLIDE: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
    pub const SLIDE_LAYOUT: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
    pub const SLIDE_MASTER: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
    pub const PRES_PROPS: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps";
    pub const HEADER: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
    pub const FOOTER: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
}

#[derive(Debug, Default)]
pub struct Rels {
    entries: Vec<(String, String, String)>,
}

impl Rels {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, kind: &str, target: &str) -> String {
        let id = format!("rId{}", self.entries.len() + 1);
        self.entries
            .push((id.clone(), kind.to_owned(), target.to_owned()));
        id
    }

    #[must_use]
    pub fn xml(&self) -> String {
        let mut w = Writer::new();
        w.open(
            "Relationships",
            &[(
                "xmlns",
                "http://schemas.openxmlformats.org/package/2006/relationships",
            )],
        );
        for (id, kind, target) in &self.entries {
            w.empty(
                "Relationship",
                &[("Id", id), ("Type", kind), ("Target", target)],
            );
        }
        w.finish()
    }
}

#[derive(Default)]
pub struct Package {
    zip: ZipWriter,
    error: Option<ZipError>,
    overrides: Vec<(String, String)>,
    defaults: Vec<(String, String)>,
}

impl Package {
    #[must_use]
    pub fn new() -> Self {
        let mut package = Self::default();
        package.default_type("rels", types::RELS);
        package.default_type("xml", types::XML);
        package
    }

    fn add(&mut self, name: &str, bytes: &[u8]) {
        if self.error.is_some() {
            return;
        }
        let method = if name.ends_with(".png") || name.ends_with(".jpeg") || name.ends_with(".jpg")
        {
            Method::Stored
        } else {
            Method::Deflated
        };
        if let Err(error) = self.zip.add(name, bytes, method) {
            self.error = Some(error);
        }
    }

    pub fn default_type(&mut self, extension: &str, content_type: &str) {
        if !self.defaults.iter().any(|(e, _)| e == extension) {
            self.defaults
                .push((extension.to_owned(), content_type.to_owned()));
        }
    }

    pub fn part(&mut self, name: &str, content_type: &str, bytes: Vec<u8>) {
        self.overrides
            .push((format!("/{name}"), content_type.to_owned()));
        self.add(name, &bytes);
    }

    pub fn file(&mut self, name: &str, bytes: Vec<u8>) {
        self.add(name, &bytes);
    }

    pub fn properties_and_root(&mut self, main: &str, title: &str, application: &str) {
        let mut core = Writer::new();
        core.open(
            "cp:coreProperties",
            &[
                (
                    "xmlns:cp",
                    "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
                ),
                ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
                ("xmlns:dcterms", "http://purl.org/dc/terms/"),
                ("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"),
            ],
        );
        if !title.is_empty() {
            core.leaf("dc:title", &[], title);
        }
        core.leaf("dc:creator", &[], "PanPDF");
        let mut app = Writer::new();
        app.open(
            "Properties",
            &[(
                "xmlns",
                "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
            )],
        );
        app.leaf("Application", &[], application);
        self.part("docProps/core.xml", types::CORE, core.finish().into_bytes());
        self.part("docProps/app.xml", types::APP, app.finish().into_bytes());
        let mut rels = Rels::new();
        rels.add(rel::OFFICE_DOCUMENT, main);
        rels.add(rel::CORE, "docProps/core.xml");
        rels.add(rel::APP, "docProps/app.xml");
        self.file("_rels/.rels", rels.xml().into_bytes());
    }

    fn content_types(&self) -> String {
        let mut w = Writer::new();
        w.open(
            "Types",
            &[(
                "xmlns",
                "http://schemas.openxmlformats.org/package/2006/content-types",
            )],
        );
        for (extension, content_type) in &self.defaults {
            w.empty(
                "Default",
                &[("Extension", extension), ("ContentType", content_type)],
            );
        }
        for (name, content_type) in &self.overrides {
            w.empty(
                "Override",
                &[("PartName", name), ("ContentType", content_type)],
            );
        }
        w.finish()
    }

    pub fn finish(mut self) -> Result<Vec<u8>, ZipError> {
        let types = self.content_types();
        self.add("[Content_Types].xml", types.as_bytes());
        if let Some(error) = self.error {
            return Err(error);
        }
        self.zip.finish()
    }
}

pub const LAO_FACE: &str = "Phetsarath OT";
pub const THAI_FACE: &str = "Leelawadee UI";
pub const LATIN_FACE: &str = "Liberation Serif";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Faces {
    pub latin: String,
    pub east_asian: String,
    pub complex: String,
}

#[must_use]
pub fn faces(style: &Style, script: Script) -> Faces {
    let family = style.family.trim();
    let named = !family.is_empty();
    let complex_default = match script {
        Script::Thai => THAI_FACE,
        _ => LAO_FACE,
    };
    let complex = if style.legacy || !named {
        complex_default.to_owned()
    } else {
        family.to_owned()
    };
    let latin = if named && !(style.legacy && script.is_complex()) {
        family.to_owned()
    } else {
        LATIN_FACE.to_owned()
    };
    Faces {
        east_asian: latin.clone(),
        latin,
        complex,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units() {
        assert_eq!(twips(72.0), 1440);
        assert_eq!(half_points(10.5), 21);
        assert_eq!(emu(1.0), 12_700);
        assert_eq!(hex([255, 0, 16]), "FF0010");
    }

    #[test]
    fn legacy_lao_gets_a_unicode_face() {
        let style = Style {
            family: "LAOFONTSU".into(),
            legacy: true,
            ..Style::default()
        };
        assert_eq!(faces(&style, Script::Lao).complex, LAO_FACE);
        let honest = Style {
            family: "Saysettha OT".into(),
            ..Style::default()
        };
        let faces = faces(&honest, Script::Lao);
        assert_eq!(faces.complex, "Saysettha OT");
        assert_eq!(faces.latin, "Saysettha OT");
    }

    #[test]
    fn content_types_name_every_part() {
        let mut package = Package::new();
        package.part("word/document.xml", "x/y", b"<a/>".to_vec());
        let types = package.content_types();
        assert!(types.contains("PartName=\"/word/document.xml\" ContentType=\"x/y\""));
        assert!(types.contains("Extension=\"rels\""));
        let bytes = package.finish().unwrap();
        assert_eq!(&bytes[30..47], b"word/document.xml");
    }
}
