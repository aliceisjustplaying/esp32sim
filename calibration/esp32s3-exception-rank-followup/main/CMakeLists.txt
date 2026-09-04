idf_component_register(
  SRCS "rank_followup.c" "rank_followup.S"
  INCLUDE_DIRS "."
  PRIV_REQUIRES esp_hw_support esp_psram spi_flash
)

if(NOT "$ENV{IDF_VERSION}" STREQUAL "6.1.0")
  message(FATAL_ERROR "H2 capture image requires ESP-IDF v6.1 exactly")
endif()

set_source_files_properties(rank_followup.c PROPERTIES
  COMPILE_OPTIONS "-fno-tree-vectorize;-fno-unroll-loops"
)
