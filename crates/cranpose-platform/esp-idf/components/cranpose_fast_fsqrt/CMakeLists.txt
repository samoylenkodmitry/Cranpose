# f32 square roots with the FPU's square root steps. See fsqrt.S.
idf_component_register(SRCS "fsqrt.S")
target_link_libraries(${COMPONENT_LIB} INTERFACE "-Wl,--wrap=sqrtf")
