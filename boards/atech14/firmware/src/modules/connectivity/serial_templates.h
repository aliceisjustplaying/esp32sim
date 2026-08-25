// Stand-in for Atech's closed "AtechSerial" USB-serial transport.
//
// Wire format: the open SDK's envelope (atech/runtime/transport.py) — see serial_templates.cpp.
//   host -> device : one JSON object per line   {"action":"set_note","value":9}
//                    "value" may be a string, number, or object; objects are
//                    re-serialised and handed to the callback as a string, which is
//                    what handleMessage() in main.cpp expects.
//   device -> host : one JSON object per line   {"event":"state","name":"waveform","value":"TRIANGLE"}
#pragma once
#include <Arduino.h>

typedef void (*AtechMessageHandler)(const char* action, const char* value);

class AtechSerial {
public:
    explicit AtechSerial(unsigned long baud) : _baud(baud) {}
    void connect();                    // "Serial.begin(115200) — always succeeds"
    void maintain();                   // poll: parse incoming lines, dispatch to handler
    void onMessage(AtechMessageHandler h) { _handler = h; }

    void postStateEvent(const char* name, const char* value);
    void postButtonEvent(const char* name, int value);
    void postSensorEventInt(const char* name, int32_t value, const char* source);

private:
    void handleLine(const char* line);
    unsigned long _baud;
    AtechMessageHandler _handler = nullptr;
    String _rx;
};
