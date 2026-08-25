// Force-included (-include) into every translation unit of the `sim` env.
// Wokwi does not simulate the ESP32-S3 I2S peripheral, so the legacy I2S driver
// calls made by the real Atech speaker.cpp are redirected to sim_i2s.cpp.
#pragma once
#ifdef ATECH_SIM
#define i2s_driver_install   sim_i2s_driver_install
#define i2s_driver_uninstall sim_i2s_driver_uninstall
#define i2s_set_pin          sim_i2s_set_pin
#define i2s_write            sim_i2s_write
#define i2s_zero_dma_buffer  sim_i2s_zero_dma_buffer
#define i2s_set_sample_rates sim_i2s_set_sample_rates
#endif
