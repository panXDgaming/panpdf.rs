use std::collections::HashSet;

pub fn base_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

pub fn stem(name: &str) -> String {
    let base = base_name(name);
    match base.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ if base.is_empty() => "output".to_owned(),
        _ => base.to_owned(),
    }
}

#[derive(Default)]
pub struct Names {
    taken: HashSet<String>,
}

impl Names {
    pub fn free(&self, wanted: &str) -> String {
        if !self.taken.contains(&wanted.to_lowercase()) {
            return wanted.to_owned();
        }
        let (folder, file) = wanted.rsplit_once('/').unwrap_or(("", wanted));
        let (stem, extension) = match file.rsplit_once('.') {
            Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
            _ => (file, String::new()),
        };
        let prefix = if folder.is_empty() {
            String::new()
        } else {
            format!("{folder}/")
        };
        (2_u32..u32::MAX)
            .map(|n| format!("{prefix}{stem}-{n}{extension}"))
            .find(|candidate| !self.taken.contains(&candidate.to_lowercase()))
            .unwrap_or_else(|| wanted.to_owned())
    }

    pub fn keep(&mut self, name: &str) {
        self.taken.insert(name.to_lowercase());
    }

    pub fn claim(&mut self, wanted: &str) -> String {
        let name = self.free(wanted);
        self.keep(&name);
        name
    }
}

pub fn made_unique(files: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    let mut names = Names::default();
    files
        .into_iter()
        .map(|(name, bytes)| (names.claim(&name), bytes))
        .collect()
}
