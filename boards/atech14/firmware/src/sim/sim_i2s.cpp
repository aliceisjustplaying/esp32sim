// Simulated I2S back-end for Wokwi (see sim_i2s_shim.h). Consumes what the real
// speaker driver writes — 16-bit stereo frames — and reports what a listener
// would hear, once per ~46 ms of audio:
//     SIM:AUDIO:note=A4 f=441 rms=0.28
// Timing is preserved: a write "takes" as long as the audio it carries.
#ifdef ATECH_SIM
#include <Arduino.h>
#include <math.h>
#include <driver/i2s.h>          // the macros renamed the declarations to sim_* — we define those
#include "modules/shared/atech_helpers.h"

static uint32_t s_rate = 44100;
static uint32_t s_winSamples = 0, s_winCrossings = 0;
static double   s_winEnergy = 0;
static int16_t  s_prev = 0;
static const uint32_t WINDOW = 2048;

static void analyse(int16_t v) {
    if (s_prev < 0 && v >= 0) s_winCrossings++;
    s_prev = v;
    s_winEnergy += (double)v * v;
    if (++s_winSamples >= WINDOW) {
        float rms = sqrtf((float)(s_winEnergy / s_winSamples)) / 32768.0f;
        float f = (float)s_winCrossings * s_rate / s_winSamples;
        if (rms > 0.005f && f > 20.0f) {
            static const char* NAMES[12] = {"C","C#","D","D#","E","F","F#","G","G#","A","A#","B"};
            int semis = (int)lroundf(12.0f * log2f(f / 440.0f));      // relative to A4
            int idx = ((semis + 9) % 12 + 12) % 12;
            int oct = 4 + (int)floorf((semis + 9) / 12.0f);
            ATECH_SIM_LOG("AUDIO:note=%s%d f=%d rms=%.2f", NAMES[idx], oct, (int)f, rms);
        }
        s_winSamples = 0; s_winCrossings = 0; s_winEnergy = 0;
    }
}

extern "C" {
esp_err_t sim_i2s_driver_install(i2s_port_t port, const i2s_config_t* cfg, int, void*) {
    s_rate = cfg ? cfg->sample_rate : 44100;
    ATECH_SIM_LOG("AUDIO:init port=%d rate=%lu bits=%d", (int)port, (unsigned long)s_rate, cfg ? (int)cfg->bits_per_sample : 0);
    return ESP_OK;
}
esp_err_t sim_i2s_driver_uninstall(i2s_port_t) { return ESP_OK; }
esp_err_t sim_i2s_set_pin(i2s_port_t port, const i2s_pin_config_t* p) {
    if (p) ATECH_SIM_LOG("AUDIO:pins port=%d bclk=%d lrclk=%d dout=%d", (int)port, p->bck_io_num, p->ws_io_num, p->data_out_num);
    return ESP_OK;
}
esp_err_t sim_i2s_set_sample_rates(i2s_port_t, uint32_t rate) { s_rate = rate; return ESP_OK; }
esp_err_t sim_i2s_write(i2s_port_t, const void* src, size_t size, size_t* bytes_written, TickType_t) {
    const int16_t* s = (const int16_t*)src;
    size_t frames = size / 4;                       // 16-bit stereo
    for (size_t i = 0; i < frames; i++) analyse(s[2 * i]);   // left channel
    if (bytes_written) *bytes_written = size;
    // Real hardware would block until the DMA drained this much audio.
    uint32_t us = (uint32_t)((uint64_t)frames * 1000000ULL / s_rate);
    if (us >= 2000) vTaskDelay(pdMS_TO_TICKS(us / 1000)); else delayMicroseconds(us);
    return ESP_OK;
}
esp_err_t sim_i2s_zero_dma_buffer(i2s_port_t) {
    ATECH_SIM_LOG("AUDIO:stop");
    s_winSamples = 0; s_winCrossings = 0; s_winEnergy = 0; s_prev = 0;
    return ESP_OK;
}
}
#endif
