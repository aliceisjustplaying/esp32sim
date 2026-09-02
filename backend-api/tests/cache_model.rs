#[path = "../src/cache.rs"]
mod cache;

use std::{path::PathBuf, process::Command};

use cache::{
    AccessKind, AccessResult, CacheModel, CacheSource, CacheTarget, ChipConfig, FillPosition,
    ReplacementPolicy, UnsupportedChipConfig,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const RECEIPT_DIRECTORY: &str = "../docs/evidence/timing/idf61-rebaseline-3db3985/receipts";
const ARCHIVES: [(&str, &[u8], &str); 4] = [
    (
        "boot-1-recovered.tar.gz",
        include_bytes!(
            "../../docs/evidence/timing/idf61-rebaseline-3db3985/receipts/boot-1-recovered.tar.gz"
        ),
        "14764620c67975eb4a6383f78d921dbbbcca06acdb5117cf78164426c3ed6287",
    ),
    (
        "boot-2-recovered.tar.gz",
        include_bytes!(
            "../../docs/evidence/timing/idf61-rebaseline-3db3985/receipts/boot-2-recovered.tar.gz"
        ),
        "4d43cb919c0bceeb7bc5b8389249b2a532ddc820b9fc0df62bc63b0a415198a1",
    ),
    (
        "boot-3-recovered.tar.gz",
        include_bytes!(
            "../../docs/evidence/timing/idf61-rebaseline-3db3985/receipts/boot-3-recovered.tar.gz"
        ),
        "9eb40b6b8714873e84666d30850e1616b72fdbbc6d0d73f5c6e40cf750b25a8e",
    ),
    (
        "boot-4-recovered.tar.gz",
        include_bytes!(
            "../../docs/evidence/timing/idf61-rebaseline-3db3985/receipts/boot-4-recovered.tar.gz"
        ),
        "5ca8183e8416c235113576d6fd7eb413d34167ca86663a62a23dcaef66f049b1",
    ),
];

#[derive(Clone, Copy)]
struct BurstCase {
    stem: &'static str,
    kind: AccessKind,
    source: CacheSource,
    base: u32,
    line_bytes: u32,
}

const BURST_CASES: [BurstCase; 3] = [
    BurstCase {
        stem: "icache_flash",
        kind: AccessKind::Fetch,
        source: CacheSource::Flash,
        base: 0x4200_0000,
        line_bytes: 32,
    },
    BurstCase {
        stem: "dcache_flash",
        kind: AccessKind::Load,
        source: CacheSource::Flash,
        base: 0x3c00_0000,
        line_bytes: 64,
    },
    BurstCase {
        stem: "dcache_psram",
        kind: AccessKind::Load,
        source: CacheSource::Psram,
        base: 0x3d00_0000,
        line_bytes: 64,
    },
];

fn archive_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(RECEIPT_DIRECTORY)
        .join(name)
}

fn receipts(kernel: &str) -> Vec<Value> {
    let mut receipts = Vec::new();
    for (archive_name, bytes, expected_sha256) in ARCHIVES {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), expected_sha256);
        let output = Command::new("tar")
            .arg("-xOzf")
            .arg(archive_path(archive_name))
            .arg(format!("./{kernel}.json"))
            .output()
            .expect("tar must be available to replay committed receipt archives");
        if output.status.success() {
            receipts.push(
                serde_json::from_slice(&output.stdout)
                    .expect("recovered receipt must contain valid JSON"),
            );
        }
    }
    assert!(
        receipts.len() >= 2,
        "{kernel} must have two independent receipts"
    );
    receipts
}

fn receipt_misses(receipt: &Value, case: BurstCase) -> Vec<u64> {
    assert_eq!(receipt["toolchain"]["espIdfVersion"], "v6.1");
    assert_eq!(receipt["toolchain"]["compilerVersion"], "15.2.0");
    assert_eq!(receipt["sdkconfig"]["cpuHz"], 240_000_000);
    assert_eq!(receipt["sdkconfig"]["flashMode"], "qio");
    assert_eq!(receipt["sdkconfig"]["flashBusHz"], 80_000_000);
    assert_eq!(receipt["sdkconfig"]["psramMode"], "octal");
    assert_eq!(receipt["sdkconfig"]["psramBusHz"], 80_000_000);
    assert_eq!(receipt["boot"]["chipRevision"], 2);
    receipt["measurement"]["samples"]
        .as_array()
        .expect("receipt samples must be an array")
        .iter()
        .map(|sample| match (case.kind, case.source) {
            (AccessKind::Fetch, _) => sample["cacheCounters"]["ibus"]["misses"]
                .as_u64()
                .expect("I-cache miss count must be an integer"),
            (_, CacheSource::Flash) => sample["cacheCounters"]["dbus"]["flashMisses"]
                .as_u64()
                .expect("D-cache flash miss count must be an integer"),
            (_, CacheSource::Psram) => sample["cacheCounters"]["dbus"]["psramMisses"]
                .as_u64()
                .expect("D-cache PSRAM miss count must be an integer"),
        })
        .collect()
}

fn run_burst(case: BurstCase, lines: u32, hot: bool) -> Vec<AccessResult> {
    let mut model = CacheModel::new(ChipConfig::RECEIPT_SCOPE)
        .expect("receipt-scoped cache configuration must be supported");
    if hot {
        for line in 0..lines {
            let _result = model.access(case.kind, case.base + line * case.line_bytes);
        }
    }
    (0..lines)
        .map(|line| model.access(case.kind, case.base + line * case.line_bytes))
        .collect()
}

#[test]
fn receipt_bursts_have_expected_first_and_subsequent_misses() {
    for case in BURST_CASES {
        let receipt_line_counts: &[u32] = if case.kind == AccessKind::Fetch {
            &[1, 2, 4, 8]
        } else {
            &[1, 2, 4, 8, 16]
        };
        for &lines in receipt_line_counts {
            for hot in [false, true] {
                let temperature = if hot { "hot" } else { "cold" };
                let kernel = format!(
                    "{}_burst_{}_lines_{}_single_core",
                    case.stem, lines, temperature
                );
                let expected_misses = if hot { 0 } else { lines };
                for receipt in receipts(&kernel) {
                    assert_eq!(receipt["measurement"]["kernel"], kernel);
                    assert!(
                        receipt_misses(&receipt, case)
                            .iter()
                            .all(|misses| *misses == u64::from(expected_misses)),
                        "{kernel} cache counters disagree with its line count"
                    );
                }

                let first = run_burst(case, lines, hot);
                let second = run_burst(case, lines, hot);
                assert_eq!(first, second, "{kernel} replay must be deterministic");
                if hot {
                    assert!(first.iter().all(|result| *result == AccessResult::Hit));
                } else {
                    assert_eq!(
                        first[0],
                        AccessResult::Miss {
                            position: FillPosition::First,
                            source: case.source,
                        }
                    );
                    assert!(first[1..].iter().all(|result| {
                        *result
                            == AccessResult::Miss {
                                position: FillPosition::Subsequent,
                                source: case.source,
                            }
                    }));
                }
            }
        }
    }
}

#[test]
fn sixteen_line_icache_replay_is_model_only_because_the_receipt_is_missing() {
    let case = BURST_CASES[0];
    let cold = run_burst(case, 16, false);
    let hot = run_burst(case, 16, true);
    assert_eq!(cold, run_burst(case, 16, false));
    assert_eq!(hot, run_burst(case, 16, true));
    assert_eq!(
        cold[0],
        AccessResult::Miss {
            position: FillPosition::First,
            source: CacheSource::Flash,
        }
    );
    assert!(cold[1..].iter().all(|result| {
        *result
            == AccessResult::Miss {
                position: FillPosition::Subsequent,
                source: CacheSource::Flash,
            }
    }));
    assert!(hot.iter().all(|result| *result == AccessResult::Hit));
}

#[test]
fn receipt_hot_hit_cells_replay_as_hits() {
    for (case, kernel, accesses) in [
        (
            BURST_CASES[0],
            "icache_hit_flash_120_instructions_single_core",
            62,
        ),
        (BURST_CASES[1], "dcache_hit_flash_16_loads_single_core", 16),
        (BURST_CASES[2], "dcache_hit_psram_16_loads_single_core", 16),
    ] {
        for receipt in receipts(kernel) {
            assert!(
                receipt_misses(&receipt, case)
                    .iter()
                    .all(|misses| *misses == 0),
                "{kernel} must be a zero-miss receipt"
            );
        }
        let mut model = CacheModel::new(ChipConfig::RECEIPT_SCOPE)
            .expect("receipt-scoped cache configuration must be supported");
        let _cold = model.access(case.kind, case.base);
        let results: Vec<_> = (0..accesses)
            .map(|offset| model.access(case.kind, case.base + (offset * 4) % case.line_bytes))
            .collect();
        assert!(
            results.iter().all(|result| *result == AccessResult::Hit),
            "{kernel} model replay must stay hot"
        );
    }
}

#[test]
fn cache_maintenance_is_explicit_and_deterministic() {
    let mut model = CacheModel::new(ChipConfig::RECEIPT_SCOPE)
        .expect("receipt-scoped cache configuration must be supported");
    assert_eq!(
        model.access(AccessKind::Store, 0x3d00_0000),
        AccessResult::Miss {
            position: FillPosition::First,
            source: CacheSource::Psram,
        }
    );
    assert_eq!(model.writeback(0x3d00_0000, 64), 1);
    assert_eq!(model.writeback(0x3d00_0000, 64), 0);
    assert_eq!(
        model.access(AccessKind::Store, 0x3d00_0000),
        AccessResult::Hit
    );
    model.invalidate(CacheTarget::Data, 0x3d00_0000, 64);
    assert_eq!(model.writeback(0x3d00_0000, 64), 0);
    assert_eq!(
        model.access(AccessKind::Load, 0x3d00_0000),
        AccessResult::Miss {
            position: FillPosition::First,
            source: CacheSource::Psram,
        }
    );
    model.invalidate_all(CacheTarget::Instruction);
}

#[test]
fn policy_is_lru_and_unsupported_config_is_named() {
    let mut unsupported = ChipConfig::RECEIPT_SCOPE;
    unsupported.icache_line_bytes = 16;
    assert_eq!(
        CacheModel::new(unsupported),
        Err(UnsupportedChipConfig {
            configuration: unsupported,
        })
    );

    let mut model = CacheModel::new(ChipConfig::RECEIPT_SCOPE)
        .expect("receipt-scoped cache configuration must be supported");
    assert_eq!(
        model.replacement_policy(),
        ReplacementPolicy::LeastRecentlyUsed
    );
    let same_set_stride = ChipConfig::RECEIPT_SCOPE.icache_size_bytes
        / u32::from(ChipConfig::RECEIPT_SCOPE.icache_ways);
    for way in 0..u32::from(ChipConfig::RECEIPT_SCOPE.icache_ways) {
        let _miss = model.access(AccessKind::Fetch, 0x4200_0000 + way * same_set_stride);
    }
    assert_eq!(
        model.access(AccessKind::Fetch, 0x4200_0000),
        AccessResult::Hit
    );
    let _eviction = model.access(
        AccessKind::Fetch,
        0x4200_0000 + u32::from(ChipConfig::RECEIPT_SCOPE.icache_ways) * same_set_stride,
    );
    assert_eq!(
        model.access(AccessKind::Fetch, 0x4200_0000),
        AccessResult::Hit
    );
    assert!(matches!(
        model.access(AccessKind::Fetch, 0x4200_0000 + same_set_stride),
        AccessResult::Miss { .. }
    ));
}
