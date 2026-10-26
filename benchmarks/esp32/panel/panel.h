#pragma once

// The display of the benchmark boards, wired, clocked and started as the
// Cranpose demo's board modules do it: the LVGL and Slint benchmarks draw
// through it.

#include <stdbool.h>
#include <stdint.h>

#define PANEL_WIDTH 320
#define PANEL_HEIGHT 480

#if CONFIG_IDF_TARGET_ESP32S3
// Hosyond ES3C35P: ST77922 on quad SPI.
#define PANEL_STRIP_ROWS 24
// The ST77922 shows an update only when its columns start and end on a
// multiple of 4.
#define PANEL_COLUMN_ALIGNMENT 4
#else
// LCDWiki E32R35T: ST7796 on SPI.
#define PANEL_STRIP_ROWS 8
#define PANEL_COLUMN_ALIGNMENT 1
#endif

// The bytes of a strip of PANEL_STRIP_ROWS full rows of RGB565.
#define PANEL_STRIP_BYTES (PANEL_WIDTH * PANEL_STRIP_ROWS * 2)

// Called from the transfer's interrupt when the panel has read the pixels of
// a panel_draw call. Returns whether it woke a task that should run as soon
// as the interrupt ends, rather than at the next tick.
typedef bool (*panel_drawn_t)(void *context);

// Starts the bus and the panel, with its backlight off, and sends it its
// start-up sequence; `drawn` is told of each transfer that ends.
void panel_open(panel_drawn_t drawn, void *context);

// Queues the transfer of `pixels`, RGB565 with the high byte first, to the
// columns x1..=x2 and rows y1..=y2. `pixels` must stay until `drawn` is told.
void panel_draw(int32_t x1, int32_t y1, int32_t x2, int32_t y2, const void *pixels);

void panel_backlight_on(void);

// For the Slint benchmark, which reads them through its foreign function
// declarations.
uint32_t panel_strip_rows(void);
uint32_t panel_column_alignment(void);
