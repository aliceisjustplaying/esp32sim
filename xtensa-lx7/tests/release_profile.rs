const _: () = assert!(
    cfg!(debug_assertions),
    "all tested profiles must retain debug assertions"
);

#[test]
fn release_profile_keeps_debug_assertions_and_overflow_checks() {
    let overflow = std::panic::catch_unwind(|| std::hint::black_box(u32::MAX) + 1);
    assert!(
        overflow.is_err(),
        "all tested profiles must trap integer overflow"
    );
}
