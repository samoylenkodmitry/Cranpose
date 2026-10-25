// The Cranpose ESP32 demo's screen in LVGL 9, on the same boards and wired
// as the demo wires them: the same bus, clock, panel start-up bytes, strip
// size and ESP-IDF settings. It logs its frame times in the demo's format.

#include <stdbool.h>
#include <stdint.h>
#include "dashboard.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "lvgl.h"
#include "panel.h"

static const char *TAG = "lvgl_bench";

#define WIDTH PANEL_WIDTH
#define HEIGHT PANEL_HEIGHT
#define COLUMN_ALIGNMENT PANEL_COLUMN_ALIGNMENT
#define STRIP_BYTES PANEL_STRIP_BYTES
#define STATS_WINDOW_US 5000000

#if CONFIG_IDF_TARGET_ESP32S3
#define TITLE "LVGL on ESP32-S3"
#else
#define TITLE "LVGL on ESP32"
#endif

static lv_display_t *display;
static TaskHandle_t ui_task;
static bool rendered;
// The bytes sent to the panel since the stats window began.
static uint64_t sent_bytes;
static SemaphoreHandle_t flushed;
static volatile bool flushing;

static bool on_transfer_done(void *context)
{
    flushing = false;
    lv_display_flush_ready(display);
    BaseType_t woken = pdFALSE;
    xSemaphoreGiveFromISR(flushed, &woken);
    return woken == pdTRUE;
}

// LVGL waits here for the panel to read a strip: blocked, as the other
// benchmarks wait, so the core idles meanwhile instead of spinning on a
// flag.
static void wait_flushed(lv_display_t *disp)
{
    while (flushing) {
        xSemaphoreTake(flushed, portMAX_DELAY);
    }
}

// Widens each invalidated area to the columns the panel accepts as an update.
static void align_area(lv_event_t *event)
{
    lv_area_t *area = lv_event_get_param(event);
    area->x1 = area->x1 / COLUMN_ALIGNMENT * COLUMN_ALIGNMENT;
    int32_t end = (area->x2 + 1 + COLUMN_ALIGNMENT - 1) / COLUMN_ALIGNMENT * COLUMN_ALIGNMENT;
    area->x2 = (end > WIDTH ? WIDTH : end) - 1;
}

static void flush(lv_display_t *disp, const lv_area_t *area, uint8_t *pixels)
{
    uint32_t pixel_count = lv_area_get_width(area) * lv_area_get_height(area);
    lv_draw_sw_rgb565_swap(pixels, pixel_count);
    sent_bytes += pixel_count * 2;
    flushing = true;
    panel_draw(area->x1, area->y1, area->x2, area->y2, pixels);
}

static void on_render_start(lv_event_t *event)
{
    rendered = true;
}

static uint32_t tick_ms(void)
{
    return (uint32_t)(esp_timer_get_time() / 1000);
}

static lv_obj_t *track;
static lv_obj_t *seconds_label;
static lv_obj_t *taps_label;
static unsigned seconds;
static unsigned taps;

static lv_obj_t *plain(lv_obj_t *parent)
{
    lv_obj_t *object = lv_obj_create(parent);
    lv_obj_remove_style_all(object);
    lv_obj_remove_flag(object, LV_OBJ_FLAG_SCROLLABLE);
    return object;
}

static lv_obj_t *text(lv_obj_t *parent, const lv_font_t *font, lv_color_t color, const char *value)
{
    lv_obj_t *label = lv_label_create(parent);
    lv_obj_remove_style_all(label);
    lv_obj_set_style_text_font(label, font, 0);
    lv_obj_set_style_text_color(label, color, 0);
    lv_label_set_text(label, value);
    return label;
}

static void fill(lv_obj_t *object, lv_color_t color, int32_t radius)
{
    lv_obj_set_style_bg_color(object, color, 0);
    lv_obj_set_style_bg_opa(object, LV_OPA_COVER, 0);
    lv_obj_set_style_radius(object, radius, 0);
}

static void set_bar_width(void *bar, int32_t permille)
{
    lv_obj_set_width(bar, lv_obj_get_content_width(track) * permille / 1000);
}

static void count_second(lv_timer_t *timer)
{
    seconds++;
    lv_label_set_text_fmt(seconds_label, "Running for %u s", seconds);
}

#if CONFIG_BENCH_TEXT_EVERY_FRAME
// A number that counts up every frame, set as the label's text.
static void count_reading(lv_timer_t *timer)
{
    seconds++;
    lv_label_set_text_fmt(seconds_label, "Reading %u", seconds);
}
#endif

static void count_tap(lv_event_t *event)
{
    taps++;
    lv_label_set_text_fmt(taps_label, "Taps: %u", taps);
}

static void build_screen(void)
{
    lv_obj_t *screen = lv_screen_active();
    lv_obj_remove_style_all(screen);
    lv_obj_remove_flag(screen, LV_OBJ_FLAG_SCROLLABLE);
    fill(screen, lv_color_make(18, 20, 30), 0);
    lv_obj_set_style_pad_all(screen, 20, 0);
    lv_obj_set_style_pad_row(screen, 14, 0);
    lv_obj_set_flex_flow(screen, LV_FLEX_FLOW_COLUMN);

    text(screen, &lv_font_montserrat_22, lv_color_white(), TITLE);
    lv_obj_t *subtitle = text(screen, &lv_font_montserrat_14, lv_color_make(160, 166, 190),
                              "Composed, laid out and drawn on the chip.");
    lv_obj_set_width(subtitle, LV_PCT(100));

    track = plain(screen);
    lv_obj_set_size(track, LV_PCT(100), 16);
    fill(track, lv_color_make(40, 44, 64), 8);
    lv_obj_t *bar = plain(track);
    lv_obj_set_height(bar, 16);
    fill(bar, lv_color_make(98, 160, 255), 8);
    lv_obj_update_layout(screen);
    lv_anim_t sweep;
    lv_anim_init(&sweep);
    lv_anim_set_var(&sweep, bar);
    lv_anim_set_exec_cb(&sweep, set_bar_width);
    lv_anim_set_values(&sweep, 100, 1000);
    lv_anim_set_duration(&sweep, 1500);
    lv_anim_set_reverse_duration(&sweep, 1500);
    lv_anim_set_repeat_count(&sweep, LV_ANIM_REPEAT_INFINITE);
    lv_anim_set_path_cb(&sweep, lv_anim_path_linear);
    lv_anim_start(&sweep);

    seconds_label = text(screen, &lv_font_montserrat_14, lv_color_make(200, 204, 220), "Running for 0 s");
#if CONFIG_BENCH_TEXT_EVERY_FRAME
    lv_timer_create(count_reading, 16, NULL);
#else
    lv_timer_create(count_second, 1000, NULL);
#endif

    lv_obj_t *button = plain(screen);
    lv_obj_set_width(button, LV_PCT(100));
    lv_obj_set_height(button, LV_SIZE_CONTENT);
    fill(button, lv_color_make(98, 160, 255), 12);
    lv_obj_set_style_pad_all(button, 20, 0);
    lv_obj_add_flag(button, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(button, count_tap, LV_EVENT_CLICKED, NULL);
    taps_label = text(button, &lv_font_montserrat_18, lv_color_make(18, 20, 30), "Taps: 0");
}

static void wake(void *arg)
{
    xTaskNotifyGive(ui_task);
}

typedef struct {
    int64_t window_start;
    uint32_t frames;
    int64_t busy;
    int64_t slowest;
    bool started;
} stats_t;

static void record(stats_t *stats, int64_t start, int64_t busy, int64_t now)
{
    if (!stats->started) {
        ESP_LOGI(TAG, "first frame after %lld ms: draw and send %lld ms", (now - start) / 1000, busy / 1000);
        stats->started = true;
        stats->window_start = now;
        return;
    }
    stats->frames++;
    stats->busy += busy;
    if (busy > stats->slowest) {
        stats->slowest = busy;
    }
    int64_t window = now - stats->window_start;
    if (window < STATS_WINDOW_US) {
        return;
    }
    ESP_LOGI(TAG, "%u frames in %.1f s: busy %.2f ms, slowest %.1f ms, sent %.1f KB a frame",
             (unsigned)stats->frames, window / 1e6, stats->busy / 1e3 / stats->frames, stats->slowest / 1e3,
             sent_bytes / 1024.0 / stats->frames);
    sent_bytes = 0;
    *stats = (stats_t){.window_start = now, .started = true};
}

void app_main(void)
{
    flushed = xSemaphoreCreateBinary();
    panel_open(on_transfer_done, NULL);
    int64_t start = esp_timer_get_time();
    lv_init();
    lv_tick_set_cb(tick_ms);
    display = lv_display_create(WIDTH, HEIGHT);
    lv_display_set_color_format(display, LV_COLOR_FORMAT_RGB565);
    void *first = heap_caps_malloc(STRIP_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
    void *second = heap_caps_malloc(STRIP_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
    lv_display_set_buffers(display, first, second, STRIP_BYTES, LV_DISPLAY_RENDER_MODE_PARTIAL);
    lv_display_set_flush_cb(display, flush);
    lv_display_set_flush_wait_cb(display, wait_flushed);
    lv_display_add_event_cb(display, align_area, LV_EVENT_INVALIDATE_AREA, NULL);
    lv_display_add_event_cb(display, on_render_start, LV_EVENT_RENDER_START, NULL);
#if CONFIG_BENCH_DASHBOARD
    dashboard_build(TITLE);
#else
    build_screen();
#endif

    ui_task = xTaskGetCurrentTaskHandle();
    esp_timer_handle_t timer;
    esp_timer_create_args_t timer_args = {.callback = wake, .name = "lvgl"};
    ESP_ERROR_CHECK(esp_timer_create(&timer_args, &timer));
    stats_t stats = {0};
    bool backlight = false;
    for (;;) {
        rendered = false;
        int64_t before = esp_timer_get_time();
        uint32_t wait_ms = lv_timer_handler();
        int64_t after = esp_timer_get_time();
        if (rendered) {
            record(&stats, start, after - before, after);
            if (!backlight) {
                // The last strip's transfer ends before the backlight turns on.
                vTaskDelay(1);
                panel_backlight_on();
                backlight = true;
            }
        }
        if (wait_ms == LV_NO_TIMER_READY) {
            wait_ms = 1000;
        }
        if (wait_ms > 0) {
            esp_timer_start_once(timer, (uint64_t)wait_ms * 1000);
            ulTaskNotifyTake(pdTRUE, portMAX_DELAY);
        }
    }
}
