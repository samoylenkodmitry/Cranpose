idf_component_register(
    SRCS "main.c"
    INCLUDE_DIRS "." "${CMAKE_CURRENT_BINARY_DIR}"
    REQUIRES esp_lcd esp_driver_spi esp_driver_gpio esp_timer
)

# The display start-up sequence comes from the Cranpose demo's board module,
# so both apps send the panel the same bytes.
if(CONFIG_IDF_TARGET_ESP32S3)
    set(board_module es3c35p)
else()
    set(board_module e32r35t)
endif()
set(board_source "${CMAKE_CURRENT_LIST_DIR}/../../../../apps/esp32-demo/src/board/${board_module}.rs")
set(init_header "${CMAKE_CURRENT_BINARY_DIR}/init_sequence.h")
add_custom_command(
    OUTPUT "${init_header}"
    COMMAND python3 "${CMAKE_CURRENT_LIST_DIR}/../../init_sequences.py" "${board_source}" "${init_header}"
    DEPENDS "${board_source}" "${CMAKE_CURRENT_LIST_DIR}/../../init_sequences.py"
)
add_custom_target(init_sequence DEPENDS "${init_header}")
add_dependencies(${COMPONENT_LIB} init_sequence)
