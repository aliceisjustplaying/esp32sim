// Host implementations of the Arduino / FreeRTOS / Preferences surface.
#include "Arduino.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "Preferences.h"
#include "../sim/board.h"
#include <chrono>
#include <thread>
#include <cstdarg>
#include <fstream>
#include <sys/stat.h>

static const auto t0 = std::chrono::steady_clock::now();
unsigned long millis() { return (unsigned long)std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - t0).count(); }
unsigned long micros() { return (unsigned long)std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now() - t0).count(); }
void delay(unsigned long ms) { std::this_thread::sleep_for(std::chrono::milliseconds(ms)); }
void delayMicroseconds(unsigned int us) { std::this_thread::sleep_for(std::chrono::microseconds(us)); }
void yield() { std::this_thread::yield(); }

void pinMode(int pin, int mode) { VirtualBoard::get().pinMode(pin, mode); }
int digitalRead(int pin) { return VirtualBoard::get().pinLevel(pin); }
void digitalWrite(int pin, int level) { VirtualBoard::get().setPinLevel(pin, level); }
void attachInterruptArg(int pin, void (*fn)(void*), void* arg, int mode) { VirtualBoard::get().attachInterrupt(pin, fn, arg, mode); }

// ---- Serial
HostSerial Serial;
EspClass ESP;
int HostSerial::available() { return VirtualBoard::get().serialAvailable(); }
int HostSerial::read() { return VirtualBoard::get().serialRead(); }
int HostSerial::peek() { return VirtualBoard::get().serialPeek(); }
size_t HostSerial::write(uint8_t c) { char ch = (char)c; VirtualBoard::get().serialOut(&ch, 1); return 1; }
size_t HostSerial::write(const uint8_t* buf, size_t n) { VirtualBoard::get().serialOut((const char*)buf, n); return n; }
void HostSerial::hostInject(const std::string& line) { VirtualBoard::get().serialIn(line); }
void EspClass::restart() { fprintf(stderr, "[hostsim] ESP.restart() requested — exiting\n"); exit(3); }

size_t Print::printf(const char* fmt, ...) {
    char stackBuf[512];
    va_list ap; va_start(ap, fmt);
    va_list ap2; va_copy(ap2, ap);
    int n = vsnprintf(stackBuf, sizeof stackBuf, fmt, ap);
    va_end(ap);
    if (n < 0) { va_end(ap2); return 0; }
    if ((size_t)n < sizeof stackBuf) { va_end(ap2); return write((const uint8_t*)stackBuf, n); }
    std::string big((size_t)n + 1, '\0');
    vsnprintf(&big[0], big.size(), fmt, ap2); va_end(ap2);
    return write((const uint8_t*)big.data(), n);
}

// ---- FreeRTOS tasks -> threads
BaseType_t xTaskCreatePinnedToCore(TaskFunction_t fn, const char*, uint32_t, void* param, UBaseType_t, TaskHandle_t* handle, int) {
    std::thread t([fn, param] { fn(param); });
    if (handle) *handle = (TaskHandle_t)t.native_handle();
    t.detach();
    return pdPASS;
}
BaseType_t xTaskCreate(TaskFunction_t fn, const char* n, uint32_t s, void* p, UBaseType_t pr, TaskHandle_t* h) { return xTaskCreatePinnedToCore(fn, n, s, p, pr, h, 0); }
void vTaskDelay(TickType_t ticks) {
    if (ticks == portMAX_DELAY) { while (true) std::this_thread::sleep_for(std::chrono::hours(1)); }
    std::this_thread::sleep_for(std::chrono::milliseconds(ticks));
}
void vTaskDelete(TaskHandle_t) { /* only ever called for "self" in drivers; thread simply returns */ }
TickType_t xTaskGetTickCount() { return (TickType_t)millis(); }

// ---- Preferences -> key=value file under $ATECH_HOSTSIM_PREFS (default ./.prefs)
static std::string prefsDir() {
    const char* d = getenv("ATECH_HOSTSIM_PREFS");
    std::string dir = d ? d : ".prefs";
    mkdir(dir.c_str(), 0755);
    return dir;
}
bool Preferences::begin(const char* ns, bool, const char*) {
    _path = prefsDir() + "/" + ns + ".txt";
    _kv.clear();
    std::ifstream f(_path);
    std::string line;
    while (std::getline(f, line)) { auto p = line.find('='); if (p != std::string::npos) _kv[line.substr(0, p)] = line.substr(p + 1); }
    return true;
}
void Preferences::save() { std::ofstream f(_path); for (auto& kv : _kv) f << kv.first << "=" << kv.second << "\n"; _dirty = false; }
void Preferences::end() { if (_dirty) save(); }
bool Preferences::clear() { _kv.clear(); _dirty = true; return true; }
bool Preferences::isKey(const char* k) { return _kv.count(k) > 0; }
uint32_t Preferences::getUInt(const char* k, uint32_t d) { auto it = _kv.find(k); return it == _kv.end() ? d : (uint32_t)strtoul(it->second.c_str(), nullptr, 10); }
size_t Preferences::putUInt(const char* k, uint32_t v) { _kv[k] = std::to_string(v); _dirty = true; save(); return 4; }
int32_t Preferences::getInt(const char* k, int32_t d) { auto it = _kv.find(k); return it == _kv.end() ? d : (int32_t)strtol(it->second.c_str(), nullptr, 10); }
size_t Preferences::putInt(const char* k, int32_t v) { _kv[k] = std::to_string(v); _dirty = true; save(); return 4; }
float Preferences::getFloat(const char* k, float d) { auto it = _kv.find(k); return it == _kv.end() ? d : strtof(it->second.c_str(), nullptr); }
size_t Preferences::putFloat(const char* k, float v) { _kv[k] = std::to_string(v); _dirty = true; save(); return 4; }
std::string Preferences::getString(const char* k, const std::string& d) { auto it = _kv.find(k); return it == _kv.end() ? d : it->second; }
size_t Preferences::putString(const char* k, const std::string& v) { _kv[k] = v; _dirty = true; save(); return v.size(); }
