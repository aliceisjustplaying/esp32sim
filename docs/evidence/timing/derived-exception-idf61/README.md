# Derived exception timing candidate

This analysis executes the real IDF 6.1 level 1, level 3, and window-vector
paths from the pinned TinyDraw product ELF through `step_measured`. It pins the
IDF 6.1 timing-target receipt and records every known ledger prefix.

All four interrupt entry and resume paths stop at `l32r`. Its adopted cost is
the interval 1 to 2 cycles, so the known term in each equation is not exact.
R8 therefore forbids deriving exception-entry delay E or return-redirect cost
R. Without E and R, the level 3 and 35-cycle window-pair validations cannot be
evaluated. The two real window handlers each contribute a known nine-cycle
ledger before their intentionally unpriced `rfwo` and `rfwu` instructions.

No exception timing price is adopted. Hardware queue item H1 remains the exact
receipt path for this family.

Reproduce with:

```text
TINYDRAW_VECTOR_V2_BUILD=/path/to/esp32-vector-v2 python3 analyze.py
```

The JSON is canonical `json.dumps(..., indent=2, sort_keys=True)` output.
