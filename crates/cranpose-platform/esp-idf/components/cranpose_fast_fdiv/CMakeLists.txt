# f32 division with the FPU's divide steps on the classic ESP32. See fdiv.S.
if(CONFIG_IDF_TARGET_ESP32)
    idf_component_register(SRCS "fdiv.S")
    target_link_libraries(${COMPONENT_LIB} INTERFACE "-Wl,--wrap=__divsf3")
else()
    idf_component_register()
endif()
