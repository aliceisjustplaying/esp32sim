// Host (macOS/Linux) stand-in for the Arduino-ESP32 core API used by Atech firmware.
#pragma once
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <string>
#include <algorithm>

#ifndef ARDUINO
#define ARDUINO 10819
#endif
#define PROGMEM
#define PSTR(s) (s)
#define F(s) (s)
#define pgm_read_byte(a)    (*(const uint8_t*)(a))
#define pgm_read_word(a)    (*(const uint16_t*)(a))
#define pgm_read_dword(a)   (*(const uint32_t*)(a))
#define IRAM_ATTR
#define HIGH 1
#define LOW 0
#define INPUT 0
#define OUTPUT 1
#define INPUT_PULLUP 2
#define INPUT_PULLDOWN 3
#define CHANGE 1
#define FALLING 2
#define RISING 3
#define digitalPinToInterrupt(p) (p)

class __FlashStringHelper;
using String = std::string;   // enough for the subset the firmware and GFX use

template <class T> T constrain(T v, T lo, T hi) { return v < lo ? lo : (v > hi ? hi : v); }
inline long map(long x, long a, long b, long c, long d) { return (x - a) * (d - c) / (b - a) + c; }

unsigned long millis();
unsigned long micros();
void delay(unsigned long ms);
void delayMicroseconds(unsigned int us);
void yield();

void pinMode(int pin, int mode);
int digitalRead(int pin);
void digitalWrite(int pin, int level);
void attachInterruptArg(int pin, void (*fn)(void*), void* arg, int mode);

#include "Print.h"

class HostSerial : public Print {
public:
    void begin(unsigned long) {}
    void setRxBufferSize(size_t) {}
    void setTxTimeoutMs(uint32_t) {}
    int available();
    int read();
    int peek();
    size_t write(uint8_t c) override;
    size_t write(const uint8_t* buf, size_t n) override;
    // host side: feed input, receive output lines
    void hostInject(const std::string& line);
};
extern HostSerial Serial;

class EspClass {
public:
    uint32_t getFreeHeap() { return 310000; }
    uint32_t getMinFreeHeap() { return 280000; }
    uint32_t getCpuFreqMHz() { return 240; }
    uint32_t getPsramSize() { return 2 * 1024 * 1024; }
    void restart();
};
extern EspClass ESP;

#define DEG_TO_RAD 0.017453292519943295769236907684886
#define RAD_TO_DEG 57.295779513082320876798154814105
#define radians(deg) ((deg) * DEG_TO_RAD)
#define degrees(rad) ((rad) * RAD_TO_DEG)
#define sq(x) ((x) * (x))
