# Gate-harness fast-mode rerun, 2026-09-03

The requested current-main gate-harness rerun did not start. The verifier
failed closed before emulator execution because no local ELF matched the
identity pinned by the current contract.

The contract requires TinyDraw commit
`7a157d44a9da3312b1ecda2b45b116af2de28e63`, ELF SHA-256
`1d67c35762fe58b72202a19b1c06912f0b9503a7331ba881cda3928648b54cd6`,
and sdkconfig SHA-256
`7490046d6e8b00d80f2bb550439821fa9d4a50da762e6e46d2aa9bdf8d520b8b`.
No file with the required ELF hash was found among 207 ELF
candidates under `~/Archives/esp32s3`, the read-only canonical TinyDraw
checkout, `/private/tmp`, or the actual `$TMPDIR`, `/Users/sarah/tmp`. The
committed [`audit_fixture.py`](audit_fixture.py) reproduces the search, and
[`fixture-search.json`](fixture-search.json) records its complete output.

The similarly named archived build at
`~/Archives/esp32s3/evidence-bytes/lane0-idf61-outputs/idf61/esp32-gate-harness`
was rejected. Its project metadata identifies TinyDraw `632c966`, its ELF
hash is `4e121a36...`, and its sdkconfig hash is `44c7f88a...`. Those bytes do
not satisfy the current-main contract and were not executed.

Therefore the post-fix scheduling horizon, all `TINYDRAW_LIVE_*` marker
counts, their counter fields, and the terminal marker remain unknown. This
receipt adopts no timing value and makes no claim about whether the small
SPI2 DMA completion fix lets the gate workload finish.

[`result.json`](result.json) records the exact expected and observed hashes,
the verifier result, the search scope, and the limits. The current contract
pins only the application ELF and sdkconfig. It does not authoritatively pin
the app binary, bootloader, partition table, or auto-discovered ROM ELF, even
though the dry-run executes those bytes. A valid rerun requires an
authoritative receipt that pins every external binary input, restoration of
those exact immutable bytes, and verification of all of them before execution.
Rebuilding TinyDraw is outside this repository task.

The ignored TinyDraw Wi-Fi credential values were checked byte-for-byte
against this evidence directory before commit. Neither value is present. No
firmware artifact is copied into the repository.
