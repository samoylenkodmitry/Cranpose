# Moves the framework's per-frame code into IRAM with a linker script of the
# chip, when CONFIG_CRANPOSE_HOT_CODE_IN_IRAM is on. See README.md.
idf_component_register()
set(script "${CMAKE_CURRENT_LIST_DIR}/${IDF_TARGET}.ld")
if(CONFIG_CRANPOSE_HOT_CODE_IN_IRAM AND EXISTS "${script}")
    target_link_libraries(${COMPONENT_LIB} INTERFACE "-Wl,-T,${script}")
endif()
