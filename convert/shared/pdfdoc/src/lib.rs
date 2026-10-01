#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

pub mod load;
pub mod shown;
pub mod value;
pub mod write;

pub use load::{Load, LoadError, load};
pub use value::{Dict, Value, read_text_string, real, text_string};
pub use write::{Protection, Written, write};

pub use pdf_security::seed_random;

pub fn seed_if_given(random: &[u8]) -> Result<(), String> {
    if random.is_empty() {
        return Ok(());
    }
    seed_random(random).map_err(|e| format!("the random bytes handed over were refused ({e})"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub value: Value,
    pub stream: Option<Vec<u8>>,
}

impl Object {
    #[must_use]
    pub const fn new(value: Value) -> Self {
        Self {
            value,
            stream: None,
        }
    }

    #[must_use]
    pub const fn stream(dict: Dict, data: Vec<u8>) -> Self {
        Self {
            value: Value::Dict(dict),
            stream: Some(data),
        }
    }

    #[must_use]
    pub const fn dict(&self) -> Option<&Dict> {
        self.value.as_dict()
    }

    pub const fn dict_mut(&mut self) -> Option<&mut Dict> {
        self.value.as_dict_mut()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub objects: BTreeMap<u32, Object>,
    pub trailer: Dict,
    pub version: String,
    pub notes: Vec<String>,
    pub was_encrypted: bool,
    pub recovered: bool,
    pub kept: Option<Kept>,
}

#[derive(Clone)]
pub struct Kept {
    pub security: std::sync::Arc<pdf_security::AuthenticatedSecurity>,
    pub dictionary: Vec<u8>,
}

impl Kept {
    #[must_use]
    pub fn restricted(&self) -> bool {
        !self.security.may_modify_content()
    }
}

impl std::fmt::Debug for Kept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Kept(..)")
    }
}

pub fn plain(bytes: Vec<u8>, password: &[u8]) -> Result<(Vec<u8>, Option<Kept>), String> {
    if !bytes.windows(8).any(|w| w == b"/Encrypt") {
        return Ok((bytes, None));
    }
    let compress = uses_object_streams(&bytes);
    let doc = load(
        bytes.clone(),
        &Load {
            password: password.to_vec(),
            recover: true,
            everything: false,
        },
    )
    .map_err(|e| e.to_string())?;
    let Some(kept) = doc.kept.clone() else {
        return Ok((bytes, None));
    };
    Ok((write(&doc, None, compress)?.bytes, Some(kept)))
}

pub fn relock(bytes: Vec<u8>, kept: &Kept) -> Result<Vec<u8>, String> {
    let compress = uses_object_streams(&bytes);
    let doc = load(bytes, &Load::default()).map_err(|e| e.to_string())?;
    let written = write(
        &doc,
        Some(&Protection {
            dictionary: &kept.dictionary,
            security: &kept.security,
        }),
        compress,
    )?;
    Ok(written.bytes)
}

#[derive(Clone, Debug)]
pub struct PageRef {
    pub number: u32,
    pub media_box: [f64; 4],
    pub crop_box: [f64; 4],
    pub rotate: i64,
}

impl Doc {
    #[must_use]
    pub fn root(&self) -> Option<u32> {
        self.trailer.get("Root").and_then(Value::as_ref)
    }

    #[must_use]
    pub fn get(&self, number: u32) -> Option<&Object> {
        self.objects.get(&number)
    }

    pub fn get_mut(&mut self, number: u32) -> Option<&mut Object> {
        self.objects.get_mut(&number)
    }

    #[must_use]
    pub fn resolve<'a>(&'a self, value: &'a Value) -> &'a Value {
        let mut value = value;
        for _ in 0..16 {
            match value {
                Value::Ref(n) => match self.objects.get(n) {
                    Some(object) => value = &object.value,
                    None => return &Value::Null,
                },
                _ => return value,
            }
        }
        &Value::Null
    }

    #[must_use]
    pub fn dict_at<'a>(&'a self, dict: &'a Dict, key: &str) -> Option<&'a Dict> {
        dict.get(key)
            .map(|v| self.resolve(v))
            .and_then(Value::as_dict)
    }

    #[must_use]
    pub fn catalog(&self) -> Option<&Dict> {
        self.root().and_then(|n| self.get(n)).and_then(Object::dict)
    }

    pub fn catalog_mut(&mut self) -> Option<&mut Dict> {
        let root = self.root()?;
        self.get_mut(root).and_then(Object::dict_mut)
    }

    #[must_use]
    pub fn next_number(&self) -> u32 {
        self.objects.keys().next_back().map_or(1, |n| n + 1)
    }

    pub fn add(&mut self, object: Object) -> u32 {
        let number = self.next_number();
        self.objects.insert(number, object);
        number
    }

    #[must_use]
    pub fn pages(&self) -> Vec<PageRef> {
        let mut out = Vec::new();
        let Some(pages) = self
            .catalog()
            .and_then(|c| c.get("Pages"))
            .and_then(Value::as_ref)
        else {
            return out;
        };
        let mut seen = BTreeSet::new();
        self.walk(pages, &Inherited::default(), &mut seen, &mut out, 0);
        out
    }

    fn walk(
        &self,
        number: u32,
        inherited: &Inherited,
        seen: &mut BTreeSet<u32>,
        out: &mut Vec<PageRef>,
        depth: usize,
    ) {
        if depth > 64 || !seen.insert(number) {
            return;
        }
        let Some(dict) = self.get(number).and_then(Object::dict) else {
            return;
        };
        let mut here = inherited.clone();
        if let Some(b) = self.rect(dict, "MediaBox") {
            here.media_box = Some(b);
        }
        if let Some(b) = self.rect(dict, "CropBox") {
            here.crop_box = Some(b);
        }
        if let Some(Value::Int(r)) = dict.get("Rotate").map(|v| self.resolve(v)) {
            here.rotate = *r;
        }
        let kids = dict.get("Kids").map(|v| self.resolve(v));
        if dict.is("Type", "Page") || (kids.is_none() && dict.has("Contents")) {
            let media = here.media_box.unwrap_or([0.0, 0.0, 612.0, 792.0]);
            out.push(PageRef {
                number,
                media_box: media,
                crop_box: here.crop_box.unwrap_or(media),
                rotate: here.rotate.rem_euclid(360),
            });
            return;
        }
        if let Some(kids) = kids.and_then(Value::as_array) {
            for kid in kids {
                if let Some(kid) = kid.as_ref() {
                    self.walk(kid, &here, seen, out, depth + 1);
                }
            }
        }
    }

    #[must_use]
    pub fn rect(&self, dict: &Dict, key: &str) -> Option<[f64; 4]> {
        let items = self.resolve(dict.get(key)?).as_array()?;
        if items.len() != 4 {
            return None;
        }
        let mut n = [0.0; 4];
        for (slot, item) in n.iter_mut().zip(items) {
            *slot = self.resolve(item).as_number()?;
        }
        Some([
            n[0].min(n[2]),
            n[1].min(n[3]),
            n[0].max(n[2]),
            n[1].max(n[3]),
        ])
    }

    #[must_use]
    pub fn reachable(&self) -> Vec<u32> {
        let mut order = Vec::new();
        let mut seen = BTreeSet::new();
        let mut queue: VecDeque<u32> = VecDeque::new();
        let mut start = Vec::new();
        Value::Dict(self.trailer.clone()).references(&mut start);
        queue.extend(start);
        while let Some(number) = queue.pop_front() {
            if !seen.insert(number) {
                continue;
            }
            let Some(object) = self.objects.get(&number) else {
                continue;
            };
            order.push(number);
            let mut found = Vec::new();
            object.value.references(&mut found);
            queue.extend(found.into_iter().filter(|n| !seen.contains(n)));
        }
        order
    }

    pub fn dedupe(&mut self) -> usize {
        let root = self.root();
        let info = self.trailer.get("Info").and_then(Value::as_ref);
        let mut merged = 0;
        for _ in 0..8 {
            let mut first: HashMap<Vec<u8>, u32> = HashMap::new();
            let mut to: BTreeMap<u32, u32> = BTreeMap::new();
            for (&number, object) in &self.objects {
                if Some(number) == root || Some(number) == info || !may_merge(object) {
                    continue;
                }
                let mut key = vec![if object.stream.is_some() { b's' } else { b'o' }];
                object.value.write(&mut key);
                if let Some(data) = &object.stream {
                    key.push(0);
                    key.extend_from_slice(data);
                }
                match first.get(&key) {
                    Some(&keep) => {
                        to.insert(number, keep);
                    }
                    None => {
                        first.insert(key, number);
                    }
                }
            }
            if to.is_empty() {
                break;
            }
            merged += to.len();
            for number in to.keys() {
                self.objects.remove(number);
            }
            let map = |n: u32| Some(to.get(&n).copied().unwrap_or(n));
            for object in self.objects.values_mut() {
                object.value.renumber(&map);
            }
        }
        merged
    }

    pub fn can_protect(&self) -> Result<(), String> {
        let Some(kept) = &self.kept else {
            return Ok(());
        };
        let probe = pdf_syntax::Reference::new(1, 0);
        let stream = kept.security.encrypt_stream(probe, b"probe");
        let string = kept.security.encrypt_string(probe, b"probe");
        match stream.and(string) {
            Ok(_) => Ok(()),
            Err(why) => Err(format!(
                "this file is protected with AES and cannot be protected again here ({why}): \
                 this place gives no secure random numbers (a page hands them over with \
                 random(64) from crypto.getRandomValues), and the file is never written \
                 unprotected; use the desktop tool, or remove the protection first"
            )),
        }
    }

    #[must_use]
    pub fn decoded(&self, object: &Object) -> Option<Vec<u8>> {
        let data = object.stream.as_ref()?;
        let dict = object.dict()?;
        let filters: Vec<Vec<u8>> = match dict.get("Filter").map(|v| self.resolve(v)) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Name(name)) => vec![name.clone()],
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| match self.resolve(item) {
                    Value::Name(name) => Some(name.clone()),
                    _ => None,
                })
                .collect(),
            Some(_) => return None,
        };
        let params = dict.get("DecodeParms").map(|v| self.resolve(v));
        let predicted = match params {
            Some(Value::Dict(d)) => {
                d.get("Predictor").and_then(Value::as_number).unwrap_or(1.0) > 1.0
            }
            Some(Value::Array(items)) => items.iter().any(|item| {
                self.resolve(item)
                    .as_dict()
                    .and_then(|d| d.get("Predictor"))
                    .and_then(Value::as_number)
                    .unwrap_or(1.0)
                    > 1.0
            }),
            _ => false,
        };
        if predicted {
            return None;
        }
        let mut bytes = data.clone();
        for filter in filters {
            match filter.as_slice() {
                b"FlateDecode" | b"Fl" => {
                    bytes = pdf_syntax::inflate_zlib(&bytes, 512 * 1024 * 1024).ok()?;
                }
                _ => return None,
            }
        }
        Some(bytes)
    }
}

fn may_merge(object: &Object) -> bool {
    if object.stream.is_some() {
        return true;
    }
    let Some(dict) = object.dict() else {
        return true;
    };
    if dict.has("Parent") || dict.has("Kids") {
        return false;
    }
    !matches!(
        dict.name("Type"),
        Some(b"Page" | b"Pages" | b"Annot" | b"Catalog" | b"Sig" | b"StructElem")
    )
}

#[derive(Clone, Default)]
struct Inherited {
    media_box: Option<[f64; 4]>,
    crop_box: Option<[f64; 4]>,
    rotate: i64,
}

#[must_use]
pub fn deflate(data: &[u8]) -> Vec<u8> {
    pdf_syntax::deflate_zlib(data)
}

#[cfg(test)]
mod tests;

#[must_use]
pub fn uses_object_streams(bytes: &[u8]) -> bool {
    bytes.windows(7).any(|w| w == b"/ObjStm")
}

#[must_use]
pub fn is_signed(doc: &Doc) -> bool {
    doc.objects.values().any(|object| {
        object
            .dict()
            .is_some_and(|d| d.has("ByteRange") && d.has("Contents"))
    })
}

pub fn check_written(bytes: &[u8], password: &[u8], pages: usize) -> Result<(), String> {
    let doc = load(
        bytes.to_vec(),
        &Load {
            password: password.to_vec(),
            ..Load::default()
        },
    )
    .map_err(|e| format!("the file written does not read back: {e}"))?;
    let found = doc.pages().len();
    if found != pages {
        return Err(format!(
            "the file written reads back with {found} pages instead of {pages}"
        ));
    }
    Ok(())
}
