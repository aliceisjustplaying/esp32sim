use crate::*;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct FakeInstruction {
    pub pc: u32,
    pub cost: Result<CostClaim, TimingBlock>,
    pub output: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
struct Pending {
    instruction: FakeInstruction,
    start: u64,
    completion: u64,
}

pub struct FakeBackend {
    config: BackendConfig,
    program: Vec<FakeInstruction>,
    program_index: usize,
    loaded: bool,
    reset: bool,
    closed: bool,
    epoch: u64,
    now: u64,
    ccount: u32,
    ccompare: [u32; 3],
    pending: Option<Pending>,
    inputs: VecDeque<InputEvent>,
    outputs: VecDeque<BackendEvent>,
    ledger: Vec<LedgerEntry>,
    next_sequence: u64,
    next_output_sequence: u64,
    last_input_sequence: Option<u64>,
    memory: Vec<u8>,
}

impl FakeBackend {
    pub fn new(config: BackendConfig, program: Vec<FakeInstruction>) -> Result<Self, BackendError> {
        validate_config(&config)?;
        Ok(Self {
            config,
            program,
            program_index: 0,
            loaded: false,
            reset: false,
            closed: false,
            epoch: 0,
            now: 0,
            ccount: 0,
            ccompare: [0; 3],
            pending: None,
            inputs: VecDeque::new(),
            outputs: VecDeque::new(),
            ledger: Vec::new(),
            next_sequence: 0,
            next_output_sequence: 0,
            last_input_sequence: None,
            memory: vec![0; 4096],
        })
    }

    pub fn set_ccompare(&mut self, index: usize, value: u32) {
        self.ccompare[index] = value;
    }

    pub fn ccount(&self) -> u32 {
        self.ccount
    }

    pub fn canonical_ledger(&self) -> Vec<u8> {
        canonical_ledger_bytes(&self.ledger)
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

    fn record(&mut self, cycle: u64, kind: LedgerKind, cost: Option<CostClaim>) {
        let entry = LedgerEntry {
            epoch: self.epoch,
            cycle,
            sequence: self.next_sequence,
            kind,
            cost,
        };
        self.next_sequence += 1;
        self.ledger.push(entry);
    }

    fn advance_time(&mut self, target: u64) {
        let delta = target - self.now;
        let before = self.ccount;
        self.ccount = self.ccount.wrapping_add(delta as u32);
        for index in 0..3 {
            let compare = self.ccompare[index];
            let wrapped_distance = compare.wrapping_sub(before);
            let distance = if wrapped_distance == 0 {
                1u64 << 32
            } else {
                u64::from(wrapped_distance)
            };
            if distance <= delta {
                self.record(
                    self.now + distance,
                    LedgerKind::CcompareAssert {
                        comparator: index as u8,
                    },
                    None,
                );
            }
        }
        self.now = target;
    }

    fn apply_inputs(&mut self) -> Option<ResetKind> {
        let reset = self
            .inputs
            .iter()
            .take_while(|event| event.cycle == self.now)
            .enumerate()
            .find_map(|(index, event)| match event.payload {
                InputPayload::Reset(kind) => Some((index, kind, event.caller_sequence)),
                InputPayload::Bytes(_) => None,
            });
        if let Some((index, kind, caller_sequence)) = reset {
            self.inputs.remove(index);
            self.record(self.now, LedgerKind::InputApplied { caller_sequence }, None);
            return Some(kind);
        }
        while self
            .inputs
            .front()
            .is_some_and(|event| event.cycle == self.now)
        {
            let event = self.inputs.pop_front().unwrap();
            self.record(
                self.now,
                LedgerKind::InputApplied {
                    caller_sequence: event.caller_sequence,
                },
                None,
            );
            match event.payload {
                InputPayload::Bytes(bytes) => {
                    self.outputs.push_back(BackendEvent {
                        schema_version: EVENT_SCHEMA_VERSION,
                        epoch: self.epoch,
                        cycle: self.now,
                        sequence: self.next_output_sequence,
                        payload: EventPayload::Bytes(bytes),
                    });
                    self.next_output_sequence += 1;
                }
                InputPayload::Reset(kind) => return Some(kind),
            }
        }
        None
    }

    fn delta(&self, start_index: usize) -> LedgerDelta {
        let entries = self.ledger[start_index..].to_vec();
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
            pending_instruction: self
                .pending
                .as_ref()
                .map(|pending| PendingInstructionSummary {
                    pc: pending.instruction.pc,
                    start: pending.start,
                    completion: pending.completion,
                }),
            stop,
            ledger: self.delta(ledger_start),
        }
    }
}

impl Backend for FakeBackend {
    fn load(&mut self, artifacts: Vec<Artifact>) -> Result<LoadReceipt, BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        let receipt = validate_artifacts(&artifacts)?;
        self.loaded = true;
        Ok(receipt)
    }

    fn reset(&mut self, kind: ResetKind) -> Result<ResetReceipt, BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        if !self.loaded {
            return Err(BackendError::NotLoaded);
        }
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or_else(|| BackendError::BackendFault("epoch overflow".into()))?;
        self.now = 0;
        self.ccount = 0;
        self.program_index = 0;
        self.pending = None;
        self.inputs.clear();
        self.outputs.clear();
        self.ledger.clear();
        self.next_sequence = 0;
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
            if request.cancellation.is_cancelled() {
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::BudgetExhausted(BudgetKind::WallCancellation),
                    ledger_start,
                ));
            }
            if self.ledger.len() - ledger_start >= request.budget.max_ledger_entries as usize {
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::BudgetExhausted(BudgetKind::LedgerEntries),
                    ledger_start,
                ));
            }
            if self.outputs.len() >= request.budget.max_output_events as usize {
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::BudgetExhausted(BudgetKind::OutputEvents),
                    ledger_start,
                ));
            }
            if self.pending.is_none() {
                if self.program_index >= self.program.len() {
                    if self.now < endpoint {
                        let next_input = self.inputs.front().map(|event| event.cycle);
                        let target = next_input.unwrap_or(endpoint).min(endpoint);
                        self.advance_time(target);
                        self.record(self.now, LedgerKind::IdleAdvance, None);
                        if let Some(kind) = self.apply_inputs() {
                            self.pending = None;
                            return Ok(self.slice(
                                start,
                                completed,
                                RunStop::ResetRequested(kind),
                                ledger_start,
                            ));
                        }
                    }
                    if self.now == request.deadline {
                        return Ok(self.slice(
                            start,
                            completed,
                            RunStop::DeadlineReached,
                            ledger_start,
                        ));
                    }
                    if self.now == cycle_endpoint {
                        return Ok(self.slice(
                            start,
                            completed,
                            RunStop::BudgetExhausted(BudgetKind::Cycles),
                            ledger_start,
                        ));
                    }
                    return Ok(self.slice(
                        start,
                        completed,
                        RunStop::Idle { next_event: None },
                        ledger_start,
                    ));
                }
                if completed == request.budget.max_instructions {
                    return Ok(self.slice(
                        start,
                        completed,
                        RunStop::BudgetExhausted(BudgetKind::Instructions),
                        ledger_start,
                    ));
                }
                let instruction = self.program[self.program_index].clone();
                let claim = match &instruction.cost {
                    Ok(claim) => claim.clone(),
                    Err(block) => {
                        return Ok(self.slice(
                            start,
                            completed,
                            RunStop::TimingBlocked(block.clone()),
                            ledger_start,
                        ));
                    }
                };
                let cycles = match claim.tier {
                    CostTier::Exact { cycles } => cycles,
                    _ => {
                        return Ok(self.slice(
                            start,
                            completed,
                            RunStop::TimingBlocked(TimingBlock {
                                claim_id: claim.id,
                                tier_candidate: claim.tier.candidate_name().into(),
                                reason: "online instruction duration is not an exact scalar".into(),
                            }),
                            ledger_start,
                        ));
                    }
                };
                let completion = self.now.checked_add(cycles).ok_or_else(|| {
                    BackendError::BackendFault("instruction completion overflow".into())
                })?;
                self.record(
                    self.now,
                    LedgerKind::InstructionStart {
                        pc: instruction.pc,
                        completion,
                    },
                    Some(claim),
                );
                self.pending = Some(Pending {
                    instruction,
                    start: self.now,
                    completion,
                });
            }

            let completion = self.pending.as_ref().unwrap().completion;
            let next_input = self
                .inputs
                .front()
                .map(|event| event.cycle)
                .unwrap_or(u64::MAX);
            let target = completion.min(next_input).min(endpoint);
            if target > self.now {
                self.advance_time(target);
            }
            if let Some(kind) = self.apply_inputs() {
                self.pending = None;
                return Ok(self.slice(
                    start,
                    completed,
                    RunStop::ResetRequested(kind),
                    ledger_start,
                ));
            }
            if self.now == completion {
                if completed == request.budget.max_instructions {
                    return Ok(self.slice(
                        start,
                        completed,
                        RunStop::BudgetExhausted(BudgetKind::Instructions),
                        ledger_start,
                    ));
                }
                let pending = self.pending.take().unwrap();
                self.record(
                    self.now,
                    LedgerKind::InstructionCommit {
                        pc: pending.instruction.pc,
                    },
                    None,
                );
                if let Some(bytes) = pending.instruction.output {
                    self.outputs.push_back(BackendEvent {
                        schema_version: EVENT_SCHEMA_VERSION,
                        epoch: self.epoch,
                        cycle: self.now,
                        sequence: self.next_output_sequence,
                        payload: EventPayload::Bytes(bytes),
                    });
                    self.next_output_sequence += 1;
                }
                self.program_index += 1;
                completed += 1;
            }
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
        let start = address as usize;
        let end = start
            .checked_add(max_bytes)
            .filter(|end| *end <= self.memory.len())
            .ok_or_else(|| BackendError::InvalidRequest("inspect range is not mapped".into()))?;
        Ok(Inspection {
            epoch: self.epoch,
            cycle: self.now,
            address,
            bytes: self.memory[start..end].to_vec(),
        })
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            adapter: ADAPTER_VERSION,
            event_schema: EVENT_SCHEMA_VERSION,
            ledger_schema: LEDGER_SCHEMA_VERSION,
            backend_name: "fake".into(),
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
        Ok(())
    }
}

pub fn test_claim(id: &str, cycles: u64) -> CostClaim {
    CostClaim {
        id: id.into(),
        tier: CostTier::Exact { cycles },
        receipts: vec![ReceiptRef {
            repository: "test".into(),
            commit: "0000000000000000000000000000000000000000".into(),
            path: "test".into(),
            sha256: Sha256::digest(id.as_bytes()).into(),
            firmware: "test".into(),
            sdkconfig_sha256: [0; 32],
            toolchain: "test".into(),
            board_revision: "test".into(),
            adoption_status: AdoptionStatus::Accepted,
        }],
    }
}
