#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

boot_build="${TINYDRAW_IDF61_BOOT_RECEIPT_BUILD:?TINYDRAW_IDF61_BOOT_RECEIPT_BUILD must name the pinned measured-boot receipt build}"
exception_build="${TINYDRAW_IDF61_EXCEPTION_RECEIPT_BUILD:?TINYDRAW_IDF61_EXCEPTION_RECEIPT_BUILD must name the pinned exception receipt build}"
rom_elf="${ESP32S3_ROM_ELF:?ESP32S3_ROM_ELF must name the pinned ESP32-S3 ROM ELF}"

TINYDRAW_IDF61_RECEIPT_BUILD="$boot_build" \
ESP32S3_ROM_ELF="$rom_elf" \
    cargo test -p esp32sim --test measured_boot --locked -- \
        --ignored --exact real_tinydraw_measured_boot_stops_deterministically_at_committed_outcome

TINYDRAW_IDF61_RECEIPT_BUILD="$exception_build" \
    cargo test -p esp32s3 --test exception_derivation --locked -- \
        --ignored --exact real_idf61_exception_paths_expose_the_incomplete_known_ledgers
