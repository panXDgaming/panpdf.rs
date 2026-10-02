use std::path::{Path, PathBuf};

use eframe::egui;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Host {
    Linux,
    Windows,
    MacOs,
}

impl Host {
    pub(crate) const fn this() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

struct Script {
    name: &'static str,
    packaged: Option<&'static str>,
    linux: &'static [&'static str],
    windows: &'static [&'static str],
    macos: &'static [&'static str],
}

const SCRIPTS: [Script; 23] = [
    Script {
        name: "thai",
        packaged: Some("NotoSansThai-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf"],
        windows: &["LeelawUI.ttf", "leelawad.ttf", "tahoma.ttf"],
        macos: &[
            "/System/Library/Fonts/Thonburi.ttc",
            "/System/Library/Fonts/Supplemental/Thonburi.ttc",
        ],
    },
    Script {
        name: "lao",
        packaged: Some("NotoSansLao-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansLao-Regular.ttf"],
        windows: &["LeelawUI.ttf", "LaoUI.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Lao Sangam MN.ttf"],
    },
    Script {
        name: "devanagari",
        packaged: Some("NotoSansDevanagari-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "mangal.ttf"],
        macos: &[
            "/System/Library/Fonts/Kohinoor.ttc",
            "/System/Library/Fonts/Supplemental/Devanagari Sangam MN.ttc",
        ],
    },
    Script {
        name: "bengali",
        packaged: Some("NotoSansBengali-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansBengali-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "vrinda.ttf"],
        macos: &[
            "/System/Library/Fonts/KohinoorBangla.ttc",
            "/System/Library/Fonts/Supplemental/Bangla Sangam MN.ttc",
        ],
    },
    Script {
        name: "tamil",
        packaged: Some("NotoSansTamil-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansTamil-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "latha.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Tamil Sangam MN.ttc"],
    },
    Script {
        name: "sinhala",
        packaged: Some("NotoSansSinhala-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansSinhala-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "iskpota.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Sinhala Sangam MN.ttc"],
    },
    Script {
        name: "khmer",
        packaged: Some("NotoSansKhmer-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansKhmer-Regular.ttf"],
        windows: &["KhmerUI.ttf", "daunpenh.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Khmer Sangam MN.ttf"],
    },
    Script {
        name: "myanmar",
        packaged: Some("NotoSansMyanmar-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansMyanmar-Regular.ttf"],
        windows: &["mmrtext.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Myanmar Sangam MN.ttc"],
    },
    Script {
        name: "arabic",
        packaged: Some("NotoSansArabic-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf"],
        windows: &["segoeui.ttf", "arial.ttf"],
        macos: &[
            "/System/Library/Fonts/GeezaPro.ttc",
            "/System/Library/Fonts/SFArabic.ttf",
        ],
    },
    Script {
        name: "hebrew",
        packaged: Some("NotoSansHebrew-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf"],
        windows: &["segoeui.ttf", "arial.ttf"],
        macos: &[
            "/System/Library/Fonts/ArialHB.ttc",
            "/System/Library/Fonts/SFHebrew.ttf",
        ],
    },
    Script {
        name: "georgian",
        packaged: Some("NotoSansGeorgian-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansGeorgian-Regular.ttf"],
        windows: &["segoeui.ttf", "sylfaen.ttf"],
        macos: &[
            "/System/Library/Fonts/SFGeorgian.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ],
    },
    Script {
        name: "armenian",
        packaged: Some("NotoSansArmenian-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansArmenian-Regular.ttf"],
        windows: &["segoeui.ttf", "sylfaen.ttf"],
        macos: &[
            "/System/Library/Fonts/SFArmenian.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ],
    },
    Script {
        name: "ethiopic",
        packaged: Some("NotoSansEthiopic-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansEthiopic-Regular.ttf"],
        windows: &["ebrima.ttf", "nyala.ttf"],
        macos: &[
            "/System/Library/Fonts/Kefa.ttc",
            "/System/Library/Fonts/Supplemental/Kefa.ttc",
        ],
    },
    Script {
        name: "gurmukhi",
        packaged: Some("NotoSansGurmukhi-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansGurmukhi-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "raavi.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Gurmukhi Sangam MN.ttc"],
    },
    Script {
        name: "gujarati",
        packaged: Some("NotoSansGujarati-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansGujarati-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "shruti.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Gujarati Sangam MN.ttc"],
    },
    Script {
        name: "oriya",
        packaged: Some("NotoSansOriya-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansOriya-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "kalinga.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Oriya Sangam MN.ttc"],
    },
    Script {
        name: "telugu",
        packaged: Some("NotoSansTelugu-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansTelugu-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "gautami.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Telugu Sangam MN.ttc"],
    },
    Script {
        name: "kannada",
        packaged: Some("NotoSansKannada-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansKannada-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "tunga.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Kannada Sangam MN.ttc"],
    },
    Script {
        name: "malayalam",
        packaged: Some("NotoSansMalayalam-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansMalayalam-Regular.ttf"],
        windows: &["Nirmala.ttf", "Nirmala.ttc", "kartika.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Malayalam Sangam MN.ttc"],
    },
    Script {
        name: "tibetan",
        packaged: Some("NotoSerifTibetan-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSerifTibetan-Regular.ttf"],
        windows: &["himalaya.ttf"],
        macos: &["/System/Library/Fonts/Supplemental/Kokonor.ttf"],
    },
    Script {
        name: "thaana",
        packaged: Some("NotoSansThaana-Regular.ttf"),
        linux: &["/usr/share/fonts/truetype/noto/NotoSansThaana-Regular.ttf"],
        windows: &["mvboli.ttf"],
        macos: &[],
    },
    Script {
        name: "dejavu",
        packaged: Some("DejaVuSans.ttf"),
        linux: &["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"],
        windows: &["seguisym.ttf", "segoeui.ttf"],
        macos: &[
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/Library/Fonts/Arial Unicode.ttf",
        ],
    },
    Script {
        name: "cjk",
        packaged: Some("NotoSansCJKsc-Regular.otf"),
        linux: &["/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf"],
        windows: &[
            "msyh.ttc",
            "msyh.ttf",
            "simsun.ttc",
            "meiryo.ttc",
            "malgun.ttf",
        ],
        macos: &[
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/Supplemental/Songti.ttc",
        ],
    },
];

pub(crate) fn candidates(
    host: Host,
    packaged: Option<&Path>,
    windows_fonts: &Path,
) -> Vec<(&'static str, Vec<PathBuf>)> {
    SCRIPTS
        .iter()
        .map(|script| {
            let ours: Vec<PathBuf> = packaged
                .zip(script.packaged)
                .map(|(directory, file)| directory.join(file))
                .into_iter()
                .collect();
            let theirs: Vec<PathBuf> = match host {
                Host::Linux => script.linux.iter().map(PathBuf::from).collect(),
                Host::Windows => script
                    .windows
                    .iter()
                    .map(|file| windows_fonts.join(file))
                    .collect(),
                Host::MacOs => script.macos.iter().map(PathBuf::from).collect(),
            };
            let paths = if host == Host::Linux {
                theirs.into_iter().chain(ours).collect()
            } else {
                ours.into_iter().chain(theirs).collect()
            };
            (script.name, paths)
        })
        .collect()
}

pub(crate) fn windows_fonts() -> PathBuf {
    std::env::var_os("WINDIR")
        .map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from)
        .join("Fonts")
}

pub(crate) struct Chosen {
    pub(crate) name: &'static str,
    pub(crate) path: PathBuf,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn choose(
    candidates: Vec<(&'static str, Vec<PathBuf>)>,
    mut read: impl FnMut(&Path) -> Option<Vec<u8>>,
) -> Vec<Chosen> {
    let mut chosen: Vec<Chosen> = Vec::new();
    for (name, paths) in candidates {
        for path in paths {
            if chosen.iter().any(|earlier| earlier.path == path) {
                break;
            }
            let Some(bytes) = read(&path) else {
                continue;
            };
            if !looks_like_a_font(&bytes) {
                continue;
            }
            chosen.push(Chosen { name, path, bytes });
            break;
        }
    }
    chosen
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn held(faces: &[(String, Vec<u8>)]) -> Vec<Chosen> {
    let mut chosen: Vec<Chosen> = Vec::new();
    for script in &SCRIPTS {
        let Some(file) = script.packaged else {
            continue;
        };
        let Some((_, bytes)) = faces.iter().find(|(name, _)| name == file) else {
            continue;
        };
        if chosen.iter().any(|earlier| earlier.path == Path::new(file)) || !looks_like_a_font(bytes)
        {
            continue;
        }
        chosen.push(Chosen {
            name: script.name,
            path: PathBuf::from(file),
            bytes: bytes.clone(),
        });
    }
    chosen
}

pub(crate) fn definitions(chosen: Vec<Chosen>) -> egui::FontDefinitions {
    let names: Vec<String> = chosen.iter().map(|face| face.name.to_owned()).collect();
    let mut fonts = egui::FontDefinitions::default();
    for face in chosen {
        fonts.font_data.insert(
            face.name.to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(face.bytes)),
        );
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.extend(names.iter().cloned());
        }
    }
    fonts
}

pub(crate) fn looks_like_a_font(bytes: &[u8]) -> bool {
    let Some(tag) = be_u32(bytes, 0) else {
        return false;
    };
    let directory = match tag {
        0x0001_0000 | 0x7472_7565 | 0x4f54_544f => 0,
        0x7474_6366 => match be_u32(bytes, 8).zip(be_u32(bytes, 12)) {
            Some((count, offset)) if count > 0 => offset as usize,
            _ => return false,
        },
        _ => return false,
    };
    let Some(tables) = be_u16(bytes, directory + 4) else {
        return false;
    };
    let mut found = [false; 4];
    for index in 0..usize::from(tables) {
        let record = directory + 12 + index * 16;
        let (Some(name), Some(offset), Some(length)) = (
            bytes.get(record..record + 4),
            be_u32(bytes, record + 8),
            be_u32(bytes, record + 12),
        ) else {
            return false;
        };
        let end = (offset as usize).checked_add(length as usize);
        if end.is_none_or(|end| end > bytes.len()) {
            return false;
        }
        for (slot, wanted) in [b"cmap", b"head", b"hhea", b"maxp"].iter().enumerate() {
            if name == *wanted {
                found[slot] = true;
            }
        }
    }
    found.iter().all(|&present| present)
}

fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use super::{Host, candidates, choose, looks_like_a_font};

    fn tiny_font() -> Vec<u8> {
        let tags: [&[u8; 4]; 4] = [b"cmap", b"head", b"hhea", b"maxp"];
        let mut bytes = vec![0, 1, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0];
        let data_start = 12 + 16 * tags.len();
        for (index, tag) in tags.iter().enumerate() {
            bytes.extend_from_slice(*tag);
            bytes.extend_from_slice(&[0; 4]);
            let offset = u32::try_from(data_start + index * 4).expect("small");
            bytes.extend_from_slice(&offset.to_be_bytes());
            bytes.extend_from_slice(&4_u32.to_be_bytes());
        }
        bytes.resize(data_start + 4 * tags.len(), 0);
        bytes
    }

    fn paths_for(list: &[(&'static str, Vec<PathBuf>)], script: &str) -> Vec<PathBuf> {
        list.iter()
            .find(|(name, _)| *name == script)
            .map(|(_, paths)| paths.clone())
            .expect("a script of ours")
    }

    #[test]
    fn the_package_comes_first_away_from_linux() {
        let packaged = Path::new("/opt/panpdf/fonts/packaged");
        let fonts = Path::new("C:\\Windows\\Fonts");

        let windows = candidates(Host::Windows, Some(packaged), fonts);
        let thai = paths_for(&windows, "thai");
        assert_eq!(thai[0], packaged.join("NotoSansThai-Regular.ttf"));
        assert_eq!(thai[1], fonts.join("LeelawUI.ttf"));
        let devanagari = paths_for(&windows, "devanagari");
        assert_eq!(
            devanagari[0],
            packaged.join("NotoSansDevanagari-Regular.ttf")
        );
        assert_eq!(devanagari[1], fonts.join("Nirmala.ttf"));

        let mac = candidates(Host::MacOs, Some(packaged), fonts);
        assert_eq!(
            paths_for(&mac, "cjk")[0],
            packaged.join("NotoSansCJKsc-Regular.otf")
        );

        let linux = candidates(Host::Linux, Some(packaged), fonts);
        let thai = paths_for(&linux, "thai");
        assert_eq!(
            thai.first().map(PathBuf::as_path),
            Some(Path::new(
                "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf"
            ))
        );
        assert_eq!(
            thai.last(),
            Some(&packaged.join("NotoSansThai-Regular.ttf"))
        );

        let bare = candidates(Host::Windows, None, fonts);
        assert_eq!(paths_for(&bare, "thai")[0], fonts.join("LeelawUI.ttf"));
    }

    #[test]
    fn a_face_is_the_first_real_font_and_is_loaded_once() {
        let packaged = Path::new("P:\\fonts\\packaged");
        let fonts = Path::new("C:\\Windows\\Fonts");
        let mut disk: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        disk.insert(packaged.join("NotoSansThai-Regular.ttf"), tiny_font());
        disk.insert(fonts.join("segoeui.ttf"), tiny_font());
        disk.insert(fonts.join("LeelawUI.ttf"), b"not a font at all".to_vec());

        let chosen = choose(candidates(Host::Windows, Some(packaged), fonts), |path| {
            disk.get(path).cloned()
        });
        let found: Vec<(&str, PathBuf)> = chosen
            .iter()
            .map(|face| (face.name, face.path.clone()))
            .collect();
        assert_eq!(
            found,
            [
                ("thai", packaged.join("NotoSansThai-Regular.ttf")),
                ("arabic", fonts.join("segoeui.ttf")),
            ]
        );
    }

    #[test]
    fn every_script_has_a_face_the_package_carries() {
        let manifest = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fonts/manifest.json"),
        )
        .expect("the font manifest is in the tree");
        for script in &super::SCRIPTS {
            let file = script
                .packaged
                .unwrap_or_else(|| panic!("{} names no packaged face", script.name));
            assert!(
                manifest.contains(&format!("\"file\": \"{file}\"")),
                "{}: {file} is not in fonts/manifest.json",
                script.name
            );
        }
    }

    #[test]
    fn the_package_alone_spells_every_script() {
        use eframe::egui;
        use egui::epaint::text::{Fonts, TextOptions};

        let packaged = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fonts/packaged");
        let bare = Path::new("/no/windows/fonts");
        let read = |path: &Path| std::fs::read(path).ok();
        let words = [
            "ສະບາຍດີ",
            "สวัสดี",
            "नमस्ते",
            "হ্যালো",
            "வணக்கம்",
            "ආයුබෝවන්",
            "សួស្តី",
            "မင်္ဂလာပါ",
            "مرحبا",
            "שלום",
            "გამარჯობა",
            "Բարեւ",
            "ሰላም",
            "ਸਤਿ",
            "નમસ્તે",
            "ନମସ୍କାର",
            "నమస్కారం",
            "ನಮಸ್ಕಾರ",
            "നമസ്കാരം",
            "བཀྲ་ཤིས",
            "ސަލާމް",
            "你好",
            "こんにちは",
            "안녕하세요",
            "Xin chào",
            "Привет",
            "Γειά",
        ];
        let font = egui::FontId::proportional(14.0);

        let chosen = choose(candidates(Host::Windows, Some(&packaged), bare), read);
        let mut fonts = Fonts::new(TextOptions::default(), super::definitions(chosen));
        for word in words {
            assert!(fonts.has_glyphs(&font, word), "boxes for {word}");
        }

        let nothing = choose(candidates(Host::Windows, None, bare), read);
        let mut fonts = Fonts::new(TextOptions::default(), super::definitions(nothing));
        assert!(!fonts.has_glyphs(&font, "สวัสดี"), "the control has no Thai");
    }

    #[test]
    fn a_tab_uses_the_faces_it_was_handed() {
        let packaged = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fonts/packaged");
        let face = |file: &str| {
            (
                file.to_owned(),
                std::fs::read(packaged.join(file)).expect("a packaged face"),
            )
        };
        let start = [
            face("LiberationSans-Regular.ttf"),
            face("NotoSansLao-Regular.ttf"),
            face("NotoSansThai-Regular.ttf"),
        ];
        let names = |faces: &[(String, Vec<u8>)]| -> Vec<&str> {
            super::held(faces)
                .iter()
                .map(|chosen| chosen.name)
                .collect()
        };
        assert_eq!(names(&start), ["thai", "lao"]);
        let mut later = start.to_vec();
        later.push(face("NotoSansDevanagari-Regular.ttf"));
        later.push((
            "NotoSansKhmer-Regular.ttf".to_owned(),
            b"not a font".to_vec(),
        ));
        assert_eq!(names(&later), ["thai", "lao", "devanagari"]);
    }

    #[test]
    fn a_font_is_known_by_its_table_directory() {
        let font = tiny_font();
        assert!(looks_like_a_font(&font));
        assert!(!looks_like_a_font(&font[..font.len() - 1]));
        let mut no_cmap = font.clone();
        no_cmap[12..16].copy_from_slice(b"cmaq");
        assert!(!looks_like_a_font(&no_cmap));
        assert!(!looks_like_a_font(b"%PDF-1.7"));
        assert!(!looks_like_a_font(&[]));

        let mut collection = b"ttcf".to_vec();
        collection.extend_from_slice(&[0, 1, 0, 0]);
        collection.extend_from_slice(&1_u32.to_be_bytes());
        collection.extend_from_slice(&16_u32.to_be_bytes());
        let mut face = font;
        for index in 0..4 {
            let at = 12 + index * 16 + 8;
            let offset = u32::from_be_bytes(face[at..at + 4].try_into().expect("four")) + 16;
            face[at..at + 4].copy_from_slice(&offset.to_be_bytes());
        }
        collection.extend_from_slice(&face);
        assert!(looks_like_a_font(&collection));
        let mut empty = collection.clone();
        empty[8..12].copy_from_slice(&0_u32.to_be_bytes());
        assert!(!looks_like_a_font(&empty));
    }
}
