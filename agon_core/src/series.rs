//! Recurring match series occurrence-date arithmetic.
//!
//! Shared between `agon_service` (which validates the occurrence count a
//! create request implies, so it knows about this before any series is
//! persisted) and `agon_worker`'s `materialize_series` workflow (which uses
//! the exact same function to compute each remaining occurrence's date) —
//! kept in one place deliberately, so the two can never drift apart on what
//! "the next occurrence" means for a given frequency.

use chrono::Datelike;

/// Every bounded series (`OnDate`/`AfterN`) is capped at this many
/// occurrences total, including the first.
pub const MAX_SERIES_OCCURRENCES: u32 = 99;

/// The calendar date of a series' occurrence at `index` (0-based; index 0 is
/// `first` itself), advancing from `first` at `frequency`. `None` for an
/// unrecognised frequency — callers validate `frequency` (one of "weekly" |
/// "fortnightly" | "monthly" | "yearly") before this is ever called, so
/// that's unreachable in practice, not a case worth a panic.
///
/// `weekly`/`fortnightly` just add whole weeks. `monthly`/`yearly` keep
/// `first`'s *ordinal* weekday-of-month (e.g. "the 2nd Tuesday") across the
/// month/year jump — a design decision made for this feature, not specified
/// by the data model: there's no single obvious meaning for "the Nth of the
/// month" once the series is anchored by a weekday rather than a
/// day-of-month, and this is the same rule most calendar apps use for
/// "monthly on the second Tuesday". A month too short for that ordinal (e.g.
/// a series anchored on a month's 5th Friday, landing in a 4-Friday month)
/// clamps to that month's last occurrence of the weekday instead of
/// skipping it.
pub fn occurrence_date(
    first: chrono::NaiveDate,
    frequency: &str,
    weekday: chrono::Weekday,
    index: u32,
) -> Option<chrono::NaiveDate> {
    match frequency {
        "weekly" => first.checked_add_signed(chrono::Duration::days(7 * i64::from(index))),
        "fortnightly" => first.checked_add_signed(chrono::Duration::days(14 * i64::from(index))),
        "monthly" => nth_weekday_of_month_offset(first, weekday, i64::from(index)),
        "yearly" => nth_weekday_of_month_offset(first, weekday, i64::from(index) * 12),
        _ => None,
    }
}

/// `first`'s ordinal weekday-of-month (the Nth `weekday` of its month),
/// advanced by `month_offset` months — clamped to the last occurrence of
/// `weekday` in the target month when that ordinal doesn't exist there.
/// Backs the `monthly`/`yearly` cases of [`occurrence_date`] (`yearly` is
/// simply `month_offset = 12 * years`).
fn nth_weekday_of_month_offset(
    first: chrono::NaiveDate,
    weekday: chrono::Weekday,
    month_offset: i64,
) -> Option<chrono::NaiveDate> {
    let ordinal = i64::from(first.day() - 1) / 7 + 1;
    let total_months = i64::from(first.month0()) + month_offset;
    let target_year = first.year() + i32::try_from(total_months.div_euclid(12)).ok()?;
    let target_month = u32::try_from(total_months.rem_euclid(12)).ok()? + 1;

    let mut days_with_weekday = Vec::new();
    let mut d = chrono::NaiveDate::from_ymd_opt(target_year, target_month, 1)?;
    while d.month() == target_month {
        if d.weekday() == weekday {
            days_with_weekday.push(d);
        }
        d = d.succ_opt()?;
    }
    let idx = usize::try_from(ordinal - 1).ok()?;
    days_with_weekday
        .get(idx)
        .or_else(|| days_with_weekday.last())
        .copied()
}

/// The `chrono::Weekday` for this codebase's `0 = Sunday .. 6 = Saturday`
/// convention (`SeriesRecord::weekday`, `CreateMatchSeriesInput::weekday`) —
/// *not* `chrono::Weekday`'s own ISO numbering, which starts the week on
/// Monday. `None` for anything outside `0..=6`.
pub fn weekday_from_sunday_index(n: u8) -> Option<chrono::Weekday> {
    use chrono::Weekday::{Fri, Mon, Sat, Sun, Thu, Tue, Wed};
    match n {
        0 => Some(Sun),
        1 => Some(Mon),
        2 => Some(Tue),
        3 => Some(Wed),
        4 => Some(Thu),
        5 => Some(Fri),
        6 => Some(Sat),
        _ => None,
    }
}

/// How many occurrences of `weekday` at `frequency`, starting at `first`
/// (inclusive, counted as occurrence 1), fall on or before `end`. Capped at
/// `MAX_SERIES_OCCURRENCES + 1` iterations so a far-future `end` can't make
/// this loop run away — anything that hits the cap is rejected by the
/// caller anyway, so the exact count past it is never needed.
pub fn count_occurrences_through(
    first: chrono::NaiveDate,
    frequency: &str,
    weekday: chrono::Weekday,
    end: chrono::NaiveDate,
) -> u32 {
    let mut count = 0u32;
    while count <= MAX_SERIES_OCCURRENCES {
        let Some(date) = occurrence_date(first, frequency, weekday, count) else {
            break;
        };
        if date > end {
            break;
        }
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn weekly_adds_seven_days_per_index() {
        let first = date(2026, 6, 2); // a Tuesday
        assert_eq!(
            occurrence_date(first, "weekly", Weekday::Tue, 0),
            Some(first)
        );
        assert_eq!(
            occurrence_date(first, "weekly", Weekday::Tue, 1),
            Some(date(2026, 6, 9))
        );
        assert_eq!(
            occurrence_date(first, "weekly", Weekday::Tue, 4),
            Some(date(2026, 6, 30))
        );
    }

    #[test]
    fn fortnightly_adds_fourteen_days_per_index() {
        let first = date(2026, 6, 2);
        // index 2 => +28 days: June 2 -> June 30.
        assert_eq!(
            occurrence_date(first, "fortnightly", Weekday::Tue, 2),
            Some(date(2026, 6, 30))
        );
    }

    #[test]
    fn monthly_keeps_the_same_ordinal_weekday() {
        // 2026-06-02 is the 1st Tuesday of June.
        let first = date(2026, 6, 2);
        assert_eq!(
            occurrence_date(first, "monthly", Weekday::Tue, 1),
            Some(date(2026, 7, 7)) // 1st Tuesday of July
        );
        assert_eq!(
            occurrence_date(first, "monthly", Weekday::Tue, 2),
            Some(date(2026, 8, 4)) // 1st Tuesday of August
        );
    }

    #[test]
    fn monthly_clamps_when_the_ordinal_weekday_does_not_exist() {
        // 2026-01-30 is the 5th Friday of January 2026.
        let first = date(2026, 1, 30);
        assert_eq!(first.weekday(), Weekday::Fri);
        // February 2026 has only 4 Fridays (2026 is not a leap year, Feb has
        // 28 days) — clamp to the last (4th) Friday rather than skip.
        let next = occurrence_date(first, "monthly", Weekday::Fri, 1).unwrap();
        assert_eq!(next, date(2026, 2, 27));
        assert_eq!(next.weekday(), Weekday::Fri);
    }

    #[test]
    fn yearly_keeps_month_and_ordinal_weekday() {
        let first = date(2026, 3, 10); // 2nd Tuesday of March
        assert_eq!(first.weekday(), Weekday::Tue);
        assert_eq!(
            occurrence_date(first, "yearly", Weekday::Tue, 1),
            Some(date(2027, 3, 9)) // 2nd Tuesday of March 2027
        );
    }

    #[test]
    fn count_through_counts_first_as_one() {
        let first = date(2026, 6, 2);
        // Same day as first → just the first occurrence.
        assert_eq!(
            count_occurrences_through(first, "weekly", Weekday::Tue, first),
            1
        );
    }

    #[test]
    fn count_through_is_zero_when_end_precedes_first() {
        let first = date(2026, 6, 2);
        let end = date(2026, 6, 1);
        assert_eq!(
            count_occurrences_through(first, "weekly", Weekday::Tue, end),
            0
        );
    }

    #[test]
    fn count_through_matches_weekly_span() {
        let first = date(2026, 1, 6); // a Tuesday
        // 4 weekly occurrences: Jan 6, 13, 20, 27.
        let end = date(2026, 1, 27);
        assert_eq!(
            count_occurrences_through(first, "weekly", Weekday::Tue, end),
            4
        );
    }

    #[test]
    fn weekday_from_sunday_index_matches_convention() {
        assert_eq!(weekday_from_sunday_index(0), Some(Weekday::Sun));
        assert_eq!(weekday_from_sunday_index(1), Some(Weekday::Mon));
        assert_eq!(weekday_from_sunday_index(6), Some(Weekday::Sat));
        assert_eq!(weekday_from_sunday_index(7), None);
    }

    #[test]
    fn count_through_caps_without_looping_forever() {
        let first = date(2000, 1, 1);
        let far_future = date(9999, 1, 1);
        let count = count_occurrences_through(first, "weekly", first.weekday(), far_future);
        assert_eq!(count, MAX_SERIES_OCCURRENCES + 1);
    }
}
