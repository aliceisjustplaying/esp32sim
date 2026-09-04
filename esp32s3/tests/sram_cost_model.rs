use emu_core::{Bus, Core};
use esp32s3::{CostClass, Esp32S3SramCostModel, ReceiptId};
use esp_soc::Stop;

const PC: u32 = 0x4038_645b;
const BASE: u32 = 0x3fc8_9000;
const POINTER: u32 = 0x3fc8_9100;
const PROGRAM: &[u8] = include_bytes!("fixtures/tinydraw-sram-kernel.bin");
const RECEIPT: &str = include_str!("fixtures/sram-cost-receipt.json");

fn configured_machine() -> (esp32s3::Machine, Esp32S3SramCostModel) {
    let mut machine = esp32s3::machine([0; 6]);
    esp_soc::SocBus::load_bytes(&mut machine.bus, PC, PROGRAM).unwrap();
    machine.bus.write32(BASE + 4, POINTER).unwrap();
    machine.bus.write32(POINTER + 0x1c4, 0x1234_5678).unwrap();
    machine.cores[0].set_pc(PC);
    machine.cores[0].set_ar(2, BASE);
    machine.cores[0].set_ar(3, 7);
    let model = Esp32S3SramCostModel::new();
    machine.set_cost_model(Box::new(model.clone())).unwrap();
    (machine, model)
}

#[test]
fn prices_the_receipted_tinydraw_sram_sequence_deterministically() {
    assert!(RECEIPT.contains("\"straightLineIssueCyclesPerInstruction\": 1"));
    assert!(RECEIPT.contains("\"independentAlignedSramAccessAdditiveCycles\": 0"));
    assert!(RECEIPT.contains("d21db307c7be78ffa0bb82a3e1446219bc0eac22cd557fc633e28fd16cadaa8e"));
    let (mut first_machine, first_model) = configured_machine();
    assert!(matches!(first_machine.run(7), Stop::MaxInsns));
    let first = first_model.ledger();

    assert_eq!(
        first.iter().map(|entry| entry.pc).collect::<Vec<_>>(),
        vec![PC, PC + 2, PC + 4, PC + 7, PC + 10, PC + 13, PC + 16,]
    );
    assert_eq!(first.iter().map(|entry| entry.cycles).sum::<u32>(), 7);
    assert!(first
        .iter()
        .all(|entry| entry.core == 0 && entry.cycles == 1));
    let sram_components = first
        .iter()
        .flat_map(|entry| &entry.components)
        .filter(|component| component.class == CostClass::IndependentSramAccess)
        .collect::<Vec<_>>();
    assert_eq!(sram_components.len(), 2);
    assert!(sram_components.iter().all(|component| {
        component.cycles == 0
            && component.receipt == ReceiptId::Idf61IndependentSramAccess
            && component.receipt.file() == "esp32s3/tests/fixtures/sram-cost-receipt.json"
    }));

    let (mut second_machine, second_model) = configured_machine();
    assert!(matches!(second_machine.run(7), Stop::MaxInsns));
    assert_eq!(second_model.ledger(), first);

    let stop = first_machine.run(1);
    assert!(
        matches!(
        stop,
        Stop::CostModel { core: 0, pc, ref reason }
            if pc == PC + 19 && reason.contains("ControlFlow(Bltu)")
        ),
        "{stop:?}"
    );
    assert_eq!(first_model.ledger(), first);
}

#[test]
fn refuses_non_sram_data_without_committing_a_ledger_entry() {
    let (mut machine, model) = configured_machine();
    machine.cores[0].set_ar(2, 0x600f_e000);
    let stop = machine.run(1);
    assert!(
        matches!(
            stop,
            Stop::CostModel { ref reason, .. } if reason.contains("non-SRAM cost not adopted")
        ),
        "{stop:?}"
    );
    assert!(model.ledger().is_empty());
}

#[test]
fn refuses_an_unpriced_load_use_dependency() {
    const DEPENDENT_PC: u32 = 0x4037_1000;
    const DEPENDENT_PROGRAM: &[u8] = &[0xa8, 0x12, 0xa0, 0x88, 0xc0];
    let mut machine = esp32s3::machine([0; 6]);
    esp_soc::SocBus::load_bytes(&mut machine.bus, DEPENDENT_PC, DEPENDENT_PROGRAM).unwrap();
    machine.bus.write32(BASE + 4, 0x1234_5678).unwrap();
    machine.cores[0].set_pc(DEPENDENT_PC);
    machine.cores[0].set_ar(2, BASE);
    machine.cores[0].set_ar(3, 7);
    let model = Esp32S3SramCostModel::new();
    machine.set_cost_model(Box::new(model.clone())).unwrap();

    assert!(matches!(machine.run(1), Stop::MaxInsns));
    let stop = machine.run(1);
    assert!(
        matches!(stop, Stop::CostModel { ref reason, .. } if reason.contains("LoadUse(Sub)")),
        "{stop:?}"
    );
    assert_eq!(model.ledger().len(), 1);
}
