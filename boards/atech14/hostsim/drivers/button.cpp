#include "modules/input/button.h"
#include "../sim/board.h"

ButtonModule::ButtonModule(int pin, bool activeLow) : _pin(pin), _activeLow(activeLow) {}
void ButtonModule::begin() {
    pinMode(_pin, INPUT_PULLUP);
    VirtualBoard::get().registerButton(_pin, "GPIO" + std::to_string(_pin));
    _currentState = isPressed(); _lastState = _currentState;
}
bool ButtonModule::isPressed() { int raw = digitalRead(_pin); return _activeLow ? raw == LOW : raw == HIGH; }
bool ButtonModule::wasPressed() { update(); if (_pressedFlag) { _pressedFlag = false; return true; } return false; }
bool ButtonModule::wasReleased() { update(); if (_releasedFlag) { _releasedFlag = false; return true; } return false; }
void ButtonModule::update() {
    bool r = isPressed();
    if (r != _currentState) { _currentState = r; if (r) _pressedFlag = true; else _releasedFlag = true; }
}
int ButtonModule::getState() { return isPressed() ? 1 : 0; }
