#include "modules/input/rotary_encoder.h"

RotaryEncoder::RotaryEncoder(int clk, int dt, int sw, int ring) : _clk(clk), _dt(dt), _sw(sw), _ringPin(ring) {}

void RotaryEncoder::begin() {
    pinMode(_clk, INPUT_PULLUP); pinMode(_dt, INPUT_PULLUP);
    if (_sw >= 0) pinMode(_sw, INPUT_PULLUP);
    VirtualBoard::get().registerEncoder(this);
    if (_ringPin >= 0) {
        _ring.enabled = true; _ring.bright = 50; _ring.leds = RING_LEDS;
        VirtualBoard::get().setRing(_ring);
        Serial.print("[RotaryEncoder] Ring initialized on pin "); Serial.println(_ringPin);
    }
    _lastStepUs = micros();
}

void RotaryEncoder::hostRotate(int detents) {
    if (!detents) return;
    int dir = detents > 0 ? 1 : -1;
    for (int i = 0; i < std::abs(detents); i++) {
        unsigned long now = micros(), elapsed = now - _lastStepUs; _lastStepUs = now;
        int mult = 1;
        if (_accel) {   // same shape as the real driver: fast turns count more
            if (elapsed < 20000) mult = _accelMax; else if (elapsed < 50000) mult = std::max(1, _accelMax / 2);
        }
        _position += dir * mult; _raw += dir; _lastDir = dir;
        if (dir > 0) _cw = true; else _ccw = true;
    }
    _updateRing();
}
void RotaryEncoder::hostSetPressed(bool down) { _swDown = down; if (_sw >= 0) VirtualBoard::get().setPinLevel(_sw, down ? 0 : 1); }

void RotaryEncoder::update() {
    bool b = isPressed();
    if (b && !_lastBtn) _pressedFlag = true;
    _lastBtn = b;
}
int32_t RotaryEncoder::getPosition() { return _position; }
void RotaryEncoder::setPosition(int32_t p) { _position = p; _raw = p; _updateRing(); }
void RotaryEncoder::resetPosition() { setPosition(0); }
int RotaryEncoder::getDirection() { return _lastDir; }
bool RotaryEncoder::wasRotatedCW() { return _cw.exchange(false); }
bool RotaryEncoder::wasRotatedCCW() { return _ccw.exchange(false); }
bool RotaryEncoder::isPressed() { return _sw >= 0 ? digitalRead(_sw) == LOW : _swDown.load(); }
bool RotaryEncoder::wasPressed() { update(); bool r = _pressedFlag; _pressedFlag = false; return r; }
void RotaryEncoder::setAcceleration(bool e, int m) { _accel = e; _accelMax = std::max(1, m); }
void RotaryEncoder::enableRing(bool e) { _ring.enabled = e; VirtualBoard::get().setRing(_ring); }
void RotaryEncoder::setRingColor(uint8_t r, uint8_t g, uint8_t b) { _ring.r = r; _ring.g = g; _ring.b = b; VirtualBoard::get().setRing(_ring); }
void RotaryEncoder::setRingBrightness(uint8_t b) { _ring.bright = b; VirtualBoard::get().setRing(_ring); }
void RotaryEncoder::setRingPosition(float p) { _override = p; _updateRing(); }
void RotaryEncoder::_updateRing() {
    float pos = _override >= 0 ? _override : (float)(((_raw.load() % DETENTS_PER_REV) + DETENTS_PER_REV) % DETENTS_PER_REV) * RING_LEDS / DETENTS_PER_REV;
    _ring.pos = pos;
    VirtualBoard::get().setRing(_ring);
}
