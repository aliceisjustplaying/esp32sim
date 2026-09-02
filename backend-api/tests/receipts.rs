use backend_api::{
    price_operation, CacheFillPosition, CacheKind, CoreId, CostExpression, Operation, ReceiptId,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const BEQZ_RECEIPT: &[u8] =
    include_bytes!("../../docs/evidence/timing/esp32s3-rev02-tinydraw-2bf3ffd-beqz-adoption.json");
const MMIO_RECEIPT: &[u8] = include_bytes!(
    "../../docs/evidence/timing/esp32s3-rev02-tinydraw-e8a9f0e-mmio-write-adoption.json"
);
const CACHE_RECEIPT: &[u8] = include_bytes!(
    "../../docs/evidence/timing/esp32s3-rev02-tinydraw-a91d1d7-cache-burst-adoption.json"
);
const TOOLCHAIN_DELTA: &[u8] =
    include_bytes!("../../docs/evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json");

fn receipt(bytes: &[u8], expected_sha256: &str) -> Value {
    assert_eq!(format!("{:x}", Sha256::digest(bytes)), expected_sha256);
    serde_json::from_slice(bytes).expect("committed receipt must be valid JSON")
}

fn cycles(operation: Operation) -> u64 {
    price_operation(CoreId::Core0, operation)
        .expect("receipt-backed operation prices")
        .0
        .cycles()
        .expect("adopted expression is nonnegative")
}

#[test]
fn branch_receipt_and_model_assert_three_and_one_exactly() {
    let receipt = receipt(
        BEQZ_RECEIPT,
        "335326d061acb0fe7465cfaa596bd77eb064ebd8b08643e2339a7749af781095",
    );
    assert_eq!(receipt["status"], "adopted-exact-beqz-path-cycles");
    assert_eq!(
        receipt["matchedResults"]["adoptedBeqzCpuCycles"]["notTaken"],
        1
    );
    assert_eq!(
        receipt["matchedResults"]["adoptedBeqzCpuCycles"]["taken"],
        3
    );
    let taken = price_operation(CoreId::Core0, Operation::BranchZero { taken: true })
        .expect("taken branch is adopted")
        .0;
    let not_taken = price_operation(CoreId::Core0, Operation::BranchZero { taken: false })
        .expect("not-taken branch is adopted")
        .0;
    assert_eq!(taken.receipt, ReceiptId::BeqzAdoption2bf3ffd);
    assert_eq!(taken.cycles(), Some(3));
    assert_eq!(not_taken.cycles(), Some(1));
}

#[test]
fn mmio_receipt_and_model_assert_three_n_minus_eight_exactly() {
    let receipt = receipt(
        MMIO_RECEIPT,
        "ac04584f3a05931795d65dc7246ae556202dd98bb7304cce06f50b5a29b0dc8a",
    );
    assert_eq!(receipt["matchedResults"]["affineSlopeCyclesPerAccess"], 3);
    assert_eq!(receipt["matchedResults"]["affineInterceptCycles"], -8);
    assert_eq!(
        receipt["matchedResults"]["additiveDeltaCycles"]["2048"],
        6136
    );
    assert_eq!(
        receipt["matchedResults"]["additiveDeltaCycles"]["4096"],
        12280
    );
    for count in [3, 4, 16, 4096] {
        let component = price_operation(
            CoreId::Core0,
            Operation::SameValueMmioWriteRun {
                address: 0x600c_001c,
                value: 1,
                count,
            },
        )
        .expect("receipt domain prices")
        .0;
        assert_eq!(component.receipt, ReceiptId::MmioWriteAdoptionE8a9f0e);
        assert_eq!(
            component.expression,
            CostExpression::Affine {
                slope: 3,
                intercept: -8,
                count,
            }
        );
        assert_eq!(component.cycles(), Some(u64::from(3 * count - 8)));
    }
}

#[test]
fn subsequent_fill_receipt_and_model_assert_adopted_values_exactly() {
    let receipt = receipt(
        CACHE_RECEIPT,
        "c181adf14f60401efa974d3807aa1c5954294745455cb8520d205d269cfd487b",
    );
    assert_eq!(receipt["status"], "adopted-cache-line-fill-costs");
    assert_eq!(
        receipt["costs"]["instruction"]["flash"]["subsequentLineCycles"],
        266
    );
    assert_eq!(
        receipt["costs"]["data"]["flash"]["subsequentLineCycles"],
        473
    );
    assert_eq!(
        receipt["costs"]["data"]["psram"]["subsequentLineCycles"],
        170
    );
    for (cache, expected) in [
        (CacheKind::InstructionFlash, 266),
        (CacheKind::DataFlash, 473),
        (CacheKind::DataPsram, 170),
    ] {
        assert_eq!(
            cycles(Operation::CacheLineFill {
                cache,
                position: CacheFillPosition::Subsequent,
                line: 1,
            }),
            expected
        );
    }
}

#[test]
fn loop_receipt_and_model_assert_adopted_value_exactly() {
    let receipt = receipt(
        TOOLCHAIN_DELTA,
        "d4a4d3547598ede01573b94b5da3fdd1258d3f4e8161778acb4fd0423ac8a654",
    );
    assert_eq!(
        receipt["siliconArchitectural"]["windowOverflowUnderflowPairCyclesPastDepth6"],
        35
    );
    assert_eq!(
        receipt["siliconArchitectural"]["loopResiduePlus3AdditionalCyclesPerIteration"],
        1
    );
    assert_eq!(receipt["cacheProbeDiagnostic"]["adopted"], false);
    for residue in 0..=3 {
        assert_eq!(
            cycles(Operation::LoopBackEdge {
                body_residue: residue,
            }),
            u64::from(residue == 3)
        );
    }
}
