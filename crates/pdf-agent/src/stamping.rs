use pdf_edit::Command;
use pdf_edit::stamp::{Edge, Facts, Only, Side, Spot, Stamp, pages_of, worded};

pub const FALLBACK_FAMILY: &str = "DejaVu Sans";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    PageNumbers,
    HeaderFooter,
    Watermark,
}

impl Kind {
    #[must_use]
    pub fn of(word: &str) -> Option<Self> {
        match word.trim() {
            "page_numbers" | "page_number" | "numbers" => Some(Self::PageNumbers),
            "header_footer" | "header" | "footer" => Some(Self::HeaderFooter),
            "watermark" => Some(Self::Watermark),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PageNumbers => "page_numbers",
            Self::HeaderFooter => "header_footer",
            Self::Watermark => "watermark",
        }
    }

    const fn wording(self) -> &'static str {
        match self {
            Self::PageNumbers => "{page}",
            Self::HeaderFooter => "{file}",
            Self::Watermark => "DRAFT",
        }
    }

    const fn spot(self) -> Spot {
        match self {
            Self::PageNumbers => Spot::Along {
                edge: Edge::Footer,
                side: Side::Centre,
            },
            Self::HeaderFooter => Spot::Along {
                edge: Edge::Header,
                side: Side::Left,
            },
            Self::Watermark => Spot::Middle,
        }
    }

    const fn size(self) -> f64 {
        match self {
            Self::PageNumbers => 10.0,
            Self::HeaderFooter => 9.0,
            Self::Watermark => 60.0,
        }
    }

    const fn fill(self) -> [f64; 3] {
        match self {
            Self::PageNumbers | Self::HeaderFooter => [0.0; 3],
            Self::Watermark => [0.5; 3],
        }
    }

    const fn opacity(self) -> f64 {
        match self {
            Self::PageNumbers | Self::HeaderFooter => 1.0,
            Self::Watermark => 0.5,
        }
    }
}

#[must_use]
pub fn spot_of(word: &str) -> Option<Spot> {
    let word = word.trim().to_lowercase().replace('-', "_");
    if word == "middle" || word == "centre_of_page" || word == "center_of_page" {
        return Some(Spot::Middle);
    }
    let (edge, side) = word.split_once('_')?;
    let edge = match edge {
        "header" | "top" => Edge::Header,
        "footer" | "bottom" => Edge::Footer,
        _ => return None,
    };
    let side = match side {
        "left" => Side::Left,
        "centre" | "center" | "middle" => Side::Centre,
        "right" => Side::Right,
        _ => return None,
    };
    Some(Spot::Along { edge, side })
}

#[must_use]
pub const fn spot_as_str(spot: Spot) -> &'static str {
    match spot {
        Spot::Middle => "middle",
        Spot::Along {
            edge: Edge::Header,
            side: Side::Left,
        } => "header_left",
        Spot::Along {
            edge: Edge::Header,
            side: Side::Centre,
        } => "header_centre",
        Spot::Along {
            edge: Edge::Header,
            side: Side::Right,
        } => "header_right",
        Spot::Along {
            edge: Edge::Footer,
            side: Side::Left,
        } => "footer_left",
        Spot::Along {
            edge: Edge::Footer,
            side: Side::Centre,
        } => "footer_centre",
        Spot::Along {
            edge: Edge::Footer,
            side: Side::Right,
        } => "footer_right",
    }
}

#[must_use]
pub fn only_of(word: &str) -> Option<Only> {
    match word.trim() {
        "every" | "all" => Some(Only::Every),
        "odd" => Some(Only::Odd),
        "even" => Some(Only::Even),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub kind: Kind,
    pub wording: Option<String>,
    pub spot: Option<Spot>,
    pub pages: String,
    pub only: Only,
    pub start: Option<i64>,
    pub family: Option<String>,
    pub size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: bool,
    pub fill: Option<[f64; 3]>,
    pub opacity: Option<f64>,
    pub margin: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub stamp: Stamp,
    pub pages: Vec<usize>,
    pub start: i64,
}

impl Asked {
    #[must_use]
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            wording: None,
            spot: None,
            pages: String::new(),
            only: Only::Every,
            start: None,
            family: None,
            size: None,
            bold: None,
            italic: false,
            fill: None,
            opacity: None,
            margin: None,
        }
    }

    pub fn prepare(&self, page_count: usize) -> Result<Prepared, String> {
        let pages =
            pages_of(&self.pages, page_count, self.only).map_err(|error| error.to_string())?;
        let wording = self
            .wording
            .clone()
            .unwrap_or_else(|| self.kind.wording().to_owned());
        let family = self.family.clone().unwrap_or_else(|| {
            crate::about::family_for(&wording).unwrap_or_else(|| FALLBACK_FAMILY.to_owned())
        });
        let first = pages.first().copied().unwrap_or(0);
        let start = self
            .start
            .unwrap_or_else(|| i64::try_from(first).unwrap_or(0) + 1);
        Ok(Prepared {
            stamp: Stamp {
                wording,
                spot: self.spot.unwrap_or_else(|| self.kind.spot()),
                family,
                size: self.size.unwrap_or_else(|| self.kind.size()),
                bold: self.bold.unwrap_or(self.kind == Kind::Watermark),
                italic: self.italic,
                fill: Some(self.fill.unwrap_or_else(|| self.kind.fill())),
                opacity: self.opacity.unwrap_or_else(|| self.kind.opacity()),
                margin: self.margin.unwrap_or(36.0),
            },
            pages,
            start,
        })
    }
}

impl Prepared {
    #[must_use]
    pub fn facts(&self, page: usize, (count, name, today): (usize, &str, &str)) -> Facts {
        let first = self.pages.first().copied().unwrap_or(0);
        let further = i64::try_from(page.saturating_sub(first)).unwrap_or(i64::MAX);
        Facts {
            number: self.start.saturating_add(further),
            count: i64::try_from(count).unwrap_or(i64::MAX),
            name: name.to_owned(),
            today: today.to_owned(),
        }
    }

    #[must_use]
    pub fn commands(&self, (count, name, today): (usize, &str, &str)) -> Vec<Command> {
        let first = self.pages.first().copied().unwrap_or(0);
        self.pages
            .iter()
            .map(|&page_index| Command::Stamp {
                page_index,
                stamp: self.stamp.clone(),
                facts: self.facts(page_index, (count, name, today)),
                share_from: (page_index != first).then_some(first),
            })
            .collect()
    }

    #[must_use]
    pub fn line_on(&self, page: usize, around: (usize, &str, &str)) -> Option<String> {
        worded(&self.stamp.wording, &self.facts(page, around)).ok()
    }

    #[must_use]
    pub fn said(&self, around: (usize, &str, &str)) -> String {
        let first = self.pages.first().copied().unwrap_or(0);
        let shown = self
            .line_on(first, around)
            .unwrap_or_else(|| self.stamp.wording.clone());
        format!(
            "Stamped {} page{} at {}: page {} reads \u{201c}{shown}\u{201d}",
            self.pages.len(),
            if self.pages.len() == 1 { "" } else { "s" },
            spot_as_str(self.stamp.spot),
            first + 1
        )
    }
}

#[must_use]
pub fn written_day(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{day:02}/{month:02}/{year}")
}

#[must_use]
pub fn today() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    written_day(seconds)
}

#[cfg(test)]
mod tests;
