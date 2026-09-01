use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const BUNDLE: &[u8] = include_bytes!("fixtures/flexe-boot-shared-replay-v1.json");
const ELF_SHA256: &str = "4e121a3642a6f18766cfe96c2be6adc8a0017fba4afa82105d642168ea40e2c8";
const TRACE_SHA256: &str = "e025823c09a8c5558dbd2147bde05e8396c6d0f4b6103ca77947d11b4bc27d2d";
const PROJECTION_SHA256: &str = "14ab04cff1d550fc83f2970a182d95c5c14205c1a4a12e95162daad2b1558b17";
const TIMING_PROFILE_SHA256: &str =
    "31a83ab4fe2253ef7ff5a0bcc944aa5c9ca38f90eef485f48f8f725fd790402a";
const MMIO_UNKNOWN_REASON: &str = "no exact matched boot-controller MMIO access receipt has no adopted cycle cost in the ESP32-S3 timing profile; source: timing profile packs/esp32-s3-touch-amoled-18/timing.json SHA-256 31a83ab4fe2253ef7ff5a0bcc944aa5c9ca38f90eef485f48f8f725fd790402a";
const SRAM_PAGES: &[u32] = &[
    0x3fc9_d000,
    0x3fca_0000,
    0x3fca_1000,
    0x3fce_9000,
    0x4037_4000,
    0x4037_5000,
    0x4037_6000,
    0x4037_7000,
    0x4037_c000,
    0x4037_d000,
    0x4037_f000,
    0x4038_0000,
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bundle {
    schema_version: u32,
    format: String,
    inputs: Inputs,
    trace: Trace,
    rom_callbacks: Vec<RomCallback>,
    typescript: TypeScriptReplay,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inputs {
    elf_sha256: String,
    trace_record_sha256: String,
    timing_profile_sha256: String,
}

#[derive(Deserialize)]
struct Trace {
    encoding: String,
    records: Vec<TraceRecord>,
}

#[derive(Clone, Copy, Deserialize)]
struct TraceRecord {
    kind: u32,
    pc: u32,
    address: u32,
    value: u32,
    width: u32,
    instruction: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RomCallback {
    kind: String,
    after_instruction_count: usize,
    cpu_cost: Cost,
}

#[derive(Deserialize)]
struct TypeScriptReplay {
    replay: ReplaySummary,
    #[serde(rename = "issuedProjection")]
    issued_projection: Vec<IssuedEvent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaySummary {
    status: String,
    issued_events: usize,
    memory_events: usize,
    mmio_events: usize,
    cpu_events: usize,
    rom_callback_events: usize,
    dependent_sram_load_use_hazards: usize,
    known_cost_events: usize,
    unknown_cost_events: usize,
    issued_projection_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct IssuedEvent {
    issue_index: usize,
    event_id: String,
    event_kind: String,
    origin_kind: String,
    cost: Cost,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Cost {
    Known { cycles: String, calibration: String },
    Unknown { reason: String },
}

impl Cost {
    fn known(cycles: u32) -> Self {
        Self::Known {
            cycles: cycles.to_string(),
            calibration: "calibrated".into(),
        }
    }
}

#[derive(Clone)]
struct InstructionGroup {
    record_index: usize,
    instruction: TraceRecord,
    data: Vec<(usize, TraceRecord)>,
}

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn trace_sha256(records: &[TraceRecord]) -> String {
    let mut bytes = Vec::with_capacity(records.len() * 24);
    for record in records {
        bytes.extend_from_slice(&record.kind.to_le_bytes());
        bytes.extend_from_slice(&record.pc.to_le_bytes());
        bytes.extend_from_slice(&record.address.to_le_bytes());
        bytes.extend_from_slice(&record.value.to_le_bytes());
        bytes.extend_from_slice(&record.width.to_le_bytes());
        bytes.extend_from_slice(&record.instruction.to_le_bytes());
    }
    sha256(bytes)
}

fn instruction_groups(records: &[TraceRecord]) -> Vec<InstructionGroup> {
    let mut groups = Vec::new();
    for (record_index, record) in records.iter().copied().enumerate() {
        match record.kind {
            1 => groups.push(InstructionGroup {
                record_index,
                instruction: record,
                data: Vec::new(),
            }),
            2 | 3 => {
                let group = groups
                    .last_mut()
                    .expect("data record has an instruction owner");
                assert_eq!(record.pc, group.instruction.pc);
                assert_eq!(record.instruction, 0);
                group.data.push((record_index, record));
            }
            _ => panic!("unsupported trace kind {}", record.kind),
        }
    }
    groups
}

fn exact_load_destination(instruction: TraceRecord, data_width: u32) -> Option<u8> {
    let encoding = instruction.instruction;
    let op0 = encoding & 0xf;
    let t = ((encoding >> 4) & 0xf) as u8;
    if instruction.width == 4 {
        return None;
    }
    if instruction.width == 2 && op0 == 8 {
        assert_eq!(data_width, 4);
        return Some(t);
    }
    if instruction.width == 3 && op0 == 1 {
        assert_eq!(data_width, 4);
        return Some(t);
    }
    if instruction.width == 3 && op0 == 2 {
        let r = (encoding >> 12) & 0xf;
        let width = match r {
            0 => 1,
            1 | 9 => 2,
            2 | 11 | 14 => 4,
            _ => panic!("unsupported scalar load at trace PC {:08x}", instruction.pc),
        };
        assert_eq!(data_width, width);
        return Some(t);
    }
    panic!("unsupported load at trace PC {:08x}", instruction.pc)
}

fn exact_source_registers(instruction: TraceRecord) -> Vec<u8> {
    let encoding = instruction.instruction;
    let op0 = encoding & 0xf;
    let t = ((encoding >> 4) & 0xf) as u8;
    let s = ((encoding >> 8) & 0xf) as u8;
    let r = ((encoding >> 12) & 0xf) as u8;
    if instruction.width == 4 {
        let opcode = encoding & 0xf800_000e;
        assert!([0x8000_000e, 0x8800_000e, 0x9000_000e, 0x9800_000e].contains(&opcode));
        let base = ((encoding >> 4) & 0xf) as u8;
        if opcode != 0x8800_000e && opcode != 0x9800_000e {
            return vec![base];
        }
        let increment = ((encoding >> 8) & 0xf) as u8;
        return if base == increment {
            vec![base]
        } else {
            vec![base, increment]
        };
    }
    if instruction.width == 2 {
        return match op0 {
            8 | 11 => vec![s],
            9 | 10 => vec![s, t],
            12 if ((t >> 2) & 3) < 2 => vec![],
            12 => vec![s],
            13 if r == 0 => vec![s],
            13 if r == 15 && s == 0 && t == 3 => vec![],
            _ => panic!("unsupported narrow register use at {:08x}", instruction.pc),
        };
    }
    if instruction.width == 3 && op0 == 2 {
        return match r {
            0 | 1 | 2 | 9 | 11 | 12 | 13 => vec![s],
            4 | 5 | 6 | 14 | 15 => vec![s, t],
            10 => vec![],
            _ => panic!("unsupported LSAI register use at {:08x}", instruction.pc),
        };
    }
    if instruction.width == 3 && (op0 == 1 || op0 == 5) {
        return vec![];
    }
    if instruction.width == 3 && op0 == 6 {
        let n = ((encoding >> 4) & 3) as u8;
        let m = ((encoding >> 6) & 3) as u8;
        if n == 0 {
            return vec![];
        }
        if n == 1 || n == 2 || m == 0 || m == 2 || m == 3 {
            return vec![s];
        }
        if n == 3 && m == 1 {
            return match r {
                0 | 1 => vec![],
                8 | 9 | 10 => vec![s],
                _ => panic!("unsupported B1 register use at {:08x}", instruction.pc),
            };
        }
    }
    if instruction.width == 3 && op0 == 7 {
        return if [6, 7, 14, 15].contains(&r) {
            vec![s]
        } else {
            vec![s, t]
        };
    }
    if instruction.width == 3 && op0 == 0 {
        let op1 = ((encoding >> 16) & 0xf) as u8;
        let op2 = ((encoding >> 20) & 0xf) as u8;
        if op1 == 0 && (op2 == 1 || op2 == 2 || op2 == 3 || (8..=15).contains(&op2)) {
            return vec![s, t];
        }
        if encoding == 0x0020_c0 {
            return vec![];
        }
        if op1 == 0 && op2 == 0 && r == 0 {
            let m = ((encoding >> 6) & 3) as u8;
            let n = ((encoding >> 4) & 3) as u8;
            if m == 2 && (n == 0 || n == 1) && s == 0 {
                return vec![0];
            }
            if m == 2 && n == 2 {
                return vec![s];
            }
            if m == 3 {
                return vec![s];
            }
        }
        if op1 == 1 && (op2 == 0 || op2 == 1) {
            return vec![s];
        }
        if op1 == 1 && [2, 3, 4, 6].contains(&op2) {
            return vec![t];
        }
        if op1 == 1 && op2 == 9 && s == 0 {
            return vec![t];
        }
        if op1 == 1 && op2 == 10 && t == 0 {
            return vec![s];
        }
        if op1 == 1 && op2 == 11 && s == 0 {
            return vec![t];
        }
        if op1 == 1 && [8, 12, 13].contains(&op2) {
            return vec![s, t];
        }
        if op1 == 2 && op2 <= 4 {
            return vec![];
        }
        if op1 == 2 && (op2 == 6 || op2 == 7 || op2 == 8 || (10..=15).contains(&op2)) {
            return vec![s, t];
        }
        if op1 == 3 && (op2 == 0 || op2 == 14) {
            return vec![];
        }
        if op1 == 3 && (op2 == 1 || op2 == 15) {
            return vec![t];
        }
        if op1 == 3 && [2, 3, 12, 13].contains(&op2) {
            return vec![s];
        }
        if op1 == 3 && (4..=11).contains(&op2) {
            return vec![s, t];
        }
        if op1 == 4 || op1 == 5 {
            return vec![t];
        }
    }
    panic!("unsupported register use at {:08x}", instruction.pc)
}

fn internal_sram(address: u32, width: u32) -> bool {
    SRAM_PAGES.iter().any(|page| {
        address >= *page
            && address
                .checked_add(width)
                .is_some_and(|end| end <= page + 4096)
    })
}

fn load_use_hazards(records: &[TraceRecord]) -> BTreeSet<usize> {
    let groups = instruction_groups(records);
    let mut hazards = BTreeSet::new();
    for pair in groups.windows(2) {
        let group = &pair[0];
        let next = &pair[1];
        let internal_loads: Vec<_> = group
            .data
            .iter()
            .filter(|(_, record)| record.kind == 2 && internal_sram(record.address, record.width))
            .collect();
        if internal_loads.is_empty() {
            continue;
        }
        assert_eq!(internal_loads.len(), 1);
        if group.data.len() != 1 {
            assert_eq!(group.data.len(), 2);
            assert_eq!(group.data[0].1.kind, 2);
            assert_eq!(group.data[1].1.kind, 3);
            assert_eq!(group.data[0].1.address, group.data[1].1.address);
        }
        assert_eq!(
            next.instruction.pc,
            group.instruction.pc.wrapping_add(group.instruction.width)
        );
        if let Some(register) = exact_load_destination(group.instruction, internal_loads[0].1.width)
        {
            if exact_source_registers(next.instruction).contains(&register) {
                hazards.insert(next.record_index);
            }
        }
    }
    hazards
}

fn timing_kind(record: TraceRecord, l32r_pcs: &BTreeSet<u32>) -> &'static str {
    match record.kind {
        1 => "instruction-fetch",
        2 if l32r_pcs.contains(&record.pc) => "literal-load",
        2 => "load",
        3 => "store",
        _ => unreachable!(),
    }
}

fn is_mmio(address: u32) -> bool {
    matches!(
        address & 0xffff_f000,
        0x6000_8000 | 0x6000_e000 | 0x600c_0000 | 0x600c_4000
    )
}

fn is_flash(address: u32) -> bool {
    matches!(address & 0xffff_f000, 0x4200_0000 | 0x4200_3000)
}

fn mmio_cost(record: TraceRecord, same_value: bool) -> Cost {
    let known_read = matches!(
        record.address,
        0x600c_0010
            | 0x600c_0060
            | 0x600c_4004
            | 0x600c_404c
            | 0x600c_4064
            | 0x600c_40a0
            | 0x600c_4130
    );
    let known_write =
        same_value && matches!(record.address, 0x600c_0060 | 0x600c_4004 | 0x600c_4064);
    if record.kind == 2 && known_read {
        Cost::known(8)
    } else if record.kind == 3 && known_write {
        Cost::known(3)
    } else {
        Cost::Unknown {
            reason: MMIO_UNKNOWN_REASON.into(),
        }
    }
}

fn push_event(events: &mut Vec<IssuedEvent>, id: String, kind: &str, origin: &str, cost: Cost) {
    events.push(IssuedEvent {
        issue_index: events.len(),
        event_id: id,
        event_kind: kind.into(),
        origin_kind: origin.into(),
        cost,
    });
}

fn push_instruction_boundary(
    events: &mut Vec<IssuedEvent>,
    pending: &(usize, String, usize),
    callbacks: &BTreeMap<usize, Vec<(usize, &RomCallback)>>,
) {
    let (_, access_id, instruction_count) = pending;
    push_event(
        events,
        format!("{access_id}:cpu"),
        "cpu",
        "cpu",
        Cost::known(1),
    );
    for (callback_index, callback) in callbacks.get(instruction_count).into_iter().flatten() {
        push_event(
            events,
            format!(
                "{access_id}:rom-callback:{callback_index}:{}",
                callback.kind
            ),
            "cpu",
            "cpu",
            callback.cpu_cost.clone(),
        );
    }
}

fn measured_replay(bundle: &Bundle) -> Vec<IssuedEvent> {
    let records = &bundle.trace.records;
    let hazards = load_use_hazards(records);
    let l32r_pcs: BTreeSet<_> = records
        .iter()
        .filter(|record| record.kind == 1 && record.width == 3 && record.instruction & 0xf == 1)
        .map(|record| record.pc)
        .collect();
    let mut callbacks: BTreeMap<usize, Vec<(usize, &RomCallback)>> = BTreeMap::new();
    for (index, callback) in bundle.rom_callbacks.iter().enumerate() {
        callbacks
            .entry(callback.after_instruction_count)
            .or_default()
            .push((index, callback));
    }
    let mut last_values = BTreeMap::new();
    let mut same_value_writes = BTreeSet::new();
    for (index, record) in records.iter().enumerate() {
        if record.kind == 2 || record.kind == 3 {
            let key = (record.address, record.width);
            if record.kind == 3 && last_values.get(&key) == Some(&record.value) {
                same_value_writes.insert(index);
            }
            last_values.insert(key, record.value);
        }
    }

    let mut events = Vec::new();
    let mut instruction_lines = BTreeSet::new();
    let mut data_lines = BTreeSet::new();
    let mut instruction_count = 0;
    let mut pending_instruction = None;
    for (index, record) in records.iter().copied().enumerate() {
        if record.kind == 1 {
            if let Some(pending) = &pending_instruction {
                push_instruction_boundary(&mut events, pending, &callbacks);
            }
        }
        let kind = timing_kind(record, &l32r_pcs);
        let access_id = format!("trace:{index:02}:{kind}");
        let address = if record.kind == 1 {
            record.pc
        } else {
            record.address
        };
        if is_mmio(address) {
            push_event(
                &mut events,
                format!("{access_id}:segment:0:mmio"),
                "mmio",
                "mmio",
                mmio_cost(record, same_value_writes.contains(&index)),
            );
        } else if is_flash(address) {
            let line_bytes = if record.kind == 1 { 32 } else { 64 };
            let line = address / line_bytes;
            let lines = if record.kind == 1 {
                &mut instruction_lines
            } else {
                &mut data_lines
            };
            if lines.insert(line) {
                push_event(
                    &mut events,
                    format!("cache:{access_id}:segment:0:cache:0:line-fill"),
                    kind,
                    "cache",
                    Cost::known(204),
                );
                push_event(
                    &mut events,
                    format!("cache:{access_id}:segment:0:cache:1:hit"),
                    kind,
                    "cache",
                    Cost::known(0),
                );
            } else {
                push_event(
                    &mut events,
                    format!("cache:{access_id}:segment:0:cache:0:hit"),
                    kind,
                    "cache",
                    Cost::known(0),
                );
            }
        } else {
            push_event(
                &mut events,
                format!("cache:{access_id}:segment:0:cache:0:sram-bypass"),
                kind,
                "cache",
                Cost::known(0),
            );
        }

        if record.kind == 1 {
            instruction_count += 1;
            if hazards.contains(&index) {
                push_event(
                    &mut events,
                    format!("{access_id}:pre-data-cpu"),
                    "cpu",
                    "cpu",
                    Cost::known(1),
                );
            }
            pending_instruction = Some((index, access_id, instruction_count));
        }
    }
    push_instruction_boundary(
        &mut events,
        pending_instruction
            .as_ref()
            .expect("trace has instructions"),
        &callbacks,
    );
    events
}

#[test]
fn measured_replay_matches_the_frozen_typescript_ledger() {
    let bundle: Bundle = serde_json::from_slice(BUNDLE).unwrap();
    assert_eq!(bundle.schema_version, 1);
    assert_eq!(bundle.format, "esp32s3-flexe-shared-replay-v1");
    assert_eq!(bundle.trace.encoding, "six-u32-little-endian");
    assert_eq!(bundle.inputs.elf_sha256, ELF_SHA256);
    assert_eq!(bundle.inputs.trace_record_sha256, TRACE_SHA256);
    assert_eq!(bundle.inputs.timing_profile_sha256, TIMING_PROFILE_SHA256);
    assert_eq!(bundle.trace.records.len(), 1_228);
    assert_eq!(bundle.rom_callbacks.len(), 30);
    assert_eq!(trace_sha256(&bundle.trace.records), TRACE_SHA256);

    let measured = measured_replay(&bundle);
    assert_eq!(measured.len(), bundle.typescript.issued_projection.len());
    for (index, (actual, expected)) in measured
        .iter()
        .zip(&bundle.typescript.issued_projection)
        .enumerate()
    {
        assert_eq!(actual, expected, "issued event {index}");
    }
    let canonical = serde_json::to_vec(&measured).unwrap();
    assert_eq!(sha256(&canonical), PROJECTION_SHA256);
    assert_eq!(
        bundle.typescript.replay.issued_projection_sha256,
        PROJECTION_SHA256
    );

    let replay = &bundle.typescript.replay;
    assert_eq!(replay.status, "blocked");
    assert_eq!(replay.issued_events, 2_288);
    assert_eq!(replay.memory_events, 1_177);
    assert_eq!(replay.mmio_events, 54);
    assert_eq!(replay.cpu_events, 1_057);
    assert_eq!(replay.rom_callback_events, 30);
    assert_eq!(replay.dependent_sram_load_use_hazards, 87);
    assert_eq!(replay.known_cost_events, 2_254);
    assert_eq!(replay.unknown_cost_events, 34);

    assert_eq!(
        measured_replay(&bundle),
        measured,
        "replay is deterministic"
    );
    eprintln!(
        "flexe differential: trace={} events={} known={} unknown={} ledger={}",
        TRACE_SHA256,
        measured.len(),
        replay.known_cost_events,
        replay.unknown_cost_events,
        PROJECTION_SHA256
    );
}
