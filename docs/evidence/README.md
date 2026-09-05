# Benchmark evidence retention

Keep enough evidence in the source tree to review a claim and reproduce its workload.
Commit a README describing the result and its limits, compact result/comparison JSON,
input and source hashes, environment details, and small reproduction scripts or summaries.
Keep individual evidence files below 50 KB; split results by run when useful, rather
than splitting raw captures merely to fit this limit.

Save raw event streams, per-block rows, CPU profiles, full console logs, screenshots
and videos under an ignored output directory such as `target/`. When a claim needs
those captures, attach them to a release in the contributor's fork or retain them in
a separate evidence repository. Link the artifact and record its SHA-256, byte size,
source revision and reproduction command in the committed summary. Verify access
before removing the only available copy. Test fixtures required by automated tests
belong with the tests and are outside this evidence policy.

Results should retain timings, pass/fail outcomes, instruction counts, run order and
measurement limitations. Remove embedded logs and frame/event arrays from compact
results; identify omitted fields and link the complete original. Screenshots can
support visual claims without being committed to the source repository.

Existing captures already pushed in the browser JIT stack remain accessible at the
immutable revisions listed in the [capture archive](ARCHIVE.md). This is a transition
for existing evidence, not the storage location for future captures. No captures have
been uploaded to a release by this cleanup. Removing files reduces the current tree;
it does not reclaim their bytes from existing Git history.
