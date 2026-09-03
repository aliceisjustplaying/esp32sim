# hostsim — native build of the Atech firmware with a local web UI. No cloud, no accounts.
#   make            build ./build/hostsim
#   make run        build + start the web UI at http://127.0.0.1:8765
#   make test       run every scenarios/*.yaml headlessly (screenshots in build/screenshots)
CXX      ?= c++
FIRMWARE ?= ../firmware/src/main.cpp
BUILD    := build
LIB      := ../firmware/lib
INC      := -Ihal -Ithird_party/Adafruit_GFX -Ithird_party/ArduinoJson -I.. -I$(LIB)/sid -I$(LIB)/crsid -I$(LIB)/sidtunes
CXXFLAGS ?= -std=c++17 -O2 -g -Wall -Wno-unused-variable -Wno-unused-but-set-variable -Wno-sign-compare -Wno-deprecated-declarations $(INC) -DATECH_SIM=1 -DATECH_HOSTSIM=1 -DARDUINO=10819 -DARDUINOJSON_ENABLE_ARDUINO_STRING=0 -DARDUINOJSON_ENABLE_ARDUINO_STREAM=0 -DARDUINOJSON_ENABLE_ARDUINO_PRINT=0 -DARDUINOJSON_ENABLE_STD_STRING=1 -DARDUINOJSON_ENABLE_STD_STREAM=1
LDFLAGS  ?= -lz -lpthread -lm
CC       ?= cc
SRCS := hal/hal.cpp sim/board.cpp sim/ws_server.cpp sim/png.cpp sim/scenario.cpp sim/main.cpp \
        drivers/speaker.cpp drivers/st7735_tft.cpp drivers/rotary_encoder.cpp drivers/button.cpp \
        hal/modules/connectivity/serial_templates.cpp third_party/Adafruit_GFX/Adafruit_GFX.cpp
# the firmware's own C libraries: the SID chip engine, cRSID (a unity build: libcRSID.c includes the C64), the tunes
COBJS := $(BUILD)/lib/sid.o $(BUILD)/lib/crsid.o $(BUILD)/lib/sidtunes.o
OBJS := $(patsubst %.cpp,$(BUILD)/%.o,$(SRCS)) $(BUILD)/firmware_main.o $(COBJS)
SCENARIOS = $(sort $(wildcard scenarios/*.yaml))
STEP_TIMEOUT ?= 20000

all: $(BUILD)/hostsim

$(BUILD)/hostsim: $(OBJS)
	@mkdir -p $(BUILD)/web && cp web/index.html $(BUILD)/web/
	$(CXX) $(OBJS) -o $@ $(LDFLAGS)

$(BUILD)/%.o: %.cpp
	@mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(BUILD)/lib/sid.o: $(LIB)/sid/sid.c $(LIB)/sid/sid.h
	@mkdir -p $(dir $@)
	$(CC) -std=c11 -O2 -g -w $(INC) -c $< -o $@
$(BUILD)/lib/crsid.o: $(LIB)/crsid/libcRSID.c $(wildcard $(LIB)/crsid/C64/*.c $(LIB)/crsid/host/*.c)
	@mkdir -p $(dir $@)
	$(CC) -std=c11 -O2 -g -w $(INC) -DATECH_HOSTSIM=1 -c $< -o $@
$(BUILD)/lib/sidtunes.o: $(LIB)/sidtunes/sidtunes.c
	@mkdir -p $(dir $@)
	$(CC) -std=c11 -O2 -g -w $(INC) -c $< -o $@

# The generated firmware, compiled unchanged against the host HAL
$(BUILD)/firmware_main.o: $(FIRMWARE) $(wildcard hal/*.h hal/modules/*/*.h)
	@mkdir -p $(dir $@)
	cp $(FIRMWARE) $(BUILD)/firmware_main.cpp
	$(CXX) $(CXXFLAGS) -Wno-unused-function -Dregister= -c $(BUILD)/firmware_main.cpp -o $@   # cRSID is C89 (register)

run: all
	cd $(BUILD) && ./hostsim --port 8765

test: all
	@mkdir -p $(BUILD)/screenshots; fail=0; for s in $(SCENARIOS); do \
	  echo "=== $$s"; \
	  (cd $(BUILD) && ./hostsim --headless --quiet --step-timeout $(STEP_TIMEOUT) --screenshots screenshots --scenario ../$$s) || fail=1; \
	done; exit $$fail

clean:
	rm -rf $(BUILD)
.PHONY: all run test clean
