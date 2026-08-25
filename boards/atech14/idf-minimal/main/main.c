/* Atech ESP32-S3 simulation firmware.
 *
 * Runs unchanged on real hardware and in Wokwi. Everything the simulator
 * asserts on is printed as a single-line "SIM:" tagged log so scenarios stay
 * robust against ordinary log noise.
 */
#include <stdio.h>
#include <string.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/queue.h"
#include "driver/gpio.h"
#include "esp_log.h"
#include "esp_chip_info.h"
#include "esp_flash.h"
#include "esp_mac.h"
#include "esp_event.h"
#include "esp_netif.h"
#include "esp_wifi.h"
#include "nvs_flash.h"
#include "sdkconfig.h"

static const char *TAG = "atech";

#define LED_GPIO    CONFIG_ATECH_LED_GPIO
#define BUTTON_GPIO CONFIG_ATECH_BUTTON_GPIO

static QueueHandle_t s_button_q;
static bool s_led_on;

static void set_led(bool on)
{
    s_led_on = on;
    gpio_set_level(LED_GPIO, on);
    ESP_LOGI(TAG, "SIM:LED:%s", on ? "ON" : "OFF");
}

static void IRAM_ATTR button_isr(void *arg)
{
    int level = gpio_get_level(BUTTON_GPIO);
    xQueueSendFromISR(s_button_q, &level, NULL);
}

static void button_task(void *arg)
{
    int level;
    int64_t last = 0;
    while (xQueueReceive(s_button_q, &level, portMAX_DELAY)) {
        int64_t now = xTaskGetTickCount();
        if (now - last < pdMS_TO_TICKS(30)) continue;   /* debounce */
        last = now;
        if (level == 0) {                                /* active low */
            ESP_LOGI(TAG, "SIM:BUTTON:PRESSED");
            set_led(!s_led_on);
        } else {
            ESP_LOGI(TAG, "SIM:BUTTON:RELEASED");
        }
    }
}

#if CONFIG_ATECH_WIFI_ENABLE
static void wifi_event(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_START) {
        esp_wifi_connect();
    } else if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        ESP_LOGW(TAG, "SIM:WIFI:DISCONNECTED");
        esp_wifi_connect();
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *e = data;
        ESP_LOGI(TAG, "SIM:WIFI:GOT_IP:" IPSTR, IP2STR(&e->ip_info.ip));
    }
}

static void wifi_start(void)
{
    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    esp_netif_create_default_wifi_sta();
    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&cfg));
    ESP_ERROR_CHECK(esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, wifi_event, NULL));
    ESP_ERROR_CHECK(esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, wifi_event, NULL));
    wifi_config_t wc = { 0 };
    strncpy((char *)wc.sta.ssid, CONFIG_ATECH_WIFI_SSID, sizeof(wc.sta.ssid));
    strncpy((char *)wc.sta.password, CONFIG_ATECH_WIFI_PASS, sizeof(wc.sta.password));
    wc.sta.threshold.authmode = strlen(CONFIG_ATECH_WIFI_PASS) ? WIFI_AUTH_WPA2_PSK : WIFI_AUTH_OPEN;
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &wc));
    ESP_ERROR_CHECK(esp_wifi_start());
    ESP_LOGI(TAG, "SIM:WIFI:CONNECTING:%s", CONFIG_ATECH_WIFI_SSID);
}
#endif

void app_main(void)
{
    esp_chip_info_t ci;
    esp_chip_info(&ci);
    uint32_t flash = 0;
    esp_flash_get_size(NULL, &flash);
    uint8_t mac[6];
    esp_read_mac(mac, ESP_MAC_WIFI_STA);
    ESP_LOGI(TAG, "SIM:BOOT:model=%d cores=%d rev=%d flash=%luMB mac=%02x:%02x:%02x:%02x:%02x:%02x",
             ci.model, ci.cores, ci.revision, (unsigned long)(flash / (1024 * 1024)),
             mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);

    ESP_ERROR_CHECK(nvs_flash_init());

    gpio_config_t led = {
        .pin_bit_mask = 1ULL << LED_GPIO,
        .mode = GPIO_MODE_OUTPUT,
    };
    gpio_config(&led);
    gpio_config_t btn = {
        .pin_bit_mask = 1ULL << BUTTON_GPIO,
        .mode = GPIO_MODE_INPUT,
        .pull_up_en = GPIO_PULLUP_ENABLE,
        .intr_type = GPIO_INTR_ANYEDGE,
    };
    gpio_config(&btn);
    s_button_q = xQueueCreate(8, sizeof(int));
    gpio_install_isr_service(0);
    gpio_isr_handler_add(BUTTON_GPIO, button_isr, NULL);
    xTaskCreate(button_task, "button", 3072, NULL, 5, NULL);
    set_led(false);

#if CONFIG_ATECH_WIFI_ENABLE
    wifi_start();
#endif

    ESP_LOGI(TAG, "SIM:READY");

    uint32_t tick = 0;
    while (1) {
        vTaskDelay(pdMS_TO_TICKS(5000));
        ESP_LOGI(TAG, "SIM:HEARTBEAT:%lu led=%d", (unsigned long)++tick, s_led_on);
    }
}
