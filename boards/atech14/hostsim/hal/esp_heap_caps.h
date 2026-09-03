// Host stand-in for ESP-IDF's capability-aware heap: there is one heap here.
#pragma once
#include <stdlib.h>
#define MALLOC_CAP_SPIRAM 0
#define MALLOC_CAP_8BIT 0
static inline void* heap_caps_calloc(size_t n, size_t size, int caps) { (void)caps; return calloc(n, size); }
static inline void heap_caps_free(void* p) { free(p); }
