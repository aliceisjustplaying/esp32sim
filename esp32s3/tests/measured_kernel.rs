use std::path::Path;

#[test]
fn tinydraw_sram_kernel_has_deterministic_receipt_priced_ledger() {
    assert!(
        Path::new("../tests/correlation/tinydraw-sram-kernel-ledger.json").is_file(),
        "exit criterion: step a SHA-256-pinned TinyDraw SRAM kernel through Machine::step_measured, price or name every instruction refusal, and reproduce the committed ledger byte for byte twice"
    );
}
