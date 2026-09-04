cmake_minimum_required(VERSION 3.22)

set(IDF_TARGET esp32s3 CACHE STRING "Calibration target")
set(SDKCONFIG "${CMAKE_BINARY_DIR}/sdkconfig")
set(SDKCONFIG_DEFAULTS "${CMAKE_CURRENT_LIST_DIR}/sdkconfig.defaults")

execute_process(
  COMMAND "${CMAKE_COMMAND}" -E env PYTHONDONTWRITEBYTECODE=1
          python3 "${CMAKE_CURRENT_LIST_DIR}/prove_design.py"
          --check "${CMAKE_CURRENT_LIST_DIR}/design-proof.json"
  RESULT_VARIABLE H2_PROOF_RESULT
)
if(NOT H2_PROOF_RESULT EQUAL 0)
  message(FATAL_ERROR "H2 exact-rank paper proof failed")
endif()

include($ENV{IDF_PATH}/tools/cmake/project.cmake)
idf_build_set_property(MINIMAL_BUILD ON)
project(esp32s3_exception_rank_followup)
