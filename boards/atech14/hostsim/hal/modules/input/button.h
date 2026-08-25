// Host implementation of the Atech ButtonModule — same public API and edge semantics as the SDK driver.
#pragma once
#include <Arduino.h>
class ButtonModule {
public:
    ButtonModule(int pin, bool activeLow = true);
    void begin();
    bool isPressed();
    bool wasPressed();
    bool wasReleased();
    void update();
    int getState();
private:
    int _pin; bool _activeLow;
    bool _lastState = false, _currentState = false, _pressedFlag = false, _releasedFlag = false;
};
