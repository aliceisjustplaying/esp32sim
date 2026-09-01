idf_component_register(
  SRCS "register_blocks.c" "register_blocks.S"
  INCLUDE_DIRS "."
  PRIV_REQUIRES esp_hw_support esp_psram
)

set_source_files_properties(register_blocks.c PROPERTIES
  COMPILE_OPTIONS "-fno-tree-vectorize;-fno-unroll-loops"
)
