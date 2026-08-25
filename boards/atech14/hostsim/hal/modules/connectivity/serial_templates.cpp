// Host AtechSerial — same wire format as the firmware version (SDK envelope).
#include "modules/connectivity/serial_templates.h"
#include <ArduinoJson.h>

void AtechSerial::connect() {}
void AtechSerial::maintain() {
    while (Serial.available()) {
        char c = (char)Serial.read();
        if (c == '\n' || c == '\r') { if (_rx.length()) handleLine(_rx.c_str()); _rx = ""; }
        else if (_rx.length() < 2048) _rx += c;
    }
}
void AtechSerial::handleLine(const char* line) {
    JsonDocument d;
    if (deserializeJson(d, line) != DeserializationError::Ok || !d.is<JsonObject>()) return;
    const char* action = d["action"];
    if (!action || !_handler) return;
    std::string value;
    JsonVariant v = d["value"];
    if (v.is<const char*>()) value = v.as<const char*>();
    else if (!v.isNull()) serializeJson(v, value);
    _handler(action, value.c_str());
}
void AtechSerial::postStateEvent(const char* name, const char* value) {
    Serial.printf("{\"type\":\"event\",\"payload\":{\"event_type\":\"state\",\"key\":\"%s\",\"value\":\"%s\",\"source\":\"user\"}}\n", name, value);
}
void AtechSerial::postButtonEvent(const char* name, int value) {
    Serial.printf("{\"type\":\"event\",\"payload\":{\"event_type\":\"button\",\"key\":\"%s\",\"value\":%d,\"source\":\"user\"}}\n", name, value);
}
void AtechSerial::postSensorEventInt(const char* name, int32_t value, const char* source) {
    Serial.printf("{\"type\":\"event\",\"payload\":{\"event_type\":\"sensor\",\"key\":\"%s\",\"value\":%ld,\"source\":\"%s\"}}\n", name, (long)value, source);
}
