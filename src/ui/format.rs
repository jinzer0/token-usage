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

pub fn truncate_ellipsis(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let count = value.chars().count();
    if count <= max_width {
        return value.to_string();
    }
    if max_width == 1 {
        return "…".into();
    }
    let mut out = value.chars().take(max_width - 1).collect::<String>();
    out.push('…');
    out
}

pub fn pad_left(value: &str, width: usize) -> String {
    format!("{value:>width$}")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn truncates_long_names() {
        assert_eq!(truncate_ellipsis("abcdef", 4), "abc…");
        assert_eq!(truncate_ellipsis("abcdef", 6), "abcdef");
        assert_eq!(truncate_ellipsis("abcdef", 1), "…");
    }

    #[test]
    fn renders_usage_bar() {
        assert_eq!(usage_bar(50, 100, 10), "█████░░░░░");
        assert_eq!(usage_bar(0, 100, 4), "░░░░");
        assert_eq!(usage_bar(1, 1, 3), "███");
    }
}
