# ESP32-S3 DMA-on-SRAM adoption

Two clean ESP-IDF v6.1 boots completed all five cells with 100 samples per
cell and zero refusals. Every copy trial used the verified 32 KiB IRAM copy
kernel while the SPI2 DMA transaction remained in flight.

The paired active-minus-idle PSRAM-to-SRAM copy medians are 3.5 and 0 cycles
per 32 KiB copy. This adopts an exact zero additive CPU slowdown within the
observed 0 to 3.5-cycle range.

At quad 40 MHz, the 32 KiB SPI2 submit-to-complete median is exactly 401,589
cycles in both boots and submit-only is exactly 5,755. These are sequence
correlation targets under price-table rule R2, not prices.

Verify with:

```text
python3 analyze.py
git diff --exit-code summary.json
shasum -a 256 -c SHA256SUMS
```
