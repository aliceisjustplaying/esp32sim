idf_component_register(
  SRCS "rom_fetch_ladder.c" "rom_twins.S"
  INCLUDE_DIRS "."
  PRIV_REQUIRES esp_hw_support esp_psram spi_flash
)

if(NOT "$ENV{IDF_VERSION}" STREQUAL "6.1.0")
  message(FATAL_ERROR "ROM fetch ladder capture image requires ESP-IDF v6.1 exactly")
endif()

set_source_files_properties(rom_fetch_ladder.c PROPERTIES
  COMPILE_OPTIONS "-fno-tree-vectorize;-fno-unroll-loops"
)
