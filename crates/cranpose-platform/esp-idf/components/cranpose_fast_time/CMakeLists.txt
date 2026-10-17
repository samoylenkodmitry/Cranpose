# CLOCK_MONOTONIC without a 64-bit division. See fast_time.c.
idf_component_register(SRCS "fast_time.c" REQUIRES newlib)
target_link_libraries(${COMPONENT_LIB} INTERFACE "-Wl,--wrap=clock_gettime")
