#pragma once
#include "FreeRTOS.h"
typedef void* TaskHandle_t;
typedef void (*TaskFunction_t)(void*);
BaseType_t xTaskCreatePinnedToCore(TaskFunction_t fn, const char* name, uint32_t stack, void* param,
                                   UBaseType_t prio, TaskHandle_t* handle, int core);
BaseType_t xTaskCreate(TaskFunction_t fn, const char* name, uint32_t stack, void* param,
                       UBaseType_t prio, TaskHandle_t* handle);
void vTaskDelay(TickType_t ticks);
void vTaskDelete(TaskHandle_t h);
TickType_t xTaskGetTickCount();
