use crate::dates::moment;
use crate::recent::{Ago, ago};
use crate::wording::Assistant;

#[must_use]
pub fn when(then: u64, now: u64) -> Assistant {
    match ago(then, now) {
        Ago::Long => {
            let day = moment(then);
            Assistant::ChatDate {
                day: day.day,
                month: day.month,
                year: day.year,
            }
        }
        near => Assistant::ChatAgo(near),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sections<T> {
    pub here: Vec<T>,
    pub elsewhere: Vec<T>,
}

#[must_use]
pub fn sections<'a, T>(
    chats: &'a [T],
    place: &str,
    places_of: impl Fn(&'a T) -> &'a [String],
) -> Sections<&'a T> {
    let (here, elsewhere) = chats
        .iter()
        .partition(|chat| !place.is_empty() && places_of(chat).iter().any(|kept| kept == place));
    Sections { here, elsewhere }
}

#[cfg(test)]
mod tests {
    use super::{sections, when};
    use crate::recent::Ago;
    use crate::wording::{Assistant, Lang};

    const NOW: u64 = 1_760_000_000;

    #[test]
    fn a_chat_is_dated_by_how_long_ago_until_it_is_a_month_old() {
        assert_eq!(when(NOW - 30, NOW), Assistant::ChatAgo(Ago::JustNow));
        assert_eq!(when(NOW - 600, NOW), Assistant::ChatAgo(Ago::Minutes(10)));
        assert_eq!(when(NOW - 7_300, NOW), Assistant::ChatAgo(Ago::Hours(2)));
        assert_eq!(when(NOW - 90_000, NOW), Assistant::ChatAgo(Ago::Yesterday));
        assert_eq!(
            when(NOW - 5 * 86_400, NOW),
            Assistant::ChatAgo(Ago::Days(5))
        );
    }

    #[test]
    fn an_older_chat_is_dated_by_its_day() {
        let then = 1_709_209_800;
        assert_eq!(
            when(then, NOW),
            Assistant::ChatDate {
                day: 29,
                month: 2,
                year: 2024
            }
        );
        assert_eq!(when(then, NOW).say(Lang::English), "29 Feb 2024");
    }

    #[test]
    fn the_ages_are_said_short() {
        let said = |ago| Assistant::ChatAgo(ago).say(Lang::English);
        assert_eq!(said(Ago::Minutes(5)), "5 min ago");
        assert_eq!(said(Ago::Hours(3)), "3 h ago");
        assert_eq!(said(Ago::Yesterday), "Yesterday");
    }

    #[test]
    fn the_chats_of_this_document_come_first_and_the_rest_after() {
        struct Chat(&'static str, Vec<String>);
        let chats = [
            Chat("a", vec!["/x.pdf".to_owned()]),
            Chat("b", vec!["/y.pdf".to_owned(), "/x.pdf".to_owned()]),
            Chat("c", vec![]),
        ];
        let split = sections(&chats, "/x.pdf", |chat| chat.1.as_slice());
        let names = |list: &[&Chat]| list.iter().map(|chat| chat.0).collect::<Vec<_>>();
        assert_eq!(names(&split.here), ["a", "b"]);
        assert_eq!(names(&split.elsewhere), ["c"]);
        let nowhere = sections(&chats, "", |chat| chat.1.as_slice());
        assert!(nowhere.here.is_empty());
        assert_eq!(nowhere.elsewhere.len(), 3);
    }
}
