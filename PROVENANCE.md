# Provenance

Upstream: https://github.com/joakimeriksson/esp32sim
Pinned base commit: 2114ffc92039b4605264d2cfb4ee5543acbf98c1 (2026-08-30)
Fork: https://github.com/aliceisjustplaying/esp32sim
Retrieved: 2026-08-31, by clone; this branch (`puck/base`) is the pinned
base plus fork-carried commits for the puck cycle-model project.

License basis: upstream declares MIT in its README ("No cloud, no
accounts. MIT.", and again under "Provenance") and in every crate via the
workspace manifest (`Cargo.toml: license = "MIT"`). No root LICENSE file
exists upstream yet; the author has been contacted and will be asked to
add one. Recorded in the puck repository's decision 0011.

Branch conventions:
- `main`: clean mirror of upstream; never committed to directly.
- `puck/base`: this branch; the pinned base plus fork-carried changes
  that upstream declines or has not yet accepted.
- `lane-*/...`: work branches per the puck repository's
  `docs/roadmap.md` lanes, branched from `puck/base` (or from `main` for
  upstream-shaped pull requests).

Upstream-first rule: fixes and capabilities upstream wants are submitted
as upstream pull requests from `main`-based branches; `puck/base` carries
only what upstream declines. Upstream syncs are explicit, reviewed
merges/rebases with a range-diff summary.

Planning, decisions, receipts, and lane briefs live in the program
office repository:
https://github.com/aliceisjustplaying/esp32s3-cycle-accurate-wasm
(`roadmap.md`, `lanes/`, `decisions/`). The project incubated in
https://github.com/aliceisjustplaying/puck, branch
`codex/esp32s3-timing-model`, now a frozen archive and donor.
