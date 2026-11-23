use chrono::{DateTime, Local, Utc};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn token_count(value: u64) -> String {
    format_compact(value)
}

fn format_compact(value: u64) -> String {
    const K: f64 = 1_000.0;
    const M: f64 = 1_000_000.0;
    const B: f64 = 1_000_000_000.0;
    match value {
        0..=999 => value.to_string(),
        1_000..=9_949 => format!("{:.1}K", value as f64 / K),
        9_950..=999_999 => format!("{}K", ((value as f64 / K).round()) as u64),
        1_000_000..=9_949_999 => format!("{:.2}M", value as f64 / M),
        9_950_000..=999_999_999 => format!("{:.1}M", value as f64 / M),
        _ => format!("{:.2}B", value as f64 / B),
    }
}

pub fn reasoning(value: u64, partial: bool) -> String {
    let mut rendered = token_count(value);
    if partial {
        rendered.push('+');
    }
    rendered
}

pub fn percentage(part: u64, total: u64) -> String {
    if total == 0 {
        return "0%".into();
    }
    let pct = part as f64 * 100.0 / total as f64;
    if pct >= 10.0 {
        format!("{pct:.0}%")
    } else if pct > 0.0 {
        format!("{pct:.1}%")
    } else {
        "0%".into()
    }
}

pub fn usage_bar(part: u64, total: u64, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if total == 0 {
        return "░".repeat(width);
    }
    let filled = ((part as f64 / total as f64) * width as f64)
        .round()
        .clamp(0.0, width as f64) as usize;
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

pub fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

pub fn truncate_ellipsis(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if display_width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 1 {
        return "…".into();
    }
    let target_width = max_width - 1;
    let mut out = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if used + ch_width > target_width {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    out
}

pub fn pad_left(value: &str, width: usize) -> String {
    let current = display_width(value);
    if current >= width {
        value.to_string()
    } else {
        format!("{}{value}", " ".repeat(width - current))
    }
}

#[allow(dead_code)]
pub fn with_commas(value: u64) -> String {
    let s = value.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (idx, ch) in s.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

pub fn relative_time(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let delta = now.signed_duration_since(at);
    let secs = delta.num_seconds();
    if secs < 60 {
        return "now".into();
    }
    let mins = delta.num_minutes();
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = delta.num_hours();
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = delta.num_days();
    if days < 30 {
        return format!("{days}d");
    }
    let local = at.with_timezone(&Local);
    if days >= 365 {
        local.format("%Y-%m").to_string()
    } else {
        local.format("%m-%d").to_string()
    }
}

pub fn cache_hit_rate(cache_read: u64, input_total: u64) -> Option<String> {
    if input_total == 0 {
        None
    } else {
        Some(percentage(cache_read.min(input_total), input_total))
    }
}

pub fn shorten_home(path: &str) -> String {
    let home = dirs::home_dir();
    shorten_home_with(path, home.as_deref().and_then(|p| p.to_str()))
}

fn shorten_home_with(path: &str, home: Option<&str>) -> String {
    let Some(home) = home else {
        return path.to_string();
    };
    let home = home.trim_end_matches(['/', '\\']);
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    if let Some(rest) = path.strip_prefix(home)
        && (rest.starts_with('/') || rest.starts_with('\\'))
    {
        return format!("~{rest}");
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn formats_human_tokens() {
        assert_eq!(token_count(0), "0");
        assert_eq!(token_count(999), "999");
        assert_eq!(token_count(1_234), "1.2K");
        assert_eq!(token_count(18_400), "18K");
        assert_eq!(token_count(892_000), "892K");
        assert_eq!(token_count(1_840_000), "1.84M");
        assert_eq!(token_count(18_700_000), "18.7M");
        assert_eq!(token_count(1_200_000_000), "1.20B");
    }

    #[test]
    fn formats_percentages() {
        assert_eq!(percentage(0, 0), "0%");
        assert_eq!(percentage(81, 100), "81%");
        assert_eq!(percentage(1, 100), "1.0%");
    }

    #[test]
    fn truncates_long_names_and_wide_chars() {
        assert_eq!(truncate_ellipsis("abcdef", 0), "");
        assert_eq!(truncate_ellipsis("abcdef", 4), "abc…");
        assert_eq!(truncate_ellipsis("abcdef", 6), "abcdef");
        assert_eq!(truncate_ellipsis("abcdef", 1), "…");

        // Korean / CJK wide characters (width 2 each)
        assert_eq!(display_width("가나다"), 6);
        assert_eq!(truncate_ellipsis("가나다", 6), "가나다");
        assert_eq!(truncate_ellipsis("가나다", 5), "가나…");
        assert_eq!(truncate_ellipsis("가나다", 4), "가…");
        assert_eq!(truncate_ellipsis("가나다", 3), "가…");
        assert_eq!(truncate_ellipsis("가나다", 2), "…");
        assert_eq!(truncate_ellipsis("가나다", 1), "…");
        assert_eq!(truncate_ellipsis("a가b나", 4), "a가…");
        assert_eq!(truncate_ellipsis("日本語テスト", 7), "日本語…");

        for sample in ["가나다라", "a가b나c", "日本語", "hello🚀world"] {
            for max_w in 0..=10 {
                let res = truncate_ellipsis(sample, max_w);
                assert!(
                    display_width(&res) <= max_w,
                    "display_width({res:?}) = {} > {max_w} for {sample:?}",
                    display_width(&res)
                );
            }
        }
    }

    #[test]
    fn pads_left_by_display_width() {
        assert_eq!(pad_left("42", 5), "   42");
        assert_eq!(pad_left("12345", 3), "12345");
        assert_eq!(pad_left("가", 4), "  가");
        assert_eq!(display_width(&pad_left("한글", 6)), 6);
        assert_eq!(pad_left("한글", 3), "한글");
    }

    #[test]
    fn formats_with_commas() {
        assert_eq!(with_commas(0), "0");
        assert_eq!(with_commas(999), "999");
        assert_eq!(with_commas(1_000), "1,000");
        assert_eq!(with_commas(1_234_567), "1,234,567");
        assert_eq!(with_commas(1_000_000_000), "1,000,000,000");
    }

    #[test]
    fn formats_relative_time() {
        let now = Utc.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap();
        assert_eq!(relative_time(now + Duration::seconds(10), now), "now");
        assert_eq!(relative_time(now, now), "now");
        assert_eq!(relative_time(now - Duration::seconds(59), now), "now");
        assert_eq!(relative_time(now - Duration::seconds(60), now), "1m");
        assert_eq!(relative_time(now - Duration::minutes(59), now), "59m");
        assert_eq!(relative_time(now - Duration::minutes(60), now), "1h");
        assert_eq!(relative_time(now - Duration::hours(23), now), "23h");
        assert_eq!(relative_time(now - Duration::hours(24), now), "1d");
        assert_eq!(relative_time(now - Duration::days(29), now), "29d");

        let d30 = now - Duration::days(30);
        assert_eq!(
            relative_time(d30, now),
            d30.with_timezone(&Local).format("%m-%d").to_string()
        );

        let d400 = now - Duration::days(400);
        assert_eq!(
            relative_time(d400, now),
            d400.with_timezone(&Local).format("%Y-%m").to_string()
        );
    }

    #[test]
    fn formats_cache_hit_rate() {
        assert_eq!(cache_hit_rate(0, 0), None);
        assert_eq!(cache_hit_rate(50, 0), None);
        assert_eq!(cache_hit_rate(0, 100), Some("0%".into()));
        assert_eq!(cache_hit_rate(81, 100), Some("81%".into()));
        assert_eq!(cache_hit_rate(150, 100), Some("100%".into()));
        assert_eq!(cache_hit_rate(5, 1000), Some("0.5%".into()));
    }

    #[test]
    fn renders_usage_bar() {
        assert_eq!(usage_bar(50, 100, 10), "█████░░░░░");
        assert_eq!(usage_bar(0, 100, 4), "░░░░");
        assert_eq!(usage_bar(1, 1, 3), "███");
    }

    #[test]
    fn shortens_home_paths_safely() {
        assert_eq!(shorten_home_with("/Users/alice", Some("/Users/alice")), "~");
        assert_eq!(
            shorten_home_with("/Users/alice/work/repo", Some("/Users/alice")),
            "~/work/repo"
        );
        assert_eq!(
            shorten_home_with("/Users/alice2/work", Some("/Users/alice")),
            "/Users/alice2/work"
        );
        assert_eq!(
            shorten_home_with("/Users/alice/work", Some("/Users/alice/")),
            "~/work"
        );
        assert_eq!(
            shorten_home_with("/Users/alice", Some("/Users/alice/")),
            "~"
        );
        assert_eq!(
            shorten_home_with(r"C:\Users\alice\work", Some(r"C:\Users\alice")),
            r"~\work"
        );
        assert_eq!(
            shorten_home_with("/Users/alice/work", None),
            "/Users/alice/work"
        );
        assert_eq!(shorten_home_with("/work", Some("/")), "/work");
    }
}
