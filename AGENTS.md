# Working rules

This fork is the product and the only live repository for it. Branch
`alice` is the product branch; everything you need is on it. `main` is
a clean upstream mirror and is never committed to directly. Branches
under `salvage/` are frozen inputs from an earlier project phase: read
them, harvest from them under review, never resume work on them.

The goal is `docs/GOAL.md`. Current state and the hardware queue are
`docs/STATUS.md`. Measured evidence lives under `docs/evidence/`.
Predecessor repositories are archived and are not required reading;
consult them only when a receipt chain in `docs/evidence/` explicitly
points there.

## Claims and evidence

- Every measured or adopted number carries a receipt: a committed file
  under `docs/evidence/`, a hash, or a committed test that recomputes
  it. No receipt, no claim.
- Cost claims use the tiered vocabulary: exact, affine, interval,
  distribution, unexplained. A refusal names its tier candidate.
- Fail closed, always: unknown costs stay unknown and block totals;
  missing corpora fail tests rather than skipping; unsupported
  operations are refused by name, never faked.
- Do not claim "cycle-accurate" without qualifying which tier and
  which cost classes the claim covers.

## Engine discipline

- Upstream's fast mode stays bit-identical and is never slowed by
  measured-mode bookkeeping.
- The measured interpreter is the reference implementation. The costed
  JIT is the product and must agree with the reference: same trace in,
  same cycle ledger out, deterministically.
- Upstream's interpreter-versus-JIT bit-identity rule is preserved for
  architectural state on every engine change.
- The machine is dual-core native. Do not build single-core
  scaffolding that assumes a second core can be added later.
- No TypeScript execution engine is ever built. The web shell is a
  thin transport and UI client.

## Checkouts and worktrees

- One canonical checkout per live repository:
  `~/src/a/esp32sim` (this repo) and `~/src/a/tinydraw`. Do not make
  additional clones of either.
- An agent that needs isolation uses `git worktree add` from the
  canonical checkout and removes the worktree when its branch is
  merged or abandoned.
- Nothing valuable lives only in a working tree overnight: push the
  branch the same day or treat the work as disposable.
- Run `git fetch` before making any claim about repository or branch
  state.
- Receipt-pinned bytes (raw captures, built probe ELFs) are copied to
  committed evidence or `~/Archives/esp32s3/` at capture time, never
  left inside a working tree.

## Process

- Upstream-first: fixes and capabilities upstream would want are built
  as upstream-shaped pull requests; this fork carries only what
  upstream declines, recorded in `PROVENANCE.md`.
- Granular commits with plain, specific messages; push at milestones.
- The physical board has one owner at a time. Hardware needs are
  queued in `docs/STATUS.md`; never open the serial port or JTAG
  opportunistically.
- Stop and report when blocked or when a finding contradicts the goal
  or the evidence; do not guess.

## Prose

- US English spelling only.
- No em dashes anywhere, including code comments: use commas, colons,
  parentheses, or periods.
- No ASCII art, no badges in markdown.
