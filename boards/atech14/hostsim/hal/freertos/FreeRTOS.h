#pragma once
#include <cstdint>
typedef uint32_t TickType_t;
typedef int BaseType_t;
typedef unsigned int UBaseType_t;
#define pdTRUE 1
#define pdFALSE 0
#define pdPASS 1
#define portTICK_PERIOD_MS 1
#define portMAX_DELAY ((TickType_t)0xffffffffUL)
#define pdMS_TO_TICKS(ms) ((TickType_t)(ms))
#define configMINIMAL_STACK_SIZE 2048
