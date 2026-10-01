use std::collections::BTreeSet;

use super::{Chat, chat_about, is_a_chat_id, name_for, newest_first, read, title_of, write};
use crate::connect::{Attachment, AttachmentKind, Provider, Raw, ToolCall, ToolResult, Turn};
use crate::json::Json;

fn a_chat() -> Chat {
    Chat {
        id: "000000012345".to_owned(),
        title: "Summarise page 2".to_owned(),
        changed: 12_345,
        model: "qwen/qwen3.8-27b".to_owned(),
        documents: vec!["report.pdf".to_owned(), "notes.pdf".to_owned()],
        places: vec![
            "/home/someone/report.pdf".to_owned(),
            "/home/someone/notes.pdf".to_owned(),
        ],
        turns: vec![
            Turn::person_with(
                "Summarise page 2",
                vec![
                    Attachment::text("notes.pdf", "the words of the file"),
                    Attachment::image("cover.png", "image/png", vec![1, 2, 3]),
                ],
            ),
            Turn::Model {
                text: "Let me read it.".to_owned(),
                calls: vec![ToolCall {
                    id: "call_1".to_owned(),
                    name: "read_text".to_owned(),
                    arguments: Json::object([("first", Json::count(2))]),
                    problem: None,
                }],
                raw: Some(Raw {
                    provider: Provider::Gemini,
                    items: vec![Json::object([("thought_signature", Json::text("abc"))])],
                }),
            },
            Turn::Results {
                results: vec![ToolResult::said("call_1", "Two paragraphs.")],
            },
            Turn::model("Page 2 is about bonds."),
        ],
    }
}

#[test]
fn a_conversation_written_reads_back_as_itself() {
    let chat = a_chat();
    let back = read(&write(&chat)).expect("it reads");
    assert_eq!(back.id, chat.id);
    assert_eq!(back.title, chat.title);
    assert_eq!(back.changed, chat.changed);
    assert_eq!(back.model, chat.model);
    assert_eq!(back.documents, chat.documents);
    assert_eq!(back.places, chat.places);
    assert_eq!(back.turns.len(), chat.turns.len());
    assert_eq!(back.turns[1], chat.turns[1]);
    assert_eq!(back.turns[2], chat.turns[2]);
    assert_eq!(back.turns[3], chat.turns[3]);
}

#[test]
fn a_text_attachment_keeps_its_words_and_a_picture_only_its_name() {
    let back = read(&write(&a_chat())).expect("it reads");
    let Turn::Person { attachments, .. } = &back.turns[0] else {
        panic!("the first turn is the person's");
    };
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0].name, "notes.pdf");
    assert_eq!(attachments[0].kind, AttachmentKind::Text);
    assert_eq!(
        attachments[0].as_text(),
        "the words of the file",
        "a follow-up in a reopened chat still has the file to read"
    );
    assert_eq!(attachments[1].name, "cover.png");
    assert_eq!(
        attachments[1].kind,
        AttachmentKind::Image {
            media_type: "image/png".to_owned()
        }
    );
    assert!(
        attachments[1].bytes.is_empty(),
        "a picture is not kept in the chat file"
    );
}

#[test]
fn a_text_attachment_too_big_to_keep_in_a_chat_file_is_kept_by_name_only() {
    let big = "word ".repeat(30_000);
    let chat = Chat {
        turns: vec![Turn::person_with(
            "read it",
            vec![Attachment::text("big.txt", big.clone())],
        )],
        ..a_chat()
    };
    let written = write(&chat);
    assert!(written.len() < big.len(), "the file stays small");
    let back = read(&written).expect("it reads");
    assert!(back.turns[0].attachments()[0].bytes.is_empty());
}

#[test]
fn a_chat_written_from_borrowed_turns_is_the_same_file_as_one_written_whole() {
    let chat = a_chat();
    let bare = Chat {
        turns: Vec::new(),
        ..chat.clone()
    };
    assert_eq!(super::write_turns(&bare, &chat.turns), write(&chat));
}

#[test]
fn what_cannot_be_read_faithfully_is_not_read() {
    assert_eq!(read(""), None);
    assert_eq!(read("{"), None);
    assert_eq!(read("{\"turns\":[]}"), None);
    assert_eq!(read("{\"version\":99,\"turns\":[]}"), None);
    assert_eq!(
        read("{\"version\":1,\"turns\":[{\"said\":\"nobody\"}]}"),
        None
    );
}

#[test]
fn an_empty_conversation_is_still_a_conversation() {
    let empty = Chat {
        turns: Vec::new(),
        ..a_chat()
    };
    assert_eq!(read(&write(&empty)), Some(empty));
}

#[test]
fn a_chat_is_titled_by_the_first_question() {
    assert_eq!(
        title_of(&[
            Turn::model("Hello"),
            Turn::person("  Change page 2\nand then page 3  "),
        ]),
        "Change page 2"
    );
    assert_eq!(title_of(&[]), "");
    assert_eq!(title_of(&[Turn::person("   ")]), "");
}

#[test]
fn a_long_title_is_shortened_at_a_word() {
    let long = "please summarise every interesting investment for the coming year and write it out";
    let title = title_of(&[Turn::person(long)]);
    assert!(title.ends_with('\u{2026}'), "{title}");
    assert!(title.chars().count() <= 61, "{title}");
    assert!(long.starts_with(title.trim_end_matches('\u{2026}')));
    assert!(!title.contains("comin\u{2026}"));
}

#[test]
fn the_list_is_newest_first_and_survives_a_bad_file() {
    let one = write(&Chat {
        id: "000000000001".to_owned(),
        changed: 10,
        ..a_chat()
    });
    let two = write(&Chat {
        id: "000000000002".to_owned(),
        changed: 20,
        ..a_chat()
    });
    let listed = newest_first(vec![
        ("000000000001".to_owned(), one),
        ("000000000003".to_owned(), "not a chat at all".to_owned()),
        ("000000000002".to_owned(), two),
    ]);
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, "000000000002");
    assert_eq!(listed[1].id, "000000000001");
}

#[test]
fn a_name_is_free_sortable_and_safe_to_join_to_a_path() {
    let mut taken = BTreeSet::new();
    let first = name_for(12_345, &taken);
    assert_eq!(first, "000000012345");
    taken.insert(first.clone());
    let second = name_for(12_345, &taken);
    assert_eq!(second, "000000012345-1");
    assert!(name_for(99, &taken) < first);
    for name in [&first, &second] {
        assert!(
            name.chars()
                .all(|letter| letter.is_ascii_digit() || letter == '-'),
            "{name}"
        );
    }
}

#[test]
fn a_chat_from_before_documents_were_kept_still_reads() {
    let old = r#"{"version":1,"id":"1","title":"t","changed":1,"model":"m","turns":[]}"#;
    let chat = read(old).expect("it reads");
    assert!(chat.documents.is_empty());
    assert!(chat.places.is_empty(), "and it is about no file");
}

#[test]
fn a_document_brings_back_its_own_chat() {
    let about = |id: &str, changed: u64, places: &[&str]| Chat {
        id: id.to_owned(),
        changed,
        documents: vec!["report.pdf".to_owned()],
        places: places.iter().map(|place| (*place).to_owned()).collect(),
        ..Chat::default()
    };
    let chats = newest_first([
        ("10".to_owned(), write(&about("10", 10, &["/a/report.pdf"]))),
        (
            "20".to_owned(),
            write(&about("20", 20, &["/a/report.pdf", "/a/report-edited.pdf"])),
        ),
        ("30".to_owned(), write(&about("30", 30, &["/c/other.pdf"]))),
    ]);
    let found = |place: &str| chat_about(&chats, place).map(|chat| chat.id.as_str());
    assert_eq!(found("/a/report.pdf"), Some("20"));
    assert_eq!(found("/a/report-edited.pdf"), Some("20"));
    assert_eq!(found("/b/report.pdf"), None, "the same name elsewhere");
    assert_eq!(found(""), None, "a document saved nowhere yet");
    assert_eq!(found("/c/other.pdf"), Some("30"));
}

#[test]
fn a_chat_is_known_by_its_file_name_and_never_by_a_path_written_inside_it() {
    let sly = write(&Chat {
        id: "/home/someone/Documents/report".to_owned(),
        ..a_chat()
    });
    assert_eq!(
        read(&sly).expect("it reads").id,
        "",
        "an id that is not digits is not an id"
    );
    let listed = newest_first(vec![
        ("000000000007".to_owned(), sly.clone()),
        ("../report".to_owned(), sly.clone()),
        ("000000000007 copy".to_owned(), sly),
    ]);
    assert_eq!(
        listed.len(),
        1,
        "only a file that is named like a chat is one"
    );
    assert_eq!(
        listed[0].id, "000000000007",
        "its name is its id, whatever it says"
    );
    for good in ["0", "000000012345", "000000012345-3"] {
        assert!(is_a_chat_id(good), "{good}");
    }
    for bad in [
        "",
        "-",
        "1-",
        "-1",
        "1-2-3",
        "a",
        "/etc/passwd",
        "..",
        "1 2",
        "1/2",
    ] {
        assert!(!is_a_chat_id(bad), "{bad:?}");
    }
}

#[test]
fn a_long_title_in_thai_is_cut_at_its_sixtieth_letter_and_not_at_a_byte_count() {
    let space_early = format!("{} {}", "\u{0e01}".repeat(11), "\u{0e02}".repeat(70));
    let title = title_of(&[Turn::person(space_early)]);
    assert_eq!(
        title.chars().count(),
        61,
        "the space is at the eleventh letter, too early to cut at: sixty letters fit"
    );
    let leading = format!("{}\u{0e40}{}", "\u{0e01}".repeat(59), "\u{0e02}".repeat(30));
    let title = title_of(&[Turn::person(leading)]);
    assert!(
        !title.trim_end_matches('\u{2026}').ends_with('\u{0e40}'),
        "a vowel written before its letter is not left without it: {title}"
    );
    let words = format!("{} {}", "\u{0e01}".repeat(40), "\u{0e02}".repeat(40));
    let title = title_of(&[Turn::person(words)]);
    assert_eq!(
        title.chars().count(),
        41,
        "cut at the space, which is past the middle"
    );
}
