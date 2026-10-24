# A sampling profiler of core 0. A wrapper of app_main starts it, so linking
# the component in is all an app needs. See sampler.c.
idf_component_register(SRCS "sampler.c" REQUIRES esp_driver_gptimer esp_timer freertos heap xtensa)
target_link_libraries(${COMPONENT_LIB} INTERFACE "-Wl,--wrap=app_main")
