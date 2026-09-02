use backend_api::{
    price_operation, CacheFillPosition, CacheKind, ChipConfig, CoreId, CostTier, InstructionCost,
    MmioTier, Operation,
};
use sha2::{Digest, Sha256};

const OPCODES: &[u8] =
    include_bytes!("../../docs/evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json");
const REGISTERS: &[u8] =
    include_bytes!("../../docs/evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json");
const TOOLCHAIN: &[u8] =
    include_bytes!("../../docs/evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json");
const CACHE: &[u8] = include_bytes!(
    "../../docs/evidence/timing/esp32s3-rev02-tinydraw-a91d1d7-cache-burst-adoption.json"
);

fn pin(bytes: &[u8], sha256: &str) {
    assert_eq!(format!("{:x}", Sha256::digest(bytes)), sha256);
}

fn price(operation: Operation) -> backend_api::CostComponent {
    price_operation(ChipConfig::RECEIPT_SCOPE, CoreId::Core0, operation)
        .expect("adopted operation prices")
        .0
}

#[test]
fn opcode_rows_match_pinned_receipt() {
    pin(
        OPCODES,
        "db29ec42ccccc958c96153340497592ecc76203166a5a98c696bdd81496c6515",
    );
    for (kind, expected) in [
        (InstructionCost::Issue, 1),
        (InstructionCost::Branch { taken: false }, 1),
        (InstructionCost::Branch { taken: true }, 3),
        (InstructionCost::Jump, 3),
        (InstructionCost::JumpRegister, 6),
        (InstructionCost::LoopSetup, 5),
        (InstructionCost::Quotient, 4),
        (InstructionCost::Remainder, 5),
        (InstructionCost::AtomicStore, 6),
        (InstructionCost::LoadUse, 1),
    ] {
        assert_eq!(price(Operation::Instruction(kind)).cycles(), Some(expected));
    }
    for kind in [
        InstructionCost::LiteralLoad,
        InstructionCost::InstructionSync,
    ] {
        let refusal = price_operation(
            ChipConfig::RECEIPT_SCOPE,
            CoreId::Core0,
            Operation::Instruction(kind),
        )
        .expect_err("interval row has no scalar total");
        assert_eq!(refusal.tier_candidate, CostTier::Interval);
    }
}

#[test]
fn mmio_rows_match_pinned_receipt() {
    pin(
        REGISTERS,
        "67d213b0f823452582115e74d18f48f0e4142bfa8851f181388fac83c9245dd6",
    );
    for (tier, read, drain) in [
        (MmioTier::Fast, 9, 4),
        (MmioTier::Apb, 15, 15),
        (MmioTier::Nrx, 18, 0),
    ] {
        assert_eq!(price(Operation::MmioRead { tier }).cycles(), Some(read));
        if tier != MmioTier::Nrx {
            assert_eq!(
                price(Operation::MmioWrite {
                    tier,
                    buffer_has_room: false
                })
                .cycles(),
                Some(drain)
            );
        }
    }
    assert_eq!(
        price(Operation::MmioWrite {
            tier: MmioTier::Fast,
            buffer_has_room: true
        })
        .cycles(),
        Some(1)
    );
    for (tier, expected) in [
        (MmioTier::Nrx, CostTier::Interval),
        (MmioTier::Rtc, CostTier::Distribution),
        (MmioTier::Efuse, CostTier::Distribution),
    ] {
        let refusal = price_operation(
            ChipConfig::RECEIPT_SCOPE,
            CoreId::Core0,
            Operation::MmioWrite {
                tier,
                buffer_has_room: false,
            },
        )
        .expect_err("nonscalar write tier fails closed");
        assert_eq!(refusal.tier_candidate, expected);
    }
}

#[test]
fn cache_rows_match_pinned_receipts() {
    pin(
        TOOLCHAIN,
        "d4a4d3547598ede01573b94b5da3fdd1258d3f4e8161778acb4fd0423ac8a654",
    );
    pin(
        CACHE,
        "c181adf14f60401efa974d3807aa1c5954294745455cb8520d205d269cfd487b",
    );
    for (cache, first, subsequent) in [
        (CacheKind::InstructionFlash, 203, 266),
        (CacheKind::DataFlash, 114, 473),
        (CacheKind::DataPsram, 81, 170),
    ] {
        assert_eq!(
            price(Operation::CacheLineFill {
                cache,
                position: CacheFillPosition::First,
                line: 1
            })
            .cycles(),
            Some(first)
        );
        assert_eq!(
            price(Operation::CacheLineFill {
                cache,
                position: CacheFillPosition::Subsequent,
                line: 2
            })
            .cycles(),
            Some(subsequent)
        );
    }
    assert_eq!(
        price(Operation::LoopBackEdge { body_residue: 3 }).cycles(),
        Some(1)
    );
    assert_eq!(price(Operation::HotCacheHit).cycles(), Some(0));
    assert_eq!(price(Operation::IndependentSramAccess).cycles(), Some(0));
    assert_eq!(price(Operation::DmaAdditiveDelay).cycles(), Some(0));
}
