#!/bin/sh
set -eu
for extra in 0 1 8; do
  sh receipts/run-tinydraw.sh --approximate-memory "$extra" --memory-contention --max-insns 100000000 > "receipts/memory-${extra}.log" 2>&1
done
