idf_component_register(
  SRCS "rom_fetch_ladder.c" "rom_twins.S"
  INCLUDE_DIRS "."
  PRIV_REQUIRES esp_hw_support esp_psram spi_flash esp_timer
)

if(NOT "$ENV{IDF_VERSION}" STREQUAL "6.1.0")
  message(FATAL_ERROR "ROM fetch ladder capture image requires ESP-IDF v6.1 exactly")
endif()

if(DEFINED ENV{ROM_HAMMER})
  target_compile_definitions(${COMPONENT_LIB} PRIVATE ROM_HAMMER=1)
endif()
if(DEFINED ENV{IRAM_HAMMER})
  target_compile_definitions(${COMPONENT_LIB} PRIVATE IRAM_HAMMER=1)
endif()

set_source_files_properties(rom_fetch_ladder.c PROPERTIES
  COMPILE_OPTIONS "-fno-tree-vectorize;-fno-unroll-loops"
)
