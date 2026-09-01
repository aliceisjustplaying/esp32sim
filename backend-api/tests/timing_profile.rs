use backend_api::*;

const PROFILE: &[u8] = include_bytes!("fixtures/timing-profile-v2.json");

fn hash(value: &str) -> [u8; 32] {
    let mut result = [0u8; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    result
}

fn receipts() -> ReceiptManifest {
    [
        (
            "timing/evidence/idf61-rebaseline-3db3985/toolchain-delta.json".into(),
            hash("d4a4d3547598ede01573b94b5da3fdd1258d3f4e8161778acb4fd0423ac8a654"),
        ),
        (
            "timing/evidence/esp32s3-rev02-tinydraw-1ddd64b-4a2c659-hot-hit-adoption.json".into(),
            hash("b8d872688aba5f7067a15bdfe7bec66beb6631155298fe38bf8a055f3cd4db57"),
        ),
        (
            "timing/evidence/esp32s3-rev02-tinydraw-e8a9f0e-mmio-write-adoption.json".into(),
            hash("ac04584f3a05931795d65dc7246ae556202dd98bb7304cce06f50b5a29b0dc8a"),
        ),
    ]
    .into_iter()
    .collect()
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
    receipts.insert(
        "timing/evidence/idf61-rebaseline-3db3985/toolchain-delta.json".into(),
        [0; 32],
    );
    assert!(matches!(
        import_timing_profile_v2(PROFILE, &receipts),
        Err(ProfileError::ReceiptMismatch { .. })
    ));
}
