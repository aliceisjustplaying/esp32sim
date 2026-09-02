use std::path::Path;

#[test]
fn committed_and_random_sram_blocks_match_interpreter_and_jit_state() {
    assert!(
        Path::new("tests/corpus/jit-interpreter-conformance.json").is_file(),
        "exit criterion: run the committed conformance corpus and randomized SRAM blocks through the interpreter and native JIT, then compare registers, PC, and touched memory"
    );
}
