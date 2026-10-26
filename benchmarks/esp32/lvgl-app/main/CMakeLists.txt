idf_component_register(
    SRCS "main.c" "dashboard.c"
    INCLUDE_DIRS "."
    REQUIRES esp_timer panel
)
