# CO5300 partial updates retain their window coordinates

ESP-IDF sends the display's column/row command headers and their four coordinate
bytes in separate SPI transfers. The CO5300 model discarded a header-only command,
so the following bytes could not update the window and strokes appeared in the
wrong part of the display. The model now retains pending `CASET` and `RASET`
commands until their parameters arrive.

The regression test sends split column and row transfers, then a RAM write and
continuation, and checks both the target pixels and the untouched origin.

In each replay, before and after the fix, the TinyDraw firmware accepted three
new strokes. The [broken screenshot](broken-after.png) shows misplaced drawing;
the [fixed screenshot](fixed-after.png) shows all three new strokes at their
intended positions. The existing upper-left multicolour patch is outside this
assertion. [The replay receipt](fixed-response.json) and
[input hashes](fixed-inputs.json) identify that diagnostic run. The replay does not prove that the settled display exactly matches the stored
drawing, and it does not measure hardware display latency.

The archived replay used the browser scheduler JIT work from PR #38. The display
parser fix itself is independent of that compiler change.
