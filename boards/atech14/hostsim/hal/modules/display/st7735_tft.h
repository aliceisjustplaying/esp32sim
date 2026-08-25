// Host implementation of the Atech ST7735_TFT (160x80) — same public API as the SDK driver.
// Canvas mode renders into a GFXcanvas16 exactly like the device; display() ships the frame
// to the VirtualBoard (→ browser / screenshots).
#pragma once
#include <Arduino.h>
#include <Adafruit_GFX.h>

class ST7735_TFT {
public:
    static constexpr uint16_t COLOR_BLACK = 0x0000, COLOR_WHITE = 0xFFFF, COLOR_RED = 0xF800,
                              COLOR_GREEN = 0x07E0, COLOR_BLUE = 0x001F, COLOR_CYAN = 0x07FF,
                              COLOR_MAGENTA = 0xF81F, COLOR_YELLOW = 0xFFE0, COLOR_ORANGE = 0xFD20;
    ST7735_TFT(int sclkPin, int csPin, int mosiPin, int dcPin);
    void begin();
    void clear();
    void fillScreen(uint16_t color);
    void drawPixel(int16_t x, int16_t y, uint16_t color);
    void drawLine(int16_t x0, int16_t y0, int16_t x1, int16_t y1, uint16_t color);
    void drawRect(int16_t x, int16_t y, int16_t w, int16_t h, uint16_t color);
    void fillRect(int16_t x, int16_t y, int16_t w, int16_t h, uint16_t color);
    void drawCircle(int16_t x, int16_t y, int16_t r, uint16_t color);
    void fillCircle(int16_t x, int16_t y, int16_t r, uint16_t color);
    void setDirectMode(bool direct);
    template <typename T> void print(T value) { _canvas->print(value); _touched(); }
    void print(float value, int decimals) { _canvas->print(value, decimals); _touched(); }
    void print(double value, int decimals) { _canvas->print(value, decimals); _touched(); }
    template <typename T> void println(T value) { _canvas->println(value); _touched(); }
    void println(float value, int decimals) { _canvas->println(value, decimals); _touched(); }
    void println(double value, int decimals) { _canvas->println(value, decimals); _touched(); }
    void displayText(const char* text, int16_t x, int16_t y, uint8_t size, uint16_t color);
    void displayText(const String& text, int16_t x, int16_t y, uint8_t size, uint16_t color);
    void display();
    void setCursor(int16_t x, int16_t y);
    void setTextColor(uint16_t color);
    void setTextColor(uint16_t color, uint16_t bg);
    void setTextSize(uint8_t size);
    void setFont(const GFXfont* font);
    void setRotation(uint8_t r);
    int16_t width();
    int16_t height();
private:
    int _sclk, _cs, _mosi, _dc;
    GFXcanvas16* _canvas = nullptr;
    bool _direct = false;
    unsigned long _lastPush = 0;
    void _touched();          // direct mode: push frames (throttled)
    void _push();
};
