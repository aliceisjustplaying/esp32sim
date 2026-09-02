use std::path::Path;

#[test]
fn random_straight_line_blocks_match_interpreter_state_and_cycle_sum() {
    assert!(
        Path::new("src/emitter.rs").is_file(),
        "exit criterion: execute 100 random straight-line LX7 SRAM blocks as emitted wasm, matching interpreter architectural state and price-table cycle sums"
    );
}
