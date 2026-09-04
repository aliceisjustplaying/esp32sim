# H2 exception-rank follow-up

This receipt pins the completed IDF 6.1 H2 archive. Both boots are byte-identical,
each contains 100 accepted samples for all nine cells, and neither contains a
refusal. The exact observed totals are `5, 5, 19, 6, 5, 6, 4, 4, 4` in manifest
order.

No price is adopted. H2 did not reproduce the declared H1 direct raw totals
`6, 5, 18`, and its observed window excess is 9 rather than the declared H1
residual 17. A source audit found that those cross-build comparisons used
different synchronization and instruction placement, omitted an executed
vector jump, and treated real window-handler work as one-cycle instructions.
The declared validation gate was therefore invalid.

Reproduce with the archived IDF 6.1 toolchain available:

```text
python3 docs/evidence/timing/h2-exception-rank-followup-2026-09-04/analyze.py > /Users/sarah/tmp/h2-summary.json
diff -u docs/evidence/timing/h2-exception-rank-followup-2026-09-04/summary.json /Users/sarah/tmp/h2-summary.json
```

`ESP32S3_H2_ARCHIVE` may point to a relocated copy of the pinned archive.
