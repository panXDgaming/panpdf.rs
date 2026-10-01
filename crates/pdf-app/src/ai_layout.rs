pub const LEAST_ROWS: usize = 2;

pub const LEAST_CAPTION: f32 = 48.0;

const MOST_ROWS_EVER: f32 = 512.0;

#[must_use]
pub const fn rows_shown(wrapped: usize, most: usize) -> usize {
    let ceiling = if most < LEAST_ROWS { LEAST_ROWS } else { most };
    if wrapped < LEAST_ROWS {
        LEAST_ROWS
    } else if wrapped > ceiling {
        ceiling
    } else {
        wrapped
    }
}

#[must_use]
pub fn most_rows(panel_height: f32, row_height: f32, chrome: f32) -> usize {
    if !row_height.is_finite() || row_height <= 0.0 {
        return LEAST_ROWS;
    }
    let room = panel_height / 2.0 - chrome;
    if !room.is_finite() || room <= 0.0 {
        return LEAST_ROWS;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "floored and clamped to 0..=512 on the line above, so the \
                  cast is exact and not negative"
    )]
    let fits = (room / row_height).floor().clamp(0.0, MOST_ROWS_EVER) as usize;
    if fits < LEAST_ROWS { LEAST_ROWS } else { fits }
}

#[must_use]
pub fn composer_height(rows: usize, row_height: f32, chrome: f32) -> f32 {
    let rows = if rows < LEAST_ROWS { LEAST_ROWS } else { rows };
    #[expect(
        clippy::cast_precision_loss,
        reason = "a row count is small; no panel has 2^24 rows in it"
    )]
    let rows = rows as f32;
    rows * row_height + chrome
}

#[must_use]
pub fn caption_room(available: f32, gap: f32) -> Option<f32> {
    let room = available - gap;
    (room.is_finite() && room >= LEAST_CAPTION).then_some(room)
}

#[cfg(test)]
mod tests {
    use super::{LEAST_CAPTION, LEAST_ROWS, caption_room, composer_height, most_rows, rows_shown};

    #[test]
    fn the_box_shows_what_was_typed_within_its_bounds() {
        assert_eq!(rows_shown(0, 8), 2);
        assert_eq!(rows_shown(5, 8), 5);
        assert_eq!(rows_shown(40, 8), 8);
        assert_eq!(rows_shown(40, 1), LEAST_ROWS);
    }

    #[test]
    fn the_ceiling_is_half_the_panel() {
        assert_eq!(most_rows(600.0, 18.0, 44.0), 14);
        let whole_panel = ((600.0 - 44.0) / 18.0_f32).floor();
        assert!(
            (whole_panel - 30.0).abs() < 0.5,
            "the control formula answers 30, not {whole_panel}"
        );
        assert_ne!(most_rows(600.0, 18.0, 44.0), 30);
        assert_eq!(most_rows(40.0, 18.0, 44.0), LEAST_ROWS);
        assert_eq!(most_rows(600.0, 0.0, 44.0), LEAST_ROWS);
    }

    #[expect(
        clippy::float_cmp,
        reason = "every number here is exact in binary, and the point of the \
                  test is the exact height"
    )]
    #[test]
    fn the_box_is_its_rows_and_its_furniture() {
        assert_eq!(composer_height(2, 18.0, 44.0), 80.0);
        assert_eq!(composer_height(14, 18.0, 44.0), 296.0);
        assert_eq!(composer_height(2, 18.0, 0.0), 36.0);
        assert_eq!(composer_height(0, 18.0, 44.0), 80.0);
    }

    #[test]
    fn the_model_caption_gets_what_the_row_has_left_or_goes() {
        assert_eq!(caption_room(100.0, 8.0), Some(92.0));
        assert_eq!(caption_room(LEAST_CAPTION + 8.0, 8.0), Some(LEAST_CAPTION));
        assert_eq!(caption_room(LEAST_CAPTION + 7.0, 8.0), None);
        assert_eq!(caption_room(-5.0, 8.0), None);
        assert_eq!(caption_room(f32::NAN, 8.0), None);
    }
}
