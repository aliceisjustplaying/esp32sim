# Atech ESP32-S3 (14-port board) "Pocket Synth" — simulation environment
#
#   make hw         build the hardware firmware (env:hw — real I2S, real ST7735)
#   make test       run the emulator's golden tests for this firmware (needs the S3 mask ROM ELF)
#   make flash      flash the real board          make monitor   serial monitor
#   make sync-sdk   re-copy the real drivers from the installed `atech` SDK
#   make sdk        (re)create .venv with the open Atech SDK (pip package `atech`)
#   make check      `atech check` — reboot the real board and report module health
#
# Needs: platformio (`pio`), uv

PORT     ?= /dev/cu.usbmodem1101
export PATH := $(HOME)/bin:$(PATH)
ATECH     = .venv/bin/atech

.PHONY: test hw flash monitor check send clean sdk sync-sdk idf-example

test:
	cd ../.. && cargo test --release -p esp32sim --test goldens -- --include-ignored atech

hw:
	cd firmware && pio run -e hw

flash: hw
	cd firmware && pio run -e hw -t upload --upload-port $(PORT)

monitor:
	cd firmware && pio device monitor -p $(PORT) -b 115200

check:
	$(ATECH) check --port $(PORT)

# usage: make send KEY=set_note VALUE=5
send:
	$(ATECH) send --port $(PORT) $(KEY) '$(VALUE)'

sdk:
	uv venv .venv -q && uv pip install -q --python .venv/bin/python atech

sync-sdk:
	tools/sync-sdk-modules.sh

clean:
	rm -rf firmware/.pio

# The minimal ESP-IDF sample from before the Atech firmware existed
idf-example:
	. $(HOME)/esp/esp-idf-v5.5.4/export.sh >/dev/null && cd examples/idf-minimal && idf.py build
