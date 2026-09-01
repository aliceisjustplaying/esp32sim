# Provenance

Upstream: https://github.com/joakimeriksson/esp32sim
Pinned base commit: 2114ffc92039b4605264d2cfb4ee5543acbf98c1 (2026-08-30)
Fork: https://github.com/aliceisjustplaying/esp32sim
Retrieved: 2026-08-31, by clone; branch `alice` is the pinned base
plus fork-carried commits for the cycle-model product.

License basis: upstream declares MIT in its README ("No cloud, no
accounts. MIT.", and again under "Provenance") and in every crate via the
workspace manifest (`Cargo.toml: license = "MIT"`). No root LICENSE file
exists upstream yet; the author has been contacted and will be asked to
add one.

Branch conventions:
- `main`: clean mirror of upstream; never committed to directly.
- `alice`: the product branch; the pinned base plus fork-carried
  changes that upstream declines or has not yet accepted. All live
  work happens here or on short-lived branches cut from here (or from
  `main` for upstream-shaped pull requests).
- `salvage/...`: frozen earlier work branches, read-only inputs;
  inventoried in `docs/STATUS.md`.

Upstream-first rule: fixes and capabilities upstream wants are submitted
as upstream pull requests from `main`-based branches; `alice` carries
only what upstream declines. Upstream syncs are explicit, reviewed
merges/rebases with a range-diff summary.

Project history: the project incubated in
https://github.com/aliceisjustplaying/puck (branch
`codex/esp32s3-timing-model`) and was then planned from
https://github.com/aliceisjustplaying/esp32s3-cycle-accurate-wasm.
Both are frozen archives. Their receipts were copied into
`docs/evidence/` with hashes intact; nothing else from them is
required reading.
