# Counts cycles, instructions and stall cycles of the UI core with the Xtensa
# performance counters over windows of CONFIG_PERFSTATS_WINDOW_MS, and logs
# the heap. A wrapper of app_main starts it.
idf_component_register(SRCS "perfstats.c" REQUIRES esp_timer perfmon heap freertos)
target_link_libraries(${COMPONENT_LIB} INTERFACE "-Wl,--wrap=app_main")
