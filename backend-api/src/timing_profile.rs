use crate::{AdoptionStatus, CostClaim, CostTier, ReceiptRef, TimingBlock};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub type ReceiptManifest = BTreeMap<String, [u8; 32]>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CostBinding {
    BlockBase {
        class: String,
    },
    Cache {
        cache: String,
        memory: String,
        event: String,
    },
    WindowExceptionPair {
        minimum_depth: u32,
    },
    LoopAlignment {
        residue_mod_4: u8,
    },
    DependentLoadUse {
        producer: String,
        consumer: String,
    },
    Mmio {
        address: u32,
        width: u8,
        operation: String,
        peripheral: String,
        write_effect: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub struct ImportedTimingProfile {
    pub profile_sha256: [u8; 32],
    pub claims: BTreeMap<String, CostClaim>,
    pub bindings: BTreeMap<CostBinding, String>,
}

impl ImportedTimingProfile {
    pub fn claim_for(&self, binding: &CostBinding) -> Result<&CostClaim, TimingBlock> {
        let claim_id = self.bindings.get(binding).ok_or_else(|| TimingBlock {
            claim_id: format!("unbound:{binding:?}"),
            tier_candidate: "unexplained".into(),
            reason: "timing profile has no claim for the exact match key".into(),
        })?;
        let claim = self.claims.get(claim_id).ok_or_else(|| TimingBlock {
            claim_id: claim_id.clone(),
            tier_candidate: "unexplained".into(),
            reason: "timing profile binding references a missing claim".into(),
        })?;
        if claim.receipt.adoption_status != AdoptionStatus::Accepted {
            return Err(TimingBlock {
                claim_id: claim.id.clone(),
                tier_candidate: claim.tier.candidate_name().into(),
                reason: "timing claim is not adopted".into(),
            });
        }
        Ok(claim)
    }

    pub fn exact_cycles_for(&self, binding: &CostBinding) -> Result<u64, TimingBlock> {
        let claim = self.claim_for(binding)?;
        match claim.tier {
            CostTier::Exact { cycles } => Ok(cycles),
            _ => Err(TimingBlock {
                claim_id: claim.id.clone(),
                tier_candidate: claim.tier.candidate_name().into(),
                reason: "claim does not provide an exact online event duration".into(),
            }),
        }
    }

    pub fn affine_total_for(&self, binding: &CostBinding, count: u64) -> Result<i128, TimingBlock> {
        let claim = self.claim_for(binding)?;
        let CostTier::Affine {
            slope,
            intercept,
            minimum_count,
            maximum_count,
        } = claim.tier
        else {
            return Err(TimingBlock {
                claim_id: claim.id.clone(),
                tier_candidate: claim.tier.candidate_name().into(),
                reason: "claim is not affine".into(),
            });
        };
        if count < minimum_count || count > maximum_count {
            return Err(TimingBlock {
                claim_id: claim.id.clone(),
                tier_candidate: "affine".into(),
                reason: format!(
                    "cohort count {count} is outside evidenced bounds {minimum_count}..={maximum_count}"
                ),
            });
        }
        Ok(i128::from(slope) * i128::from(count) + i128::from(intercept))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    Json(String),
    UnsupportedSchema(u64),
    Invalid(String),
    ReceiptMismatch { path: String },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawProfile {
    schema_version: u64,
    format: String,
    claims: Vec<RawClaim>,
    bindings: Vec<RawBinding>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawClaim {
    id: String,
    #[serde(flatten)]
    tier: RawTier,
    receipt: RawReceipt,
}

#[derive(Deserialize)]
#[serde(
    tag = "tier",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum RawTier {
    Exact {
        cycles: u64,
    },
    Affine {
        slope: i64,
        intercept: i64,
        minimum_count: u64,
        maximum_count: u64,
    },
    Interval {
        minimum: u64,
        maximum: u64,
        cause: String,
    },
    Distribution {
        minimum: u64,
        median: u64,
        maximum: u64,
        samples: u64,
        boots: u32,
        cause: String,
    },
    Unexplained {
        reason: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawReceipt {
    repository: String,
    commit: String,
    path: String,
    sha256: String,
    firmware: String,
    sdkconfig_sha256: String,
    toolchain: String,
    board_revision: String,
    adoption_status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBinding {
    claim_id: String,
    #[serde(flatten)]
    class: RawBindingClass,
}

#[derive(Deserialize)]
#[serde(
    tag = "class",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum RawBindingClass {
    BlockBase {
        block_class: String,
    },
    Cache {
        cache: String,
        memory: String,
        event: String,
    },
    WindowExceptionPair {
        minimum_depth: u32,
    },
    LoopAlignment {
        residue_mod_4: u8,
    },
    DependentLoadUse {
        producer: String,
        consumer: String,
    },
    Mmio {
        address: String,
        width: u8,
        operation: String,
        peripheral: String,
        write_effect: Option<String>,
    },
}

fn nonempty(value: String, path: &str) -> Result<String, ProfileError> {
    if value.is_empty() || value.as_bytes().contains(&0) {
        return Err(ProfileError::Invalid(format!(
            "{path} must be nonempty and NUL-free"
        )));
    }
    Ok(value)
}

fn hash(value: &str, path: &str) -> Result<[u8; 32], ProfileError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ProfileError::Invalid(format!(
            "{path} must be 64 lowercase hexadecimal bytes"
        )));
    }
    let mut result = [0u8; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| ProfileError::Invalid(format!("{path} is not hexadecimal")))?;
    }
    Ok(result)
}

fn address(value: &str) -> Result<u32, ProfileError> {
    let digits = value
        .strip_prefix("0x")
        .ok_or_else(|| ProfileError::Invalid("MMIO address must start with 0x".into()))?;
    if digits.len() != 8
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ProfileError::Invalid(
            "MMIO address must have eight lowercase hexadecimal digits".into(),
        ));
    }
    u32::from_str_radix(digits, 16)
        .map_err(|_| ProfileError::Invalid("MMIO address is invalid".into()))
}

pub fn import_timing_profile_v2(
    bytes: &[u8],
    receipts: &ReceiptManifest,
) -> Result<ImportedTimingProfile, ProfileError> {
    let raw: RawProfile =
        serde_json::from_slice(bytes).map_err(|error| ProfileError::Json(error.to_string()))?;
    if raw.schema_version != 2 {
        return Err(ProfileError::UnsupportedSchema(raw.schema_version));
    }
    if raw.format != "esp32sim-timing-profile-v2" {
        return Err(ProfileError::Invalid(
            "timing profile format is not esp32sim-timing-profile-v2".into(),
        ));
    }
    let mut claims = BTreeMap::new();
    for (index, raw_claim) in raw.claims.into_iter().enumerate() {
        let id = nonempty(raw_claim.id, &format!("claims[{index}].id"))?;
        if claims.contains_key(&id) {
            return Err(ProfileError::Invalid(format!("duplicate claim id {id}")));
        }
        let tier = match raw_claim.tier {
            RawTier::Exact { cycles } => CostTier::Exact { cycles },
            RawTier::Affine {
                slope,
                intercept,
                minimum_count,
                maximum_count,
            } => {
                if slope < 0 || minimum_count == 0 || minimum_count > maximum_count {
                    return Err(ProfileError::Invalid(format!(
                        "claim {id} has invalid affine bounds"
                    )));
                }
                CostTier::Affine {
                    slope,
                    intercept,
                    minimum_count,
                    maximum_count,
                }
            }
            RawTier::Interval {
                minimum,
                maximum,
                cause,
            } => {
                nonempty(cause, &format!("claim {id} interval cause"))?;
                if minimum > maximum {
                    return Err(ProfileError::Invalid(format!(
                        "claim {id} interval is reversed"
                    )));
                }
                CostTier::Interval { minimum, maximum }
            }
            RawTier::Distribution {
                minimum,
                median,
                maximum,
                samples,
                boots,
                cause,
            } => {
                nonempty(cause, &format!("claim {id} distribution cause"))?;
                if minimum > median || median > maximum || samples == 0 || boots == 0 {
                    return Err(ProfileError::Invalid(format!(
                        "claim {id} distribution is invalid"
                    )));
                }
                CostTier::Distribution {
                    minimum,
                    median,
                    maximum,
                    samples,
                    boots,
                }
            }
            RawTier::Unexplained { reason } => {
                nonempty(reason, &format!("claim {id} unexplained reason"))?;
                CostTier::Unexplained
            }
        };
        let receipt_hash = hash(
            &raw_claim.receipt.sha256,
            &format!("claim {id} receipt sha256"),
        )?;
        let manifest_hash =
            receipts
                .get(&raw_claim.receipt.path)
                .ok_or_else(|| ProfileError::ReceiptMismatch {
                    path: raw_claim.receipt.path.clone(),
                })?;
        if *manifest_hash != receipt_hash {
            return Err(ProfileError::ReceiptMismatch {
                path: raw_claim.receipt.path,
            });
        }
        let adoption_status = match raw_claim.receipt.adoption_status.as_str() {
            "accepted" => AdoptionStatus::Accepted,
            "candidate" => AdoptionStatus::Candidate,
            "rejected" => AdoptionStatus::Rejected,
            _ => {
                return Err(ProfileError::Invalid(format!(
                    "claim {id} has invalid adoption status"
                )))
            }
        };
        let receipt = ReceiptRef {
            repository: nonempty(
                raw_claim.receipt.repository,
                &format!("claim {id} repository"),
            )?,
            commit: nonempty(raw_claim.receipt.commit, &format!("claim {id} commit"))?,
            path: nonempty(raw_claim.receipt.path, &format!("claim {id} path"))?,
            sha256: receipt_hash,
            firmware: nonempty(raw_claim.receipt.firmware, &format!("claim {id} firmware"))?,
            sdkconfig_sha256: hash(
                &raw_claim.receipt.sdkconfig_sha256,
                &format!("claim {id} sdkconfig sha256"),
            )?,
            toolchain: nonempty(
                raw_claim.receipt.toolchain,
                &format!("claim {id} toolchain"),
            )?,
            board_revision: nonempty(
                raw_claim.receipt.board_revision,
                &format!("claim {id} board revision"),
            )?,
            adoption_status,
        };
        claims.insert(id.clone(), CostClaim { id, tier, receipt });
    }

    let mut bindings = BTreeMap::new();
    let mut used_claims = BTreeSet::new();
    for raw_binding in raw.bindings {
        if !claims.contains_key(&raw_binding.claim_id) {
            return Err(ProfileError::Invalid(format!(
                "binding references missing claim {}",
                raw_binding.claim_id
            )));
        }
        let binding = match raw_binding.class {
            RawBindingClass::BlockBase { block_class } => CostBinding::BlockBase {
                class: nonempty(block_class, "block base class")?,
            },
            RawBindingClass::Cache {
                cache,
                memory,
                event,
            } => CostBinding::Cache {
                cache: nonempty(cache, "cache kind")?,
                memory: nonempty(memory, "cache memory")?,
                event: nonempty(event, "cache event")?,
            },
            RawBindingClass::WindowExceptionPair { minimum_depth } => {
                CostBinding::WindowExceptionPair { minimum_depth }
            }
            RawBindingClass::LoopAlignment { residue_mod_4 } => {
                if residue_mod_4 > 3 {
                    return Err(ProfileError::Invalid(
                        "loop residue must be in 0..=3".into(),
                    ));
                }
                CostBinding::LoopAlignment { residue_mod_4 }
            }
            RawBindingClass::DependentLoadUse { producer, consumer } => {
                CostBinding::DependentLoadUse {
                    producer: nonempty(producer, "dependent producer")?,
                    consumer: nonempty(consumer, "dependent consumer")?,
                }
            }
            RawBindingClass::Mmio {
                address: raw_address,
                width,
                operation,
                peripheral,
                write_effect,
            } => {
                if !matches!(width, 1 | 2 | 4) {
                    return Err(ProfileError::Invalid(
                        "MMIO width must be 1, 2, or 4".into(),
                    ));
                }
                CostBinding::Mmio {
                    address: address(&raw_address)?,
                    width,
                    operation: nonempty(operation, "MMIO operation")?,
                    peripheral: nonempty(peripheral, "MMIO peripheral")?,
                    write_effect: write_effect
                        .map(|value| nonempty(value, "MMIO write effect"))
                        .transpose()?,
                }
            }
        };
        if bindings
            .insert(binding, raw_binding.claim_id.clone())
            .is_some()
        {
            return Err(ProfileError::Invalid("duplicate timing match key".into()));
        }
        used_claims.insert(raw_binding.claim_id);
    }
    if used_claims.len() != claims.len() {
        return Err(ProfileError::Invalid(
            "every timing claim must have a binding".into(),
        ));
    }
    Ok(ImportedTimingProfile {
        profile_sha256: Sha256::digest(bytes).into(),
        claims,
        bindings,
    })
}
