#pragma once
#include <cstdint>
#include <cstddef>
#include <string>

class __FlashStringHelper;

class Print {
public:
    virtual ~Print() {}
    virtual size_t write(uint8_t c) = 0;
    virtual size_t write(const uint8_t* buf, size_t n) { size_t k = 0; for (size_t i = 0; i < n; i++) k += write(buf[i]); return k; }
    size_t write(const char* s) { return s ? write((const uint8_t*)s, strlen(s)) : 0; }
    size_t write(const char* s, size_t n) { return write((const uint8_t*)s, n); }

    size_t print(const char* s) { return write(s); }
    size_t print(const std::string& s) { return write((const uint8_t*)s.data(), s.size()); }
    size_t print(char c) { return write((uint8_t)c); }
    size_t print(int v) { return printf("%d", v); }
    size_t print(unsigned int v) { return printf("%u", v); }
    size_t print(long v) { return printf("%ld", v); }
    size_t print(unsigned long v) { return printf("%lu", v); }
    size_t print(long long v) { return printf("%lld", v); }
    size_t print(unsigned long long v) { return printf("%llu", v); }
    size_t print(double v, int digits = 2) { return printf("%.*f", digits, v); }
    size_t print(float v, int digits = 2) { return printf("%.*f", digits, (double)v); }
    size_t print(bool v) { return write(v ? "true" : "false"); }
    size_t print(const __FlashStringHelper* s) { return write((const char*)s); }

    size_t println() { return write("\r\n"); }
    template <class T> size_t println(T v) { size_t n = print(v); return n + println(); }
    template <class T> size_t println(T v, int d) { size_t n = print(v, d); return n + println(); }

    size_t printf(const char* fmt, ...) __attribute__((format(printf, 2, 3)));
};
