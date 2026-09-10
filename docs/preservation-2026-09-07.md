# Experiment preservation — September 7, 2026

The preservation pass is complete. The [archive guide](/Users/alice/Archives/esp32s3/preservation-2026-09-07/README.md) is the starting point for recovery; the [cleanup manifest](/Users/alice/Archives/esp32s3/preservation-2026-09-07/cleanup-manifest.json) gives original paths, recovery locations and the conditions for future removal. Redundant original files and worktrees were subsequently removed in the [September 8 cleanup](cleanup-2026-09-08.md). Local branch heads remain in place.

The archive covers this repository's source, experiment files and build outputs, the ESP32-S3 hardware archive and the old coordinator project. It preserves 370,727 files as 78,549 distinct file contents: 41.3 GB of logical file data represented by 20.1 GB of stored payloads. Git packages and filesystem metadata use additional storage. These figures describe the archive's deduplication. [Capture and verification receipt](/Users/alice/Archives/esp32s3/preservation-2026-09-07/COMPLETE.json)

| Preserved material | Recovery record |
| --- | --- |
| Source and evidence files, including untracked experiment files and exact available binaries | [Readable snapshots](/Users/alice/Archives/esp32s3/preservation-2026-09-07/files), [per-file manifest](/Users/alice/Archives/esp32s3/preservation-2026-09-07/files.jsonl) |
| 60 worktree states, including staged and unstaged edits | [Worktree map](/Users/alice/Archives/esp32s3/preservation-2026-09-07/worktree-catalog-map.json) |
| 130 branch heads across two repositories | [Branch recovery locations](/Users/alice/Archives/esp32s3/preservation-2026-09-07/cleanup-manifest.json), [Git bundles](/Users/alice/Archives/esp32s3/preservation-2026-09-07/git) |
| Local Git settings, reflogs, indexes and additional objects | [Git administration verification](/Users/alice/Archives/esp32s3/preservation-2026-09-07/git-administration-verification.json) |
| All 101 source locations referenced by the catalog | [Receipt map](/Users/alice/Archives/esp32s3/preservation-2026-09-07/catalog-receipts.json), [source inventory](experiment-sources.json) |

Every unique file payload was copied into a fresh directory and checked by SHA-256. Every readable snapshot path was checked against its stored payload. Both Git bundles were freshly cloned and checked, with their original refs compared. All 60 tracked worktrees were reconstructed and their staged and unstaged binary diffs matched exactly. [Verification details](/Users/alice/Archives/esp32s3/preservation-2026-09-07/COMPLETE.json)

The complete Git administration snapshots also matched the original files byte for byte. The main repository carries an existing `fork/HEAD` alias pointing to the retired `alice` branch. The same alias appears in the archive; removing it in the disposable verification copy produced a clean Git integrity check. [Administration result](/Users/alice/Archives/esp32s3/preservation-2026-09-07/git-administration-verification.json)

## Retained locations and dependencies

The [exclusion record](/Users/alice/Archives/esp32s3/preservation-2026-09-07/exclusions.json) identifies 91 browser runtime profiles, live agent communication state, the separate writing and product-description projects and preservation working files. They remain at their original locations. The cleanup manifest holds affected parent directories for review.

The two ROM links now resolve to the exact preserved ROM bytes. Four Python links identify the external uv-managed runtime; recreate those virtual environments with uv when restoring to another machine. [Dependency record](/Users/alice/Archives/esp32s3/preservation-2026-09-07/external-inputs.json)

The previously missing exception ELF remains a documented evidence gap. [Original retirement record](/Users/alice/Archives/esp32s3/preservation-2026-09-07/files/hardware/alice-retirement-2026-09-04/MANIFEST.md)

## Recovery and cleanup

Use the [archive guide](/Users/alice/Archives/esp32s3/preservation-2026-09-07/README.md) to restore a writable experiment directory and its Git state. The [preservation tool](../tools/preserve-experiments.mjs) provides hash-checked file and directory restoration.

The cleanup manifest contains 60 worktree records, 130 branch records and 285 path groups. Groups can overlap, so their logical byte totals should be read individually. Each record names its recovery location and review condition. Future cleanup requires selecting the active work, checking current use, resolving retained children and reviewing PR/native-stack relationships before branch retirement. [Cleanup manifest](/Users/alice/Archives/esp32s3/preservation-2026-09-07/cleanup-manifest.json)

