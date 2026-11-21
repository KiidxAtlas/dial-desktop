use super::{Budget, MAX_CAPTURE_BYTES, MAX_CONSECUTIVE_FAILURES, read_capped};

#[test]
fn a_file_at_the_cap_is_read_and_one_byte_over_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let at_cap = dir.path().join("at.ppm");
    let over = dir.path().join("over.ppm");
    std::fs::write(&at_cap, vec![0u8; 16]).unwrap();
    std::fs::write(&over, vec![0u8; 17]).unwrap();
    assert_eq!(read_capped(&at_cap, 16).unwrap().len(), 16);
    assert!(
        read_capped(&over, 16)
            .unwrap_err()
            .contains("larger than 16 bytes")
    );
}

#[test]
fn the_production_cap_is_64_mib() {
    assert_eq!(MAX_CAPTURE_BYTES, 64 * 1024 * 1024);
}

#[test]
fn a_missing_file_is_an_error_not_an_empty_frame() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_capped(&dir.path().join("none.ppm"), 16).is_err());
}

#[test]
fn consecutive_failures_exhaust_the_budget_and_a_success_resets_it() {
    let mut budget = Budget::default();
    for _ in 1..MAX_CONSECUTIVE_FAILURES {
        assert!(!budget.failed());
    }
    budget.succeeded();
    for _ in 1..MAX_CONSECUTIVE_FAILURES {
        assert!(!budget.failed());
    }
    assert!(
        budget.failed(),
        "the failure that reaches the limit reports exhaustion"
    );
}
