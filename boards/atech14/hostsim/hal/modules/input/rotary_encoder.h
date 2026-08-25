// Host implementation of the Atech RotaryEncoder (knob + switch + 12-LED ring) — same public API.
#pragma once
#include <Arduino.h>
#include <atomic>
#include "../../../sim/board.h"

class RotaryEncoder : public EncoderHost {
public:
    static const uint8_t RING_LEDS = 12;
    static const int32_t DETENTS_PER_REV = 18;
    RotaryEncoder(int pinClk, int pinDt, int pinSw = -1, int pinRing = -1);
    void begin();
    void update();
    int32_t getPosition();
    void setPosition(int32_t pos);
    void resetPosition();
    int getDirection();
    bool wasRotatedCW();
    bool wasRotatedCCW();
    bool isPressed();
    bool wasPressed();
    void setAcceleration(bool enabled, int maxMultiplier = 5);
    void enableRing(bool enabled);
    void setRingColor(uint8_t r, uint8_t g, uint8_t b);
    void setRingBrightness(uint8_t brightness);
    void setRingPosition(float pos);
    // EncoderHost (driven by the UI / scenarios)
    void hostRotate(int detents) override;
    void hostSetPressed(bool down) override;
private:
    int _clk, _dt, _sw, _ringPin;
    std::atomic<int32_t> _position{0}, _raw{0};
    std::atomic<int> _lastDir{0};
    std::atomic<bool> _cw{false}, _ccw{false};
    bool _accel = false; int _accelMax = 5; unsigned long _lastStepUs = 0;
    std::atomic<bool> _swDown{false}; bool _lastBtn = false, _pressedFlag = false;
    RingState _ring; float _override = -1;
    void _updateRing();
};
