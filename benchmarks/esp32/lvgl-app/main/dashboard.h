#pragma once

#include "lvgl.h"

// Builds the dashboard of the Cranpose ESP32 benchmarks on the active screen,
// titled `title`, with a timer that sets its values every frame.
void dashboard_build(const char *title);
