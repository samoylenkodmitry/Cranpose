# The Cranpose ESP32 demo's screen in LVGL. See ../README.md.
cmake_minimum_required(VERSION 3.16)
# The display driver the Slint benchmark shares.
list(APPEND EXTRA_COMPONENT_DIRS "${CMAKE_CURRENT_LIST_DIR}/../panel")
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(lvgl_bench)
