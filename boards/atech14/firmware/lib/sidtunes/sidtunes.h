#pragma once
#include <stdint.h>
#include <stddef.h>

typedef struct { const char *name; const uint8_t *data; size_t len; } sid_tune_t;

extern const sid_tune_t SID_TUNES[];
extern const int SID_TUNE_COUNT;
