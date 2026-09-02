idf_component_register(
  SRCS "dma_sram_probe.c" "copy_loop.S"
  INCLUDE_DIRS "."
  PRIV_REQUIRES esp_driver_spi esp_hw_support esp_psram
)

if(NOT "$ENV{IDF_VERSION}" STREQUAL "6.1.0")
  message(FATAL_ERROR "DMA-on-SRAM capture image requires ESP-IDF v6.1 exactly")
endif()

set_source_files_properties(dma_sram_probe.c PROPERTIES
  COMPILE_OPTIONS "-fno-tree-vectorize;-fno-unroll-loops"
)
