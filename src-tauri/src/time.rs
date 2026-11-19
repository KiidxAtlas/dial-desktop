//! RFC 3339 timestamps (as Dial's session listing sends them) and relative labels.

#[cfg(test)]
mod tests;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Seconds since the Unix epoch for `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`, or a plain integer.
pub fn parse(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(seconds) = text.parse::<i64>() {
        return Some(seconds);
    }
    let b = text.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        rest = &frac[digits..];
    }
    let offset = match rest {
        "Z" | "z" | "" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hours = rest.get(1..3)?.parse::<i64>().ok()?;
            let minutes = rest.get(4..6)?.parse::<i64>().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
    };
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// (year, month, day) for days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A short label for `then` as seen at `now` (both Unix seconds, UTC).
pub fn relative(then: i64, now: i64) -> String {
    let ago = now - then;
    match ago {
        i64::MIN..60 => "just now".to_owned(),
        60..3600 => format!("{}m ago", ago / 60),
        3600..86_400 => format!("{}h ago", ago / 3600),
        86_400..172_800 => "yesterday".to_owned(),
        172_800..604_800 => format!("{}d ago", ago / 86_400),
        _ => {
            let (year, month, day) = civil_from_days(then.div_euclid(86_400));
            let (this_year, _, _) = civil_from_days(now.div_euclid(86_400));
            let name = MONTHS[usize::try_from(month - 1).unwrap_or(0)];
            if year == this_year {
                format!("{name} {day}")
            } else {
                format!("{name} {day}, {year}")
            }
        }
    }
}

/// Seconds since the Unix epoch now.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Relative label for a wire timestamp; the raw text when it does not parse.
pub fn label(text: &str, now: i64) -> String {
    parse(text).map_or_else(|| text.to_owned(), |then| relative(then, now))
}
