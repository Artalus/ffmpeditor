pub mod dialogs;
pub mod export;
pub mod project_settings;
pub mod status_bar;
pub mod timeline;
pub mod viewer;

/// `m:ss`, or `h:mm:ss` from an hour on (always with `hours`), plus `decimals` fractional digits.
pub fn format_time(seconds: f64, decimals: usize, hours: bool) -> String {
    let scale = 10_u64.pow(decimals as u32);
    let total = (seconds.max(0.0) * scale as f64).round() as u64;
    let (secs, frac) = (total / scale, total % scale);
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    let mut text = if h > 0 || hours {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    };
    if decimals > 0 {
        text += &format!(".{frac:0decimals$}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_with_hours_and_milliseconds() {
        let time = |seconds| format_time(seconds, 3, true);
        assert_eq!(time(0.0), "0:00:00.000");
        assert_eq!(time(3723.4567), "1:02:03.457");
        assert_eq!(time(59.9996), "0:01:00.000");
        assert_eq!(time(-5.0), "0:00:00.000");
        assert_eq!(time(36_000.0), "10:00:00.000");
    }

    #[test]
    fn hours_are_shown_from_an_hour_on_by_default() {
        assert_eq!(format_time(0.0, 0, false), "0:00");
        assert_eq!(format_time(75.0, 0, false), "1:15");
        assert_eq!(format_time(59.6, 0, false), "1:00");
        assert_eq!(format_time(3725.0, 0, false), "1:02:05");
        assert_eq!(format_time(1.5, 1, false), "0:01.5");
        assert_eq!(format_time(61.25, 2, false), "1:01.25");
        assert_eq!(format_time(59.99, 1, false), "1:00.0");
    }
}
