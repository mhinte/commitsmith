//! Today's date, as `YYYY-MM-DD`, for the changelog heading.
//!
//! No date crate for one line: days since the Unix epoch are turned into a civil
//! date with the algorithm Howard Hinnant published, which is exact for every
//! date this program will ever see.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    civil_from_days((seconds / 86_400) as i64)
}

/// `days` after 1970-01-01 as `YYYY-MM-DD`.
pub fn civil_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn the_epoch_and_a_known_day_come_out_right() {
        assert_eq!(civil_from_days(0), "1970-01-01");
        assert_eq!(civil_from_days(1), "1970-01-02");
        // 2026-11-26 is 20_783 days after the epoch.
        assert_eq!(civil_from_days(20_783), "2026-11-26");
    }
}
