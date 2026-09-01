//! Version-1 measured backend adapter for the ESP32-S3 machine.

use crate::measured::{DeviceDeadline, Esp32TimingSource};
use crate::Machine;
use backend_api::*;
use std::collections::VecDeque;
use xtensa_lx7::exec::Trap;
use xtensa_lx7::measured::{
    complete_instruction, plan_instruction, CompletionError, PendingInstruction, PlanError,
};
use xtensa_lx7::state::TIMER_INTERRUPT;

const DIRECT_APP_OFFSET: usize = 0x1_0000;

#[derive(Clone)]
struct LoadedArtifacts {
    mask_rom: Option<Vec<u8>>,
    flash: Option<Vec<u8>>,
    application: Option<Vec<u8>>,
    profile: ImportedTimingProfile,
}

pub struct Esp32SimBackend {
    config: BackendConfig,
    trusted_receipts: ReceiptManifest,
    artifacts: Option<LoadedArtifacts>,
    machine: Option<Machine>,
    timing: Option<Esp32TimingSource>,
    loaded: bool,
    reset: bool,
    closed: bool,
    epoch: u64,
    now: u64,
    pending: Option<PendingInstruction>,
    inputs: VecDeque<InputEvent>,
    outputs: VecDeque<BackendEvent>,
    ledger: Vec<LedgerEntry>,
    due_ccompare: Vec<u8>,
    due_device: Option<DeviceDeadline>,
    next_ledger_sequence: u64,
    next_output_sequence: u64,
    last_input_sequence: Option<u64>,
}

impl Esp32SimBackend {
    pub fn new(
        config: BackendConfig,
        trusted_receipts: ReceiptManifest,
    ) -> Result<Self, BackendError> {
        validate_config(&config)?;
        Ok(Self {
            config,
            trusted_receipts,
            artifacts: None,
            machine: None,
            timing: None,
            loaded: false,
            reset: false,
            closed: false,
            epoch: 0,
            now: 0,
            pending: None,
            inputs: VecDeque::new(),
            outputs: VecDeque::new(),
            ledger: Vec::new(),
            due_ccompare: Vec::new(),
            due_device: None,
            next_ledger_sequence: 0,
            next_output_sequence: 0,
            last_input_sequence: None,
        })
    }

    pub fn canonical_ledger(&self) -> Vec<u8> {
        canonical_ledger_bytes(&self.ledger)
    }

    pub fn ccount(&self) -> Option<u32> {
        self.machine.as_ref().map(|machine| machine.cpu.ccount)
    }

    fn check_ready(&self) -> Result<(), BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        if !self.loaded {
            return Err(BackendError::NotLoaded);
        }
        if !self.reset {
            return Err(BackendError::NotReset);
        }
        Ok(())
    }

    fn machine(&self) -> &Machine {
        self.machine.as_ref().expect("ready backend has a machine")
    }

    fn machine_mut(&mut self) -> &mut Machine {
        self.machine.as_mut().expect("ready backend has a machine")
    }

    fn record(&mut self, kind: LedgerKind, costs: Vec<CostClaim>) {
        self.ledger.push(LedgerEntry {
            epoch: self.epoch,
            cycle: self.now,
            sequence: self.next_ledger_sequence,
            kind,
            costs,
        });
        self.next_ledger_sequence += 1;
    }

    fn delta(&self, start: usize) -> LedgerDelta {
        let entries = self.ledger[start..].to_vec();
        LedgerDelta {
            canonical_sha256: ledger_sha256(&entries),
            entries,
        }
    }

    fn slice(&self, start: u64, completed: u64, stop: RunStop, ledger_start: usize) -> RunSlice {
        RunSlice {
            epoch: self.epoch,
            start_cycle: start,
            end_cycle: self.now,
            completed_instructions: completed,
            pending_instruction: self.pending.as_ref().map(PendingInstruction::summary),
            stop,
            ledger: self.delta(ledger_start),
        }
    }

    fn timing_block(device: String, reason: String) -> RunStop {
        RunStop::TimingBlocked(TimingBlock {
            claim_id: format!("device:{device}"),
            tier_candidate: "unexplained".into(),
            reason,
        })
    }

    fn reset_input_at_now(&mut self) -> Option<ResetKind> {
        let reset = self
            .inputs
            .iter()
            .take_while(|event| event.cycle == self.now)
            .enumerate()
            .find_map(|(index, event)| match event.payload {
                InputPayload::Reset(kind) => Some((index, kind, event.caller_sequence)),
                InputPayload::Bytes(_) => None,
            });
        let (index, kind, caller_sequence) = reset?;
        self.inputs.remove(index);
        self.record(LedgerKind::InputApplied { caller_sequence }, vec![]);
        self.pending = None;
        Some(kind)
    }

    fn apply_byte_inputs_at_now(&mut self) {
        while self
            .inputs
            .front()
            .is_some_and(|event| event.cycle == self.now)
        {
            let event = self.inputs.pop_front().expect("front checked");
            self.record(
                LedgerKind::InputApplied {
                    caller_sequence: event.caller_sequence,
                },
                vec![],
            );
            match event.payload {
                InputPayload::Bytes(bytes) => {
                    self.machine_mut().bus.periph.usb.host_input(&bytes);
                    self.machine_mut().bus.irq_dirty = true;
                }
                InputPayload::Reset(_) => unreachable!("reset was handled before byte inputs"),
            }
        }
    }

    fn collect_outputs(&mut self) -> Result<(), BackendError> {
        let pending_bytes = {
            let periph = &self.machine().bus.periph;
            periph.usb.tx_out.len()
                + periph
                    .uart
                    .iter()
                    .map(|uart| uart.tx_out.len())
                    .sum::<usize>()
        };
        if pending_bytes == 0 {
            return Ok(());
        }
        if self.outputs.len() >= MAX_QUEUED_OUTPUT_EVENTS {
            return Err(BackendError::BackendFault("output queue is full".into()));
        }
        if pending_bytes > MAX_QUEUED_OUTPUT_BYTES.saturating_sub(self.queued_output_bytes()) {
            return Err(BackendError::BackendFault(
                "output queue byte limit exceeded".into(),
            ));
        }
        let machine = self.machine_mut();
        let mut bytes = std::mem::take(&mut machine.bus.periph.usb.tx_out);
        for uart in &mut machine.bus.periph.uart {
            bytes.extend(std::mem::take(&mut uart.tx_out));
        }
        self.outputs.push_back(BackendEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            epoch: self.epoch,
            cycle: self.now,
            sequence: self.next_output_sequence,
            payload: EventPayload::Bytes(bytes),
        });
        self.next_output_sequence += 1;
        Ok(())
    }

    fn process_devices_at_now(&mut self) -> Option<RunStop> {
        if let Some(before) = self.due_device.take() {
            let DeviceDeadline::At { device, .. } = before else {
                unreachable!("only exact device deadlines are deferred");
            };
            self.machine_mut().bus.measured_advance(1);
            let after = self.machine().bus.measured_device_deadline();
            if after
                == (DeviceDeadline::At {
                    cycle: self.now,
                    device: device.clone(),
                })
            {
                return Some(RunStop::BackendFault(format!(
                    "device deadline for {device} did not advance state"
                )));
            }
            self.record(LedgerKind::DeviceDeadline { device }, vec![]);
        }
        for _ in 0..1024 {
            let before = self.machine().bus.measured_device_deadline();
            match before {
                DeviceDeadline::Unknown { device, reason } => {
                    return Some(Self::timing_block(device, reason));
                }
                DeviceDeadline::None => return None,
                DeviceDeadline::At { cycle, .. } if cycle < self.now => {
                    return Some(RunStop::BackendFault(
                        "device deadline precedes virtual time".into(),
                    ));
                }
                DeviceDeadline::At { cycle, .. } if cycle > self.now => return None,
                DeviceDeadline::At { cycle: _, device } => {
                    self.machine_mut().bus.measured_advance(0);
                    let after = self.machine().bus.measured_device_deadline();
                    if after
                        == (DeviceDeadline::At {
                            cycle: self.now,
                            device: device.clone(),
                        })
                    {
                        return Some(RunStop::BackendFault(format!(
                            "device deadline for {device} did not advance state"
                        )));
                    }
                    self.record(LedgerKind::DeviceDeadline { device }, vec![]);
                }
            }
        }
        Some(RunStop::BackendFault(
            "too many device completions at one cycle".into(),
        ))
    }

    fn next_ccompare(&self) -> Option<u64> {
        let cpu = &self.machine().cpu;
        cpu.ccompare
            .iter()
            .map(|compare| {
                let distance = compare.wrapping_sub(cpu.ccount);
                let distance = if distance == 0 {
                    1u64 << 32
                } else {
                    u64::from(distance)
                };
                self.now.checked_add(distance)
            })
            .collect::<Option<Vec<_>>>()
            .and_then(|deadlines| deadlines.into_iter().min())
    }

    fn advance_to(&mut self, target: u64) {
        let delta = target - self.now;
        let device = self.machine().bus.measured_device_deadline();
        let defer_device = matches!(&device, DeviceDeadline::At { cycle, .. } if *cycle == target);
        let matches: Vec<u8> = {
            let cpu = &self.machine().cpu;
            cpu.ccompare
                .iter()
                .enumerate()
                .filter_map(|(index, compare)| {
                    let distance = compare.wrapping_sub(cpu.ccount);
                    let distance = if distance == 0 {
                        1u64 << 32
                    } else {
                        u64::from(distance)
                    };
                    (distance == delta).then_some(index as u8)
                })
                .collect()
        };
        if defer_device && delta != 0 {
            self.machine_mut().bus.measured_advance(delta - 1);
            self.due_device = Some(device);
        } else {
            self.machine_mut().bus.measured_advance(delta);
        }
        let mut remaining = delta;
        while remaining != 0 {
            let step = remaining.min(u32::MAX as u64) as u32;
            self.machine_mut().cpu.advance_ccount(step);
            remaining -= u64::from(step);
        }
        self.now = target;
        self.due_ccompare.extend(matches);
    }

    fn record_due_ccompare(&mut self) {
        let due = std::mem::take(&mut self.due_ccompare);
        for comparator in due {
            self.machine_mut().cpu.interrupt |= 1 << TIMER_INTERRUPT[comparator as usize];
            self.record(LedgerKind::CcompareAssert { comparator }, vec![]);
        }
    }

    fn next_input(&self) -> Option<u64> {
        self.inputs.front().map(|event| event.cycle)
    }

    fn next_device(&self) -> Result<Option<u64>, RunStop> {
        match self.machine().bus.measured_device_deadline() {
            DeviceDeadline::At { cycle, .. } => Ok(Some(cycle)),
            DeviceDeadline::None => Ok(None),
            DeviceDeadline::Unknown { device, reason } => Err(Self::timing_block(device, reason)),
        }
    }

    fn queued_input_bytes(&self) -> usize {
        self.inputs
            .iter()
            .map(|event| match &event.payload {
                InputPayload::Bytes(bytes) => bytes.len(),
                InputPayload::Reset(_) => 0,
            })
            .sum()
    }

    fn queued_output_bytes(&self) -> usize {
        self.outputs
            .iter()
            .map(|event| match &event.payload {
                EventPayload::Bytes(bytes) => bytes.len(),
                EventPayload::Reset(_) => 0,
            })
            .sum()
    }

    fn reset_kind(&self) -> ResetKind {
        match self.machine().bus.periph.rtc.reset_cause {
            crate::periph::RST_RTCWDT_SYS
            | crate::periph::RST_RTCWDT_CPU
            | crate::periph::RST_RTCWDT_RTC => ResetKind::Watchdog,
            _ => ResetKind::Software,
        }
    }

    fn current_cycle_ledger_entries(&self) -> usize {
        if self.machine().bus.periph.rtc.sw_reset {
            return 0;
        }
        if self
            .inputs
            .iter()
            .take_while(|event| event.cycle == self.now)
            .any(|event| matches!(event.payload, InputPayload::Reset(_)))
        {
            return 1;
        }
        let device_due = self.due_device.is_some()
            || matches!(
                self.machine().bus.measured_device_deadline(),
                DeviceDeadline::At { cycle, .. } if cycle == self.now
            );
        let devices = if device_due { 1024 } else { 0 };
        let inputs = self
            .inputs
            .iter()
            .take_while(|event| event.cycle == self.now)
            .count();
        let commit = usize::from(
            self.pending
                .as_ref()
                .is_some_and(|pending| pending.completion == self.now),
        );
        devices + self.due_ccompare.len() + inputs + commit
    }

    fn budget_stop(&self, request: &RunRequest, ledger_start: usize) -> Option<BudgetKind> {
        if request.cancellation.is_cancelled() {
            return Some(BudgetKind::WallCancellation);
        }
        let used = self.ledger.len() - ledger_start;
        if used >= request.budget.max_ledger_entries as usize
            || self.current_cycle_ledger_entries()
                > request.budget.max_ledger_entries as usize - used
        {
            return Some(BudgetKind::LedgerEntries);
        }
        if self.outputs.len() >= request.budget.max_output_events as usize {
            return Some(BudgetKind::OutputEvents);
        }
        None
    }
}

impl Backend for Esp32SimBackend {
    fn load(&mut self, artifacts: Vec<Artifact>) -> Result<LoadReceipt, BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        let receipt = validate_artifacts(&artifacts)?;
        let exactly_one = |kind: ArtifactKind| -> Result<Option<Vec<u8>>, BackendError> {
            let matching: Vec<_> = artifacts
                .iter()
                .filter(|artifact| artifact.kind == kind)
                .collect();
            if matching.len() > 1 {
                return Err(BackendError::InvalidArtifact(format!(
                    "duplicate {kind:?} artifact"
                )));
            }
            Ok(matching.first().map(|artifact| artifact.bytes.clone()))
        };
        let profile_bytes = exactly_one(ArtifactKind::TimingProfile)?.ok_or_else(|| {
            BackendError::InvalidArtifact("measured mode requires a timing profile".into())
        })?;
        let profile = import_timing_profile_v2(&profile_bytes, &self.trusted_receipts)
            .map_err(|error| BackendError::InvalidArtifact(format!("timing profile: {error:?}")))?;
        let loaded = LoadedArtifacts {
            mask_rom: exactly_one(ArtifactKind::MaskRom)?,
            flash: exactly_one(ArtifactKind::FlashImage)?,
            application: exactly_one(ArtifactKind::Application)?,
            profile,
        };
        match self.config.boot {
            BootMode::RomFlash if loaded.mask_rom.is_none() || loaded.flash.is_none() => {
                return Err(BackendError::InvalidArtifact(
                    "ROM-flash boot requires mask ROM and flash artifacts".into(),
                ));
            }
            BootMode::DirectApplication if loaded.application.is_none() => {
                return Err(BackendError::InvalidArtifact(
                    "direct boot requires an application artifact".into(),
                ));
            }
            _ => {}
        }
        self.artifacts = Some(loaded);
        self.machine = None;
        self.timing = None;
        self.loaded = true;
        self.reset = false;
        Ok(receipt)
    }

    fn reset(&mut self, kind: ResetKind) -> Result<ResetReceipt, BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        let artifacts = self.artifacts.clone().ok_or(BackendError::NotLoaded)?;
        let mut machine = Machine::new([0x02, 0, 0, 0, 0, 1]);
        match self.config.boot {
            BootMode::RomFlash => {
                machine
                    .load_rom(artifacts.mask_rom.as_deref().expect("validated ROM"))
                    .map_err(BackendError::InvalidArtifact)?;
                machine
                    .write_flash(0, artifacts.flash.as_deref().expect("validated flash"))
                    .map_err(BackendError::InvalidArtifact)?;
                machine.boot_rom();
            }
            BootMode::DirectApplication => {
                machine
                    .write_flash(
                        DIRECT_APP_OFFSET,
                        artifacts
                            .application
                            .as_deref()
                            .expect("validated application"),
                    )
                    .map_err(BackendError::InvalidArtifact)?;
                machine
                    .boot_app(DIRECT_APP_OFFSET)
                    .map_err(BackendError::InvalidArtifact)?;
            }
        }
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or_else(|| BackendError::BackendFault("epoch overflow".into()))?;
        self.now = 0;
        self.machine = Some(machine);
        self.timing = Some(Esp32TimingSource::new(artifacts.profile));
        self.pending = None;
        self.inputs.clear();
        self.outputs.clear();
        self.ledger.clear();
        self.due_ccompare.clear();
        self.due_device = None;
        self.next_ledger_sequence = 0;
        self.next_output_sequence = 0;
        self.last_input_sequence = None;
        self.reset = true;
        Ok(ResetReceipt {
            epoch: self.epoch,
            cycle: 0,
            kind,
        })
    }

    fn run_until(&mut self, request: RunRequest) -> Result<RunSlice, BackendError> {
        self.check_ready()?;
        if request.deadline < self.now {
            return Err(BackendError::InvalidRequest(
                "deadline precedes current cycle".into(),
            ));
        }
        if request.budget.max_cycles > MAX_RUN_CYCLES
            || request.budget.max_instructions > MAX_RUN_INSTRUCTIONS
            || request.budget.max_output_events as usize > MAX_QUEUED_OUTPUT_EVENTS
            || request.budget.max_ledger_entries as usize > MAX_LEDGER_ENTRIES_PER_RUN
        {
            return Err(BackendError::InvalidRequest(
                "run budget exceeds hard limit".into(),
            ));
        }
        let start = self.now;
        let ledger_start = self.ledger.len();
        let cycle_endpoint = start
            .checked_add(request.budget.max_cycles)
            .ok_or_else(|| BackendError::InvalidRequest("cycle budget overflow".into()))?;
        let endpoint = request.deadline.min(cycle_endpoint);
        let mut completed = 0u64;

        loop {
            if let Some(kind) = self.budget_stop(&request, ledger_start) {
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::BudgetExhausted(kind),
                    ledger_start,
                ));
            }
            if self.machine().bus.periph.rtc.sw_reset {
                self.pending = None;
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::ResetRequested(self.reset_kind()),
                    ledger_start,
                ));
            }
            if let Some(kind) = self.reset_input_at_now() {
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::ResetRequested(kind),
                    ledger_start,
                ));
            }
            if let Some(stop) = self.process_devices_at_now() {
                return Ok(self.slice(start, completed, stop, ledger_start));
            }
            if self.machine().bus.periph.rtc.sw_reset {
                self.pending = None;
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::ResetRequested(self.reset_kind()),
                    ledger_start,
                ));
            }
            self.record_due_ccompare();
            self.apply_byte_inputs_at_now();

            if self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.completion == self.now)
            {
                if completed == request.budget.max_instructions {
                    return Ok(self.slice(
                        start,
                        completed,
                        RunStop::BudgetExhausted(BudgetKind::Instructions),
                        ledger_start,
                    ));
                }
                let pending = self.pending.take().expect("completion checked");
                let pc = pending.observation.pc;
                let outcome = {
                    let machine = self.machine.as_mut().expect("ready");
                    let timing = self.timing.as_mut().expect("ready");
                    complete_instruction(
                        &mut machine.cpu,
                        &mut machine.bus,
                        timing,
                        pending,
                        self.now,
                    )
                };
                self.machine_mut().cpu.ccount = self.now as u32;
                self.record(LedgerKind::InstructionCommit { pc }, vec![]);
                completed += 1;
                match outcome {
                    Ok(()) | Err(CompletionError::Trap(Trap::Exception(_))) => {}
                    Err(CompletionError::Trap(trap)) => {
                        self.collect_outputs()?;
                        return Ok(self.slice(
                            start,
                            completed,
                            RunStop::BackendFault(format!("guest trap: {trap:?}")),
                            ledger_start,
                        ));
                    }
                    Err(CompletionError::BeforeCompletion) => unreachable!("completion checked"),
                }
            }
            self.collect_outputs()?;

            if self.now == request.deadline {
                return Ok(self.slice(start, completed, RunStop::DeadlineReached, ledger_start));
            }
            if self.now == cycle_endpoint {
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::BudgetExhausted(BudgetKind::Cycles),
                    ledger_start,
                ));
            }
            if completed == request.budget.max_instructions && self.pending.is_none() {
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::BudgetExhausted(BudgetKind::Instructions),
                    ledger_start,
                ));
            }
            if self.pending.is_none() {
                self.machine_mut().measured_refresh_interrupts();
                let _ = self.machine_mut().cpu.check_interrupts();
                if !self.machine().cpu.waiting {
                    let planned = {
                        let machine = self.machine();
                        let timing = self.timing.as_ref().expect("ready");
                        plan_instruction(&machine.cpu, &machine.bus, timing, self.now)
                    };
                    let pending = match planned {
                        Ok(pending) => pending,
                        Err(PlanError::Timing(block)) => {
                            return Ok(self.slice(
                                start,
                                completed,
                                RunStop::TimingBlocked(block),
                                ledger_start,
                            ));
                        }
                        Err(PlanError::Fetch { pc, tier_candidate }) => {
                            return Ok(self.slice(
                                start,
                                completed,
                                RunStop::TimingBlocked(TimingBlock {
                                    claim_id: format!("fetch:{pc:08x}"),
                                    tier_candidate,
                                    reason: "instruction fetch is unresolved".into(),
                                }),
                                ledger_start,
                            ));
                        }
                        Err(PlanError::CompletionOverflow) => {
                            return Ok(self.slice(
                                start,
                                completed,
                                RunStop::BackendFault("instruction completion overflow".into()),
                                ledger_start,
                            ));
                        }
                    };
                    self.record(
                        LedgerKind::InstructionStart {
                            pc: pending.observation.pc,
                            completion: pending.completion,
                        },
                        pending.claims.clone(),
                    );
                    self.pending = Some(pending);
                    continue;
                }
            }

            let device = match self.next_device() {
                Ok(deadline) => deadline.unwrap_or(u64::MAX),
                Err(stop) => return Ok(self.slice(start, completed, stop, ledger_start)),
            };
            let target = endpoint
                .min(
                    self.pending
                        .as_ref()
                        .map_or(u64::MAX, |pending| pending.completion),
                )
                .min(self.next_input().unwrap_or(u64::MAX))
                .min(self.next_ccompare().unwrap_or(u64::MAX))
                .min(device);
            if target < self.now {
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::BackendFault("scheduler target precedes virtual time".into()),
                    ledger_start,
                ));
            }
            if target == self.now {
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::BackendFault("scheduler made no progress".into()),
                    ledger_start,
                ));
            }
            self.advance_to(target);
        }
    }

    fn inject(&mut self, event: InputEvent) -> Result<(), BackendError> {
        self.check_ready()?;
        if event.epoch != self.epoch {
            return Err(BackendError::InvalidInput("input epoch mismatch".into()));
        }
        if event.cycle < self.now {
            return Err(BackendError::InvalidInput(
                "input precedes current cycle".into(),
            ));
        }
        if self
            .last_input_sequence
            .is_some_and(|previous| event.caller_sequence <= previous)
        {
            return Err(BackendError::InvalidInput(
                "input caller sequence is not increasing".into(),
            ));
        }
        if self.inputs.len() >= MAX_QUEUED_INPUT_EVENTS {
            return Err(BackendError::InvalidInput("input queue is full".into()));
        }
        let additional = match &event.payload {
            InputPayload::Bytes(bytes) => bytes.len(),
            InputPayload::Reset(_) => 0,
        };
        if additional > MAX_QUEUED_INPUT_BYTES.saturating_sub(self.queued_input_bytes()) {
            return Err(BackendError::InvalidInput(
                "input queue byte limit exceeded".into(),
            ));
        }
        self.last_input_sequence = Some(event.caller_sequence);
        let index = self
            .inputs
            .iter()
            .position(|queued| {
                (queued.cycle, queued.caller_sequence) > (event.cycle, event.caller_sequence)
            })
            .unwrap_or(self.inputs.len());
        self.inputs.insert(index, event);
        Ok(())
    }

    fn drain_events(&mut self, limit: usize) -> Result<EventBatch, BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        let count = limit.min(self.outputs.len());
        let events = self.outputs.drain(..count).collect();
        Ok(EventBatch {
            events,
            remaining: self.outputs.len(),
        })
    }

    fn inspect(&self, address: u32, max_bytes: usize) -> Result<Inspection, BackendError> {
        self.check_ready()?;
        if !self.config.inspection {
            return Err(BackendError::InspectionDenied);
        }
        if max_bytes > MAX_INSPECT_BYTES {
            return Err(BackendError::InvalidRequest(
                "inspect limit exceeds hard limit".into(),
            ));
        }
        let bytes = self
            .machine()
            .bus
            .measured_inspect(address, max_bytes)
            .ok_or_else(|| BackendError::InvalidRequest("inspect range is not mapped".into()))?;
        Ok(Inspection {
            epoch: self.epoch,
            cycle: self.now,
            address,
            bytes,
        })
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            adapter: ADAPTER_VERSION,
            event_schema: EVENT_SCHEMA_VERSION,
            ledger_schema: LEDGER_SCHEMA_VERSION,
            backend_name: "esp32sim-measured".into(),
            measured_interpreter: true,
            measured_single_core: true,
            measured_dual_core: false,
            networking: false,
            native_jit_observation_proven: false,
        }
    }

    fn close(&mut self) -> Result<(), BackendError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.inputs.clear();
        self.outputs.clear();
        self.pending = None;
        self.machine = None;
        self.timing = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn hex(bytes: &[u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn trusted_receipt() -> ReceiptRef {
        ReceiptRef {
            repository: "test-repository".into(),
            commit: "0000000000000000000000000000000000000000".into(),
            path: "test-receipt.json".into(),
            sha256: [1; 32],
            firmware: "contract-fixture".into(),
            sdkconfig_sha256: [2; 32],
            toolchain: "contract-toolchain".into(),
            board_revision: "contract-board".into(),
            adoption_status: AdoptionStatus::Accepted,
        }
    }

    fn raw_receipt(receipt: &ReceiptRef) -> String {
        format!(
            r#"{{"repository":"{}","commit":"{}","path":"{}","sha256":"{}","firmware":"{}","sdkconfigSha256":"{}","toolchain":"{}","boardRevision":"{}","adoptionStatus":"accepted"}}"#,
            receipt.repository,
            receipt.commit,
            receipt.path,
            hex(&receipt.sha256),
            receipt.firmware,
            hex(&receipt.sdkconfig_sha256),
            receipt.toolchain,
            receipt.board_revision,
        )
    }

    fn profile(base_cycles: u64, flash_first_line: Option<u64>) -> (Vec<u8>, ReceiptManifest) {
        let receipt = trusted_receipt();
        let cache_claim = flash_first_line.map_or(String::new(), |cycles| {
            format!(
                r#",{{"id":"cache-first","tier":"exact","cycles":{cycles},"receipt":{}}}"#,
                raw_receipt(&receipt)
            )
        });
        let cache_binding = flash_first_line.map_or(String::new(), |_| {
            r#",{"claimId":"cache-first","class":"cache","cache":"instruction","memory":"flash","event":"first-line-fill"}"#.into()
        });
        let json = format!(
            r#"{{"schemaVersion":2,"format":"esp32sim-timing-profile-v2","claims":[{{"id":"base","tier":"exact","cycles":{base_cycles},"receipt":{}}}{cache_claim}],"bindings":[{{"claimId":"base","class":"block-base","blockClass":"straight-line"}}{cache_binding}]}}"#,
            raw_receipt(&receipt),
        );
        (json.into_bytes(), vec![receipt])
    }

    fn application(entry: u32) -> Vec<u8> {
        let mut image = vec![0; 24];
        image[0] = 0xe9;
        image[1] = 1;
        image[4..8].copy_from_slice(&entry.to_le_bytes());
        image.extend_from_slice(&entry.to_le_bytes());
        let code = [0xf0, 0x20, 0x00].repeat(32);
        image.extend_from_slice(&(code.len() as u32).to_le_bytes());
        image.extend_from_slice(&code);
        image
    }

    fn ready_backend(base_cycles: u64, entry: u32, cache: Option<u64>) -> Esp32SimBackend {
        let (profile, receipts) = profile(base_cycles, cache);
        let mut config = BackendConfig::default();
        config.inspection = true;
        let mut backend = Esp32SimBackend::new(config, receipts).unwrap();
        backend
            .load(vec![
                Artifact::new("profile", ArtifactKind::TimingProfile, profile),
                Artifact::new("application", ArtifactKind::Application, application(entry)),
            ])
            .unwrap();
        backend.reset(ResetKind::PowerOn).unwrap();
        backend
    }

    fn request(deadline: u64) -> RunRequest {
        RunRequest {
            deadline,
            budget: RunBudget::default(),
            cancellation: CancellationFlag::new(),
        }
    }

    #[test]
    fn esp32sim_passes_the_shared_backend_contract() {
        let mut factory =
            || Box::new(ready_backend(5, crate::bus::IRAM_LOW, None)) as Box<dyn Backend>;
        backend_api::contract_suite::run_shared_contract(&mut factory);
    }

    #[test]
    fn real_backend_ledger_keeps_base_and_cache_components() {
        let entry = crate::bus::IBUS_LOW + 0x1_0020;
        let mut backend = ready_backend(1, entry, Some(400));
        let slice = backend.run_until(request(401)).unwrap();
        let start = slice
            .ledger
            .entries
            .iter()
            .find(|entry| matches!(entry.kind, LedgerKind::InstructionStart { .. }))
            .unwrap();
        assert_eq!(start.costs.len(), 2);
        assert_eq!(
            start
                .costs
                .iter()
                .map(|claim| claim.id.as_str())
                .collect::<Vec<_>>(),
            ["base", "cache-first"]
        );
        assert_eq!(slice.completed_instructions, 1);
    }

    #[test]
    fn ccompare_and_usb_deadlines_split_pending_latency_exactly() {
        let mut backend = ready_backend(100_000, crate::bus::IRAM_LOW, None);
        backend.machine_mut().cpu.ccompare[0] = 30_000;
        let slice = backend.run_until(request(100_000)).unwrap();
        let ccompare = slice
            .ledger
            .entries
            .iter()
            .find(|entry| matches!(entry.kind, LedgerKind::CcompareAssert { comparator: 0 }))
            .unwrap();
        let usb = slice
            .ledger
            .entries
            .iter()
            .find(|entry| {
                matches!(
                    &entry.kind,
                    LedgerKind::DeviceDeadline { device } if device == "usb-serial-jtag"
                )
            })
            .unwrap();
        assert_eq!(ccompare.cycle, 30_000);
        assert_eq!(usb.cycle, crate::periph::CPU_HZ / 4000);
        assert_eq!(slice.completed_instructions, 1);
        assert_eq!(backend.ccount(), Some(100_000));
    }

    #[test]
    fn active_unmodeled_device_blocks_without_advancing() {
        let mut backend = ready_backend(5, crate::bus::IRAM_LOW, None);
        backend.machine_mut().bus.periph.rmt.ch[0].running = true;
        let slice = backend.run_until(request(10)).unwrap();
        assert_eq!(slice.end_cycle, 0);
        assert!(matches!(
            slice.stop,
            RunStop::TimingBlocked(TimingBlock { claim_id, .. }) if claim_id == "device:rmt"
        ));
        assert!(slice.ledger.entries.is_empty());
    }

    #[test]
    fn wdt_reset_precedes_same_cycle_input_and_ready_completion() {
        let mut backend = ready_backend(1600, crate::bus::IRAM_LOW, None);
        backend
            .machine_mut()
            .bus
            .periph
            .rtc
            .ram
            .write(0x98, (1 << 31) | (2 << 28));
        backend.machine_mut().bus.periph.rtc.ram.write(0x9c, 1);
        backend
            .inject(InputEvent {
                epoch: 1,
                cycle: 1600,
                caller_sequence: 1,
                payload: InputPayload::Bytes(vec![0x77]),
            })
            .unwrap();

        let slice = backend.run_until(request(1600)).unwrap();
        assert_eq!(slice.stop, RunStop::ResetRequested(ResetKind::Watchdog));
        assert_eq!(slice.completed_instructions, 0);
        assert!(slice.pending_instruction.is_none());
        assert_eq!(
            slice
                .ledger
                .entries
                .iter()
                .filter(|entry| matches!(entry.kind, LedgerKind::DeviceDeadline { .. }))
                .count(),
            1
        );
        assert!(!slice
            .ledger
            .entries
            .iter()
            .any(|entry| matches!(entry.kind, LedgerKind::InputApplied { .. })));
    }

    proptest! {
        #[test]
        fn esp32_backend_slice_invariance_is_byte_exact(
            raw_cuts in prop::collection::vec(0u64..150, 0..40),
        ) {
            let total = 150;
            let mut whole = ready_backend(5, crate::bus::IRAM_LOW, None);
            whole.run_until(request(total)).unwrap();
            let whole_ledger = whole.canonical_ledger();
            let whole_events = whole.drain_events(usize::MAX).unwrap().events;
            let whole_ccount = whole.ccount();

            let mut partitioned = ready_backend(5, crate::bus::IRAM_LOW, None);
            let mut cuts: Vec<_> = raw_cuts.into_iter().map(|cut| cut.min(total)).collect();
            cuts.push(total);
            cuts.sort_unstable();
            cuts.dedup();
            for cut in cuts {
                partitioned.run_until(request(cut)).unwrap();
            }
            prop_assert_eq!(partitioned.canonical_ledger(), whole_ledger);
            prop_assert_eq!(partitioned.drain_events(usize::MAX).unwrap().events, whole_events);
            prop_assert_eq!(partitioned.ccount(), whole_ccount);
        }
    }
}
