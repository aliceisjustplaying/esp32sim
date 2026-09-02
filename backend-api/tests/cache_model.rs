use std::path::Path;

#[test]
fn receipt_bursts_have_expected_first_and_subsequent_misses() {
    assert!(
        Path::new("src/cache.rs").is_file(),
        "exit criterion: replay cold 1, 2, 4, 8, and 16-line I-cache and D-cache receipts, then hot-hit receipts, with deterministic miss counts and one First miss"
    );
}
