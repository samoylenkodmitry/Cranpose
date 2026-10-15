# Thread-local lookups through ESP-IDF's pthread keys in a few instructions.
# See fast_tls.c.
idf_component_register(SRCS "fast_tls.c" REQUIRES pthread)
target_link_libraries(${COMPONENT_LIB} INTERFACE
    "-Wl,--wrap=pthread_getspecific"
    "-Wl,--wrap=pthread_setspecific")
