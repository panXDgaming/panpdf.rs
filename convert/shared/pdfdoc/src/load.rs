use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use pdf_bytes::{ByteStore, SourceId};
use pdf_security::AuthenticatedSecurity;
use pdf_syntax::{
    DecryptionRefused, Object as SyntaxObject, ObjectKind, ObjectParser, ParseLimits, Reference,
    ResolveLimits, RevisionChain, RevisionIndex, StreamDecryptor, XrefEntryKind, XrefLimits,
    decode_name, decode_string,
};

use crate::value::{Dict, Value};
use crate::{Doc, Object};

#[derive(Clone, Debug, Default)]
pub struct Load {
    pub password: Vec<u8>,
    pub recover: bool,
    pub everything: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    NotPdf(String),
    Damaged(String),
    Password,
    Encryption(String),
    NoCatalog,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPdf(why) => write!(f, "the file cannot be read as a PDF: {why}"),
            Self::Damaged(why) => write!(
                f,
                "the file is damaged ({why}); repair it first (repair-pdf)"
            ),
            Self::Password => f.write_str("the password is wrong, or the file needs one"),
            Self::Encryption(why) => write!(f, "the file's protection cannot be read: {why}"),
            Self::NoCatalog => f.write_str("no document catalogue was found in the file"),
        }
    }
}

impl std::error::Error for LoadError {}

const MOST_STRING_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
struct ObjectStreams(Arc<AuthenticatedSecurity>);

impl StreamDecryptor for ObjectStreams {
    fn decrypt_stream(
        &self,
        reference: Reference,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, DecryptionRefused> {
        self.0
            .decrypt_stream(reference, encrypted)
            .map_err(|_| DecryptionRefused::new("the security handler produced no plaintext"))
    }
}

pub fn load(bytes: Vec<u8>, how: &Load) -> Result<Doc, LoadError> {
    let source = ByteStore::owning(SourceId::next_document(), bytes);
    let limits = pdf_syntax::RecoverLimits::default();
    let (header, mut repairs) = pdf_syntax::parse_header_recovering(&source, limits)
        .map_err(|error| LoadError::NotPdf(error.to_string()))?
        .into_parts();
    let version = header.version().to_string();
    let mut notes: Vec<String> = repairs.drain(..).map(|r| r.to_string()).collect();

    let mut newer: Vec<(u32, u32)> = Vec::new();
    let chain = pdf_syntax::parse_revision_chain_strict(&source, XrefLimits::default());
    let (index, trailer, chain) = match chain {
        Ok(chain) => match RevisionIndex::from_chain(&chain) {
            Ok(index) => {
                let trailer = chain
                    .revisions()
                    .first()
                    .map(|section| section.trailer().clone())
                    .ok_or_else(|| LoadError::Damaged("no revision".into()))?;
                let trailer = to_value(&source, &trailer, None).as_dict().cloned();
                (index, trailer, Some(chain))
            }
            Err(error) => {
                if !how.recover {
                    return Err(LoadError::Damaged(error.to_string()));
                }
                notes.push(format!("object index rebuilt by scanning: {error}"));
                let (index, trailer) = scanned(&source, &mut notes, &mut newer)?;
                (index, trailer, None)
            }
        },
        Err(error) => {
            if !how.recover {
                return Err(LoadError::Damaged(error.to_string()));
            }
            notes.push(format!("cross-reference rebuilt by scanning: {error}"));
            let (index, trailer) = scanned(&source, &mut notes, &mut newer)?;
            (index, trailer, None)
        }
    };
    let mut index = index.tolerating_damage().resolving_undefined_as_null();
    let mut trailer = trailer.unwrap_or_default();

    let mut security: Option<Arc<AuthenticatedSecurity>> = None;
    let mut encrypt_number = None;
    if trailer.has("Encrypt") {
        let Some(chain) = chain.as_ref() else {
            return Err(LoadError::Encryption(
                "a protected file whose cross-reference is damaged cannot be opened".into(),
            ));
        };
        encrypt_number = trailer.get("Encrypt").and_then(Value::as_ref);
        let handler = authenticate(&source, chain, &index, &how.password)?;
        let handler = Arc::new(handler);
        index = index.with_stream_decryptor(Arc::new(ObjectStreams(Arc::clone(&handler))));
        security = Some(handler);
    }

    let mut doc = Doc {
        objects: BTreeMap::new(),
        trailer: Dict::new(),
        version,
        notes,
        was_encrypted: security.is_some(),
        recovered: chain.is_none(),
        kept: None,
    };
    if let (Some(handler), Some(number)) = (security.as_ref(), encrypt_number) {
        let plain = Reader {
            source: &source,
            index: &index,
            security: None,
            overrides: std::collections::HashMap::new(),
        };
        let mut notes = Vec::new();
        let dictionary = plain.object(number, &mut notes).value;
        doc.kept = Some(crate::Kept {
            security: Arc::clone(handler),
            dictionary: dictionary.to_bytes(),
        });
    }
    let mut reader = Reader {
        source: &source,
        index: &index,
        security: security.as_deref(),
        overrides: std::collections::HashMap::new(),
    };
    for (object, stream) in newer {
        if let Some(value) = reader.in_object_stream(stream, object) {
            doc.notes.push(format!(
                "object {object}: the later definition in object stream {stream} was taken"
            ));
            reader.overrides.insert(object, value);
        }
    }

    let mut root = trailer.get("Root").and_then(Value::as_ref);
    if let Some(number) = root
        && !reader.is_catalog(number)
    {
        doc.notes
            .push(format!("the trailer's /Root {number} is not a catalogue"));
        root = None;
    }
    if root.is_none() {
        root = reader.find_catalog();
        if let Some(number) = root {
            doc.notes.push(format!(
                "the catalogue was found by scanning: object {number}"
            ));
        }
    }
    let mut start: Vec<u32> = Vec::new();
    if let Some(root) = root {
        start.push(root);
        doc.trailer.set("Root", Value::Ref(root));
    }
    if let Some(info) = trailer.get("Info").and_then(Value::as_ref) {
        start.push(info);
        doc.trailer.set("Info", Value::Ref(info));
    }
    if let Some(id) = trailer.remove("ID") {
        doc.trailer.set("ID", id);
    }
    if how.everything {
        let mut all: Vec<u32> = index
            .selected_entries()
            .filter(|s| !matches!(s.entry().kind(), XrefEntryKind::Free { .. }))
            .map(|s| s.entry().object_number())
            .collect();
        all.sort_unstable();
        start.extend(all);
    }

    let mut queue: VecDeque<u32> = start.into_iter().collect();
    while let Some(number) = queue.pop_front() {
        if doc.objects.contains_key(&number) || Some(number) == encrypt_number {
            continue;
        }
        let object = reader.object(number, &mut doc.notes);
        let mut found = Vec::new();
        object.value.references(&mut found);
        doc.objects.insert(number, object);
        queue.extend(found.into_iter().filter(|n| !doc.objects.contains_key(n)));
    }
    if how.everything {
        doc.objects.retain(|_, object| {
            let dict = object.value.as_dict();
            !dict.is_some_and(|d| d.is("Type", "XRef") || d.is("Type", "ObjStm"))
        });
    }
    if root.is_none() && !how.everything {
        return Err(LoadError::NoCatalog);
    }
    Ok(doc)
}

fn authenticate(
    source: &ByteStore,
    chain: &RevisionChain,
    index: &RevisionIndex,
    password: &[u8],
) -> Result<AuthenticatedSecurity, LoadError> {
    pdf_security::authenticate_standard_password(
        source,
        chain,
        index,
        password,
        ResolveLimits::default(),
    )
    .map_err(|error| match error.kind() {
        pdf_security::SecurityErrorKind::InvalidPassword => LoadError::Password,
        _ => LoadError::Encryption(error.to_string()),
    })
}

fn scanned(
    source: &ByteStore,
    notes: &mut Vec<String>,
    newer: &mut Vec<(u32, u32)>,
) -> Result<(RevisionIndex, Option<Dict>), LoadError> {
    let (objects, repairs) =
        pdf_syntax::scan_indirect_objects(source, pdf_syntax::RecoverLimits::default())
            .map_err(|error| LoadError::NotPdf(error.to_string()))?
            .into_parts();
    notes.extend(repairs.iter().take(50).map(ToString::to_string));
    if repairs.len() > 50 {
        notes.push(format!("... and {} more repairs", repairs.len() - 50));
    }
    for repair in &repairs {
        if let pdf_syntax::RepairKind::DirectDefinitionShadowsObjectStream {
            object_number,
            object_stream_number,
        } = repair.kind()
            && let Some(entry) = objects.entry_for_number(object_number)
            && let pdf_syntax::ScannedLocation::Direct { byte_offset } = entry.location()
            && repair.byte_offset() > byte_offset
        {
            newer.push((object_number, object_stream_number));
        }
    }
    let index = RevisionIndex::from_scanned(&objects);
    Ok((index, last_trailer(source)))
}

fn last_trailer(source: &ByteStore) -> Option<Dict> {
    let bytes = source.as_bytes();
    let keyword = b"trailer";
    let mut end = bytes.len();
    while let Some(at) = bytes[..end]
        .windows(keyword.len())
        .rposition(|window| window == keyword)
    {
        let mut parser = ObjectParser::new(source, at + keyword.len(), ParseLimits::default());
        if let Ok(Some(object)) = parser.parse_next()
            && let Value::Dict(dict) = to_value(source, &object, None)
        {
            return Some(dict);
        }
        end = at;
    }
    None
}

struct Reader<'a> {
    source: &'a ByteStore,
    index: &'a RevisionIndex,
    security: Option<&'a AuthenticatedSecurity>,
    overrides: std::collections::HashMap<u32, Value>,
}

impl Reader<'_> {
    fn reference(&self, number: u32) -> Reference {
        let generation = self
            .index
            .selected_for_number(number)
            .map_or(0, |selected| selected.entry().generation());
        Reference::new(number, generation)
    }

    fn in_object_stream(&self, stream: u32, object: u32) -> Option<Value> {
        let resolved = self
            .index
            .resolve_object(
                self.source,
                self.reference(stream),
                ResolveLimits::default(),
            )
            .ok()?;
        let data = resolved.stream()?;
        let ObjectKind::Dictionary(entries) = resolved.value().kind() else {
            return None;
        };
        let span = data.data_span();
        let encoded = resolved.source().resolve(span).ok()?;
        let decoded = pdf_syntax::decode_stream_bytes(
            resolved.source(),
            entries,
            encoded,
            span.start(),
            256 * 1024 * 1024,
        )
        .ok()?;
        let dict = to_value(resolved.source(), resolved.value(), None);
        let dict = dict.as_dict()?;
        let count = dict.get("N").and_then(Value::as_number)? as usize;
        let first = dict.get("First").and_then(Value::as_number)? as usize;
        let store = ByteStore::owning(SourceId::next_document(), decoded);
        let mut parser = ObjectParser::new(&store, 0, ParseLimits::default());
        let mut offset = None;
        for _ in 0..count.min(1_000_000) {
            let number = parser.parse_next().ok()??;
            let at = parser.parse_next().ok()??;
            let number = to_value(&store, &number, None).as_number()? as u32;
            let at = to_value(&store, &at, None).as_number()? as usize;
            if number == object {
                offset = Some(at);
                break;
            }
        }
        let mut parser = ObjectParser::new(&store, first + offset?, ParseLimits::default());
        let value = parser.parse_next().ok()??;
        Some(to_value(&store, &value, None))
    }

    fn object(&self, number: u32, notes: &mut Vec<String>) -> Object {
        if let Some(value) = self.overrides.get(&number) {
            return Object {
                value: value.clone(),
                stream: None,
            };
        }
        let reference = self.reference(number);
        let resolved =
            match self
                .index
                .resolve_object(self.source, reference, ResolveLimits::default())
            {
                Ok(resolved) => resolved,
                Err(error) => {
                    notes.push(format!("object {number} could not be read: {error}"));
                    return Object {
                        value: Value::Null,
                        stream: None,
                    };
                }
            };
        for repair in resolved.repairs() {
            notes.push(format!("object {number}: {repair}"));
        }
        let strings = if resolved.is_compressed() {
            None
        } else {
            self.security.map(|security| (security, reference))
        };
        let mut value = to_value(resolved.source(), resolved.value(), strings);
        let stream = resolved.stream().and_then(|stream| {
            let data = resolved.source().resolve(stream.data_span()).ok()?.to_vec();
            let dict = value.as_dict();
            let exempt = dict.is_some_and(|d| {
                d.is("Type", "XRef")
                    || (d.is("Type", "Metadata")
                        && self.security.is_some_and(|s| !s.encrypt_metadata()))
            });
            match self.security {
                Some(security) if !exempt => match security.decrypt_stream(reference, &data) {
                    Ok(plain) => Some(plain),
                    Err(error) => {
                        notes.push(format!("stream {number} could not be decrypted: {error}"));
                        Some(Vec::new())
                    }
                },
                _ => Some(data),
            }
        });
        if stream.is_some()
            && let Some(dict) = value.as_dict_mut()
        {
            dict.remove("Length");
        }
        Object { value, stream }
    }

    fn dict_of(&self, number: u32) -> Option<Dict> {
        let resolved = self
            .index
            .resolve_object(
                self.source,
                self.reference(number),
                ResolveLimits::default(),
            )
            .ok()?;
        to_value(resolved.source(), resolved.value(), None)
            .as_dict()
            .cloned()
    }

    fn is_catalog(&self, number: u32) -> bool {
        self.dict_of(number)
            .is_some_and(|d| d.is("Type", "Catalog") || d.get("Pages").is_some())
    }

    fn find_catalog(&self) -> Option<u32> {
        let mut numbers: Vec<u32> = self
            .index
            .selected_entries()
            .filter(|s| !matches!(s.entry().kind(), XrefEntryKind::Free { .. }))
            .map(|s| s.entry().object_number())
            .collect();
        numbers.sort_unstable();
        numbers.into_iter().rev().find(|&number| {
            self.dict_of(number)
                .is_some_and(|d| d.is("Type", "Catalog") && d.get("Pages").is_some())
        })
    }
}

fn to_value(
    source: &ByteStore,
    object: &SyntaxObject,
    strings: Option<(&AuthenticatedSecurity, Reference)>,
) -> Value {
    match object.kind() {
        ObjectKind::Null => Value::Null,
        ObjectKind::Boolean(b) => Value::Bool(*b),
        ObjectKind::Number(_) => {
            let text = source
                .resolve(object.span())
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default();
            number(&text)
        }
        ObjectKind::Name => {
            let name = decode_name(source, object).unwrap_or_default();
            Value::Name(name.get(1..).unwrap_or_default().to_vec())
        }
        ObjectKind::LiteralString | ObjectKind::HexString => {
            let bytes = decode_string(source, object, MOST_STRING_BYTES).unwrap_or_default();
            match strings {
                Some((security, reference)) => {
                    Value::Str(security.decrypt_string(reference, &bytes).unwrap_or(bytes))
                }
                None => Value::Str(bytes),
            }
        }
        ObjectKind::Reference(reference) => Value::Ref(reference.object_number()),
        ObjectKind::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| to_value(source, item, strings))
                .collect(),
        ),
        ObjectKind::Dictionary(entries) => {
            let mut dict = Dict::new();
            for entry in entries {
                let Ok(key) = entry.decoded_key(source) else {
                    continue;
                };
                let key = key.get(1..).unwrap_or_default().to_vec();
                let value = to_value(source, entry.value(), strings);
                dict.0.push((key, value));
            }
            Value::Dict(dict)
        }
    }
}

fn number(text: &str) -> Value {
    let text = text.trim();
    if !text.contains('.')
        && let Ok(n) = text.parse::<i64>()
    {
        return Value::Int(n);
    }
    text.parse::<f64>().map_or(Value::Int(0), Value::Real)
}
