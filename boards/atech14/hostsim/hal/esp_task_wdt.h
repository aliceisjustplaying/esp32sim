#pragma once
#include <cstdint>
typedef int esp_err_t;
#define ESP_OK 0
inline esp_err_t esp_task_wdt_init(uint32_t, bool) { return ESP_OK; }
inline esp_err_t esp_task_wdt_add(void*) { return ESP_OK; }
inline esp_err_t esp_task_wdt_reset() { return ESP_OK; }
inline esp_err_t esp_task_wdt_delete(void*) { return ESP_OK; }
