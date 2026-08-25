// Stand-in for the hosted platform's modules/shared/atech_helpers.h (not part of the
// open SDK). NOTE_* constants come from the real speaker.h. Only sim helpers live here.
#pragma once
#include <Arduino.h>
#define ATECH_SIM_LOG(fmt, ...) Serial.printf("SIM:" fmt "\n", ##__VA_ARGS__)
