#!/bin/sh
set -eu
assets=/Users/alice/src/a/esp32sim/work/perf-pie-confirm/candidate/assets.json
for prices in '96 96' '96 160' '96 168' '160 160'; do
  set -- $prices
  node tools/approximate-jit-smoke.mjs "$assets" 1 64 3 2 2 1 "$1" "$2" > "work/receipts/ready-${1}-service-${2}-3s.json"
done
