#pragma once
#include <Arduino.h>
#define ATECH_SIM_LOG(fmt, ...) Serial.printf("SIM:" fmt "\n", ##__VA_ARGS__)
