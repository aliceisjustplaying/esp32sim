use backend_api::*;

const PROFILE: &[u8] = include_bytes!("fixtures/timing-profile-v2.json");

fn hash(value: &str) -> [u8; 32] {
    let mut result = [0u8; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    result
}

fn receipt(
    path: &str,
    sha256: &str,
    firmware: &str,
    sdkconfig_sha256: &str,
    toolchain: &str,
    adoption_status: AdoptionStatus,
) -> ReceiptRef {
    ReceiptRef {
        repository: "https://github.com/aliceisjustplaying/esp32s3-cycle-accurate-wasm".into(),
        commit: "c64351e90602200691c5ec590b08f57bb68e34bb".into(),
        path: path.into(),
        sha256: hash(sha256),
        firmware: firmware.into(),
        sdkconfig_sha256: hash(sdkconfig_sha256),
        toolchain: toolchain.into(),
        board_revision: "Waveshare ESP32-S3-Touch-AMOLED-1.8, ESP32-S3 rev 0.2".into(),
        adoption_status,
    }
}

fn receipts() -> ReceiptManifest {
    let delta = |status| {
        receipt(
            "timing/evidence/idf61-rebaseline-3db3985/toolchain-delta.json",
            "d4a4d3547598ede01573b94b5da3fdd1258d3f4e8161778acb4fd0423ac8a654",
            "TinyDraw 3db39856f0a04266a42aef8cd5ead1be6fc8eca4",
            "20ba6a9133738c3ab131cf19679984580b3180f5f28704a7d164e5989e8fbe02",
            "ESP-IDF v6.1, xtensa-esp-elf 15.2.0",
            status,
        )
    };
    vec![
        delta(AdoptionStatus::Accepted),
        delta(AdoptionStatus::Candidate),
        receipt(
            "timing/evidence/esp32s3-rev02-tinydraw-a91d1d7-cache-burst-adoption.json",
            "c181adf14f60401efa974d3807aa1c5954294745455cb8520d205d269cfd487b",
            "TinyDraw a91d1d74af0ff4c1b55aebc3ed584e9074821394",
            "5befec96cb7e4dbd86a69abccf96828696b0c79cfe0b0c904d5bdc75543d3d68",
            "ESP-IDF v6.0.2",
            AdoptionStatus::Accepted,
        ),
        receipt(
            "timing/evidence/esp32s3-rev02-tinydraw-1ddd64b-4a2c659-hot-hit-adoption.json",
            "b8d872688aba5f7067a15bdfe7bec66beb6631155298fe38bf8a055f3cd4db57",
            "TinyDraw 1ddd64b and 4a2c659",
            "5befec96cb7e4dbd86a69abccf96828696b0c79cfe0b0c904d5bdc75543d3d68",
            "ESP-IDF v6.0.2",
            AdoptionStatus::Candidate,
        ),
        receipt(
            "timing/evidence/esp32s3-rev02-tinydraw-e8a9f0e-mmio-write-adoption.json",
            "ac04584f3a05931795d65dc7246ae556202dd98bb7304cce06f50b5a29b0dc8a",
            "TinyDraw e8a9f0e574f0e3f8902ae4c66585d43c9775a098",
            "5befec96cb7e4dbd86a69abccf96828696b0c79cfe0b0c904d5bdc75543d3d68",
            "ESP-IDF v6.0.2",
            AdoptionStatus::Accepted,
        ),
    ]
}

#[test]
fn schema_two_imports_exact_and_affine_claims() {
    let profile = import_timing_profile_v2(PROFILE, &receipts()).unwrap();
    assert_eq!(
        profile
            .exact_cycles_for(&CostBinding::BlockBase {
                class: "straight-line".into(),
            })
            .unwrap(),
        1
    );
    let mmio = CostBinding::Mmio {
        address: 0x600c_0060,
        width: 4,
        operation: "write".into(),
        peripheral: "system-controller".into(),
        write_effect: Some("same-value".into()),
    };
    assert_eq!(profile.affine_total_for(&mmio, 2048).unwrap(), 6136);
    assert_eq!(profile.affine_total_for(&mmio, 4096).unwrap(), 12280);
    assert_eq!(
        profile.exact_cycles_for(&mmio).unwrap_err().tier_candidate,
        "affine"
    );
}

#[test]
fn blocked_classes_remain_blocked_by_tier_and_adoption() {
    let profile = import_timing_profile_v2(PROFILE, &receipts()).unwrap();
    let first_line = CostBinding::Cache {
        cache: "instruction".into(),
        memory: "flash".into(),
        event: "first-line-fill".into(),
    };
    let block = profile.exact_cycles_for(&first_line).unwrap_err();
    assert_eq!(block.tier_candidate, "unexplained");
    assert_eq!(block.reason, "timing claim is not adopted");
}

#[test]
fn schema_one_and_receipt_mismatch_fail_closed() {
    let schema_one =
        br#"{"schemaVersion":1,"format":"esp32sim-timing-profile-v2","claims":[],"bindings":[]}"#;
    assert!(matches!(
        import_timing_profile_v2(schema_one, &ReceiptManifest::new()),
        Err(ProfileError::UnsupportedSchema(1))
    ));
    let mut receipts = receipts();
    receipts[0].sha256[0] ^= 1;
    assert!(matches!(
        import_timing_profile_v2(PROFILE, &receipts),
        Err(ProfileError::ReceiptMismatch { .. })
    ));
}

#[test]
fn every_trusted_receipt_field_is_validated() {
    type Mutator = fn(&mut ReceiptRef);
    let mutations: [Mutator; 9] = [
        |receipt| receipt.repository.push_str("/changed"),
        |receipt| receipt.commit.replace_range(..1, "0"),
        |receipt| receipt.path.push_str(".changed"),
        |receipt| receipt.sha256[0] ^= 1,
        |receipt| receipt.firmware.push_str(" changed"),
        |receipt| receipt.sdkconfig_sha256[0] ^= 1,
        |receipt| receipt.toolchain.push_str(" changed"),
        |receipt| receipt.board_revision.push_str(" changed"),
        |receipt| receipt.adoption_status = AdoptionStatus::Rejected,
    ];
    for mutate in mutations {
        let mut manifest = receipts();
        mutate(&mut manifest[0]);
        assert!(matches!(
            import_timing_profile_v2(PROFILE, &manifest),
            Err(ProfileError::ReceiptMismatch { .. })
        ));
    }
}

#[test]
fn canonical_ledger_covers_tier_causes_and_full_receipts() {
    let base_receipt = receipts()[0].clone();
    let entry = |tier, receipt| LedgerEntry {
        epoch: 1,
        cycle: 2,
        sequence: 3,
        kind: LedgerKind::InstructionCommit { pc: 0x4000_0400 },
        costs: vec![CostClaim {
            id: "claim".into(),
            tier,
            receipts: vec![receipt],
        }],
    };
    let original = canonical_ledger_bytes(&[entry(
        CostTier::Interval {
            minimum: 4,
            maximum: 8,
            cause: "understood cause".into(),
        },
        base_receipt.clone(),
    )]);
    let changed_cause = canonical_ledger_bytes(&[entry(
        CostTier::Interval {
            minimum: 4,
            maximum: 8,
            cause: "different cause".into(),
        },
        base_receipt.clone(),
    )]);
    assert_ne!(original, changed_cause);

    let mut changed_receipt = base_receipt.clone();
    changed_receipt.toolchain.push_str(" changed");
    let changed_receipt_bytes = canonical_ledger_bytes(&[entry(
        CostTier::Interval {
            minimum: 4,
            maximum: 8,
            cause: "understood cause".into(),
        },
        changed_receipt,
    )]);
    assert_ne!(original, changed_receipt_bytes);

    let additional_receipt = receipts()[2].clone();
    let with_additional = LedgerEntry {
        costs: vec![CostClaim {
            id: "claim".into(),
            tier: CostTier::Interval {
                minimum: 4,
                maximum: 8,
                cause: "understood cause".into(),
            },
            receipts: vec![base_receipt, additional_receipt],
        }],
        ..entry(
            CostTier::Unexplained {
                reason: "unused".into(),
            },
            receipts()[0].clone(),
        )
    };
    assert_ne!(original, canonical_ledger_bytes(&[with_additional]));
}

#[test]
fn canonical_ledger_carries_every_cost_component() {
    let entry = LedgerEntry {
        epoch: 1,
        cycle: 20,
        sequence: 4,
        kind: LedgerKind::InstructionStart {
            pc: 0x4200_0000,
            completion: 421,
        },
        costs: vec![test_claim("base", 1), test_claim("cache-fill", 400)],
    };
    let combined = canonical_ledger_bytes(&[entry.clone()]);
    let mut base_only = entry;
    base_only.costs.pop();
    assert_ne!(combined, canonical_ledger_bytes(&[base_only]));
}
