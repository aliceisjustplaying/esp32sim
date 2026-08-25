#include "modules/display/st7735_tft.h"
#include "../sim/board.h"

ST7735_TFT::ST7735_TFT(int s, int c, int m, int d) : _sclk(s), _cs(c), _mosi(m), _dc(d) {}

void ST7735_TFT::begin() {
    _canvas = new GFXcanvas16(160, 80);
    _canvas->fillScreen(COLOR_BLACK);
    // splash, like the real driver
    _canvas->setTextColor(COLOR_WHITE); _canvas->setTextSize(2); _canvas->setCursor(34, 30); _canvas->print("atech");
    _push();
    // the real driver draws the splash on the panel and hands the user a fresh canvas
    delete _canvas; _canvas = new GFXcanvas16(160, 80);
    _canvas->fillScreen(COLOR_BLACK);
    Serial.println("ST7735 display initialized (160x80, double-buffered)");
}
void ST7735_TFT::clear() { _canvas->fillScreen(COLOR_BLACK); _canvas->setCursor(0, 0); _touched(); }
void ST7735_TFT::fillScreen(uint16_t c) { _canvas->fillScreen(c); _touched(); }
void ST7735_TFT::drawPixel(int16_t x, int16_t y, uint16_t c) { _canvas->drawPixel(x, y, c); _touched(); }
void ST7735_TFT::drawLine(int16_t a, int16_t b, int16_t c, int16_t d, uint16_t e) { _canvas->drawLine(a, b, c, d, e); _touched(); }
void ST7735_TFT::drawRect(int16_t a, int16_t b, int16_t c, int16_t d, uint16_t e) { _canvas->drawRect(a, b, c, d, e); _touched(); }
void ST7735_TFT::fillRect(int16_t a, int16_t b, int16_t c, int16_t d, uint16_t e) { _canvas->fillRect(a, b, c, d, e); _touched(); }
void ST7735_TFT::drawCircle(int16_t a, int16_t b, int16_t r, uint16_t e) { _canvas->drawCircle(a, b, r, e); _touched(); }
void ST7735_TFT::fillCircle(int16_t a, int16_t b, int16_t r, uint16_t e) { _canvas->fillCircle(a, b, r, e); _touched(); }
void ST7735_TFT::setDirectMode(bool d) { _direct = d; }
void ST7735_TFT::displayText(const char* t, int16_t x, int16_t y, uint8_t s, uint16_t c) {
    _canvas->setCursor(x, y); _canvas->setTextSize(s); _canvas->setTextColor(c); _canvas->print(t); _touched();
}
void ST7735_TFT::displayText(const String& t, int16_t x, int16_t y, uint8_t s, uint16_t c) { displayText(t.c_str(), x, y, s, c); }
void ST7735_TFT::display() { if (_direct) return; _push(); }
void ST7735_TFT::setCursor(int16_t x, int16_t y) { _canvas->setCursor(x, y); }
void ST7735_TFT::setTextColor(uint16_t c) { _canvas->setTextColor(c); }
void ST7735_TFT::setTextColor(uint16_t c, uint16_t bg) { _canvas->setTextColor(c, bg); }
void ST7735_TFT::setTextSize(uint8_t s) { _canvas->setTextSize(s); }
void ST7735_TFT::setFont(const GFXfont* f) { _canvas->setFont(f); }
void ST7735_TFT::setRotation(uint8_t r) { _canvas->setRotation(r); }
int16_t ST7735_TFT::width() { return _canvas->width(); }
int16_t ST7735_TFT::height() { return _canvas->height(); }
void ST7735_TFT::_touched() {
    if (!_direct) return;
    unsigned long now = millis();
    if (now - _lastPush >= 33) _push();
}
void ST7735_TFT::_push() {
    _lastPush = millis();
    VirtualBoard::get().pushFrame(_canvas->getBuffer(), _canvas->width(), _canvas->height());
}
