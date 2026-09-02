//! Shared ESP32-S3 external-memory cache state.

use crate::ChipConfig;

/// Replacement policy used when all ways in a set are valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplacementPolicy {
    LeastRecentlyUsed,
}

/// External-memory access routed through a cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKind {
    Fetch,
    Load,
    Store,
}

/// Cache selected for an explicit maintenance operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheTarget {
    Instruction,
    Data,
}

/// Backing external memory for a missed line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheSource {
    Flash,
    Psram,
}

/// Position of a miss within the fill sequence since invalidation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillPosition {
    First,
    Subsequent,
}

/// Result of an external-memory cache lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessResult {
    Hit,
    Miss {
        position: FillPosition,
        source: CacheSource,
    },
}

/// Typed refusal for a cache configuration outside the receipt scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnsupportedChipConfig {
    pub configuration: ChipConfig,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Line {
    tag: u32,
    last_use: u64,
    valid: bool,
    dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Cache {
    line_bytes: u32,
    set_count: u32,
    ways: Vec<Vec<Line>>,
    first_fill: bool,
    use_clock: u64,
}

impl Cache {
    fn new(size_bytes: u32, ways: u8, line_bytes: u8) -> Self {
        let set_count = size_bytes / u32::from(ways) / u32::from(line_bytes);
        Self {
            line_bytes: u32::from(line_bytes),
            set_count,
            ways: vec![vec![Line::default(); usize::from(ways)]; set_count as usize],
            first_fill: true,
            use_clock: 0,
        }
    }

    fn access(&mut self, addr: u32, dirty: bool, source: CacheSource) -> AccessResult {
        let line_number = addr / self.line_bytes;
        let set_index = line_number % self.set_count;
        let tag = line_number / self.set_count;
        self.use_clock = self.use_clock.saturating_add(1);
        let ways = &mut self.ways[set_index as usize];

        if let Some(line) = ways.iter_mut().find(|line| line.valid && line.tag == tag) {
            line.last_use = self.use_clock;
            line.dirty |= dirty;
            return AccessResult::Hit;
        }

        let victim = ways.iter().position(|line| !line.valid).unwrap_or_else(|| {
            ways.iter()
                .enumerate()
                .min_by_key(|(way, line)| (line.last_use, *way))
                .map_or(0, |(way, _line)| way)
        });
        ways[victim] = Line {
            tag,
            last_use: self.use_clock,
            valid: true,
            dirty,
        };
        let position = if self.first_fill {
            self.first_fill = false;
            FillPosition::First
        } else {
            FillPosition::Subsequent
        };
        AccessResult::Miss { position, source }
    }

    fn invalidate(&mut self, addr: u32, len: u32) {
        self.for_each_line_in_range(addr, len, |line| *line = Line::default());
        self.first_fill = true;
    }

    fn invalidate_all(&mut self) {
        for set in &mut self.ways {
            set.fill(Line::default());
        }
        self.first_fill = true;
    }

    fn writeback(&mut self, addr: u32, len: u32) -> u32 {
        let mut dirty_lines = 0_u32;
        self.for_each_line_in_range(addr, len, |line| {
            if line.dirty {
                line.dirty = false;
                dirty_lines = dirty_lines.saturating_add(1);
            }
        });
        dirty_lines
    }

    fn for_each_line_in_range(&mut self, addr: u32, len: u32, mut apply: impl FnMut(&mut Line)) {
        let Some(last_addr) = len.checked_sub(1).map(|offset| addr.saturating_add(offset)) else {
            return;
        };
        let first_line = addr / self.line_bytes;
        let last_line = last_addr / self.line_bytes;
        for (set_index, set) in self.ways.iter_mut().enumerate() {
            for line in set {
                let line_number = line
                    .tag
                    .saturating_mul(self.set_count)
                    .saturating_add(set_index as u32);
                if line.valid && (first_line..=last_line).contains(&line_number) {
                    apply(line);
                }
            }
        }
    }
}

/// One shared I-cache and one shared D-cache, as specified by the ESP32-S3 TRM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheModel {
    instruction: Cache,
    data: Cache,
    policy: ReplacementPolicy,
}

impl CacheModel {
    pub fn new(config: ChipConfig) -> Result<Self, UnsupportedChipConfig> {
        if config != ChipConfig::RECEIPT_SCOPE {
            return Err(UnsupportedChipConfig {
                configuration: config,
            });
        }
        Ok(Self {
            instruction: Cache::new(
                config.icache_size_bytes,
                config.icache_ways,
                config.icache_line_bytes,
            ),
            data: Cache::new(
                config.dcache_size_bytes,
                config.dcache_ways,
                config.dcache_line_bytes,
            ),
            policy: ReplacementPolicy::LeastRecentlyUsed,
        })
    }

    pub const fn replacement_policy(&self) -> ReplacementPolicy {
        self.policy
    }

    pub fn access(&mut self, kind: AccessKind, addr: u32) -> AccessResult {
        let source = source_for(kind, addr);
        match kind {
            AccessKind::Fetch => self.instruction.access(addr, false, source),
            AccessKind::Load => self.data.access(addr, false, source),
            AccessKind::Store => self.data.access(addr, true, source),
        }
    }

    /// Invalidates lines without writing dirty data back.
    pub fn invalidate(&mut self, target: CacheTarget, addr: u32, len: u32) {
        match target {
            CacheTarget::Instruction => self.instruction.invalidate(addr, len),
            CacheTarget::Data => self.data.invalidate(addr, len),
        }
    }

    pub fn invalidate_all(&mut self, target: CacheTarget) {
        match target {
            CacheTarget::Instruction => self.instruction.invalidate_all(),
            CacheTarget::Data => self.data.invalidate_all(),
        }
    }

    /// Clears dirty state and returns the number of written-back D-cache lines.
    pub fn writeback(&mut self, addr: u32, len: u32) -> u32 {
        self.data.writeback(addr, len)
    }
}

const fn source_for(kind: AccessKind, addr: u32) -> CacheSource {
    let psram_window = match kind {
        AccessKind::Fetch => addr & 0xff00_0000 == 0x4300_0000,
        AccessKind::Load | AccessKind::Store => addr & 0xff00_0000 == 0x3d00_0000,
    };
    if psram_window {
        CacheSource::Psram
    } else {
        CacheSource::Flash
    }
}
