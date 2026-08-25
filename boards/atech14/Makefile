# Atech ESP32-S3 (14-port board) "Pocket Synth" — simulation environment
#
#   make hostsim    build the LOCAL simulator (native build of the firmware, no cloud)
#   make run        start the local simulator's web UI at http://127.0.0.1:8765
#   make test       run every scenario in sim/scenarios against the local simulator
#   make build      build the Wokwi firmware (PlatformIO env:sim)   [optional, needs Wokwi token]
#   make wokwi-test run the scenarios in Wokwi's cloud simulator      [optional, needs Wokwi token]
#   make sim        interactive Wokwi run                             [optional, needs Wokwi token]
#   make hw         build the hardware firmware (env:hw — real I2S, real ST7735)
#   make flash      flash the real board          make monitor   serial monitor
#   make sync-sdk   re-copy the real drivers from the installed `atech` SDK
#   make sdk        (re)create .venv with the open Atech SDK (pip package `atech`)
#   make check      `atech check` — reboot the real board and report module health
#
# Needs: platformio (`pio`), uv, wokwi-cli, WOKWI_CLI_TOKEN (https://wokwi.com/dashboard/ci)

PORT     ?= /dev/cu.usbmodem1101
export PATH := $(HOME)/.wokwi/bin:$(HOME)/bin:$(PATH)
SCENARIOS = $(sort $(wildcard sim/scenarios/*.yaml))
TIMEOUT  ?= 120000
ATECH     = .venv/bin/atech

.PHONY: hostsim run test wokwi-test build hw sim lint flash monitor check send clean sdk sync-sdk install-wokwi-cli idf-example

hostsim:
	$(MAKE) -C hostsim

run: hostsim
	$(MAKE) -C hostsim run

test: hostsim
	$(MAKE) -C hostsim test

build:
	cd firmware && pio run -e sim

hw:
	cd firmware && pio run -e hw

sim: build
	cd sim && wokwi-cli --interactive --timeout 0

wokwi-test: build
	@mkdir -p sim/screenshots; fail=0; for s in $(SCENARIOS); do \
	  echo "=== $$s"; \
	  (cd sim && wokwi-cli --timeout $(TIMEOUT) --scenario ../$$s \
	      --fail-text "Guru Meditation" --fail-text "Task watchdog got triggered" --fail-text "assert failed") || fail=1; \
	done; exit $$fail

lint:
	cd sim && wokwi-cli lint

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
	rm -rf firmware/.pio sim/screenshots; $(MAKE) -C hostsim clean

install-wokwi-cli:
	curl -fsSL https://wokwi.com/ci/install.sh | sh   # installs to ~/.wokwi/bin

# The minimal ESP-IDF sample from before the Atech firmware existed
idf-example:
	. $(HOME)/esp/esp-idf-v5.5.4/export.sh >/dev/null && cd examples/idf-minimal && idf.py build
