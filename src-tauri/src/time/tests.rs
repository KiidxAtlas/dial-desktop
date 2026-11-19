use super::*;

#[test]
fn parses_dial_session_timestamps() {
    // 2026-11-16T07:21:14Z, checked with `date -u -d @1794813674`.
    assert_eq!(parse("2026-11-16T07:21:14.449Z"), Some(1_794_813_674));
    assert_eq!(parse("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse("2000-02-29T12:00:00+02:00"), Some(951_818_400));
    assert_eq!(parse("1700000000"), Some(1_700_000_000));
    assert_eq!(parse("yesterday"), None);
    assert_eq!(parse("2026-13-01T00:00:00Z"), None);
}

#[test]
fn civil_dates_round_trip() {
    for days in [-1, 0, 59, 60, 365, 11_016, 20_408, 100_000] {
        let (y, m, d) = civil_from_days(days);
        assert_eq!(days_from_civil(y, m, d), days, "{days} -> {y}-{m}-{d}");
    }
}

#[test]
fn relative_labels_by_age() {
    let now = parse("2026-11-18T12:00:00Z").unwrap();
    assert_eq!(relative(now - 5, now), "just now");
    assert_eq!(relative(now + 30, now), "just now");
    assert_eq!(relative(now - 5 * 60, now), "5m ago");
    assert_eq!(relative(now - 3 * 3600, now), "3h ago");
    assert_eq!(relative(now - 30 * 3600, now), "yesterday");
    assert_eq!(relative(now - 3 * 86_400, now), "3d ago");
    assert_eq!(
        relative(parse("2026-10-02T09:00:00Z").unwrap(), now),
        "Oct 2"
    );
    assert_eq!(
        relative(parse("2025-12-31T09:00:00Z").unwrap(), now),
        "Dec 31, 2025"
    );
    assert_eq!(label("not a time", now), "not a time");
}
