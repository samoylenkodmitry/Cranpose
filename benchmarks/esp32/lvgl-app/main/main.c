// The Cranpose ESP32 demo's screen in LVGL 9, on the same boards and wired
// as the demo wires them: the same bus, clock, panel start-up bytes, strip
// size and ESP-IDF settings. It logs its frame times in the demo's format.

#include <stdbool.h>
#include <stdint.h>
#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "esp_heap_caps.h"
#include "esp_lcd_panel_io.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "init_sequence.h"
#include "lvgl.h"

static const char *TAG = "lvgl_bench";

#define WIDTH 320
#define HEIGHT 480
#define CLOCK_HZ 40000000
#define TRANSFER_QUEUE_DEPTH 10
#define STATS_WINDOW_US 5000000

#if CONFIG_IDF_TARGET_ESP32S3
// Hosyond ES3C35P: ST77922 on quad SPI.
#define TITLE "LVGL on ESP32-S3"
#define QUAD 1
#define PIN_CS 10
#define PIN_CLK 12
#define PIN_DATA0 11
#define PIN_DATA1 13
#define PIN_DATA2 14
#define PIN_DATA3 9
#define PIN_DC (-1)
#define PIN_BACKLIGHT 41
#define COLUMN_ALIGNMENT 4
#define STRIP_ROWS 24
#else
// LCDWiki E32R35T: ST7796 on SPI.
#define TITLE "LVGL on ESP32"
#define QUAD 0
#define PIN_CS 15
#define PIN_CLK 14
#define PIN_DATA0 13
#define PIN_DATA1 12
#define PIN_DATA2 (-1)
#define PIN_DATA3 (-1)
#define PIN_DC 2
#define PIN_BACKLIGHT 27
#define COLUMN_ALIGNMENT 1
#define STRIP_ROWS 8
#endif

#define STRIP_BYTES (WIDTH * STRIP_ROWS * 2)

static esp_lcd_panel_io_handle_t panel_io;
static lv_display_t *display;
static TaskHandle_t ui_task;
static bool rendered;

static int command_word(uint8_t command)
{
    return QUAD ? (0x02 << 24) | (command << 8) : command;
}

static void send(uint8_t command, const void *data, size_t length)
{
    ESP_ERROR_CHECK(esp_lcd_panel_io_tx_param(panel_io, command_word(command), data, length));
}

static bool on_transfer_done(esp_lcd_panel_io_handle_t io, esp_lcd_panel_io_event_data_t *event, void *context)
{
    lv_display_flush_ready(display);
    return false;
}

static void open_panel(void)
{
    gpio_reset_pin(PIN_BACKLIGHT);
    gpio_set_direction(PIN_BACKLIGHT, GPIO_MODE_OUTPUT);
    gpio_set_level(PIN_BACKLIGHT, 0);
    spi_bus_config_t bus = {
        .sclk_io_num = PIN_CLK,
        .data0_io_num = PIN_DATA0,
        .data1_io_num = PIN_DATA1,
        .data2_io_num = PIN_DATA2,
        .data3_io_num = PIN_DATA3,
        .data4_io_num = -1,
        .data5_io_num = -1,
        .data6_io_num = -1,
        .data7_io_num = -1,
        .max_transfer_sz = STRIP_BYTES,
        .flags = SPICOMMON_BUSFLAG_MASTER,
    };
    ESP_ERROR_CHECK(spi_bus_initialize(SPI2_HOST, &bus, SPI_DMA_CH_AUTO));
    esp_lcd_panel_io_spi_config_t config = {
        .cs_gpio_num = PIN_CS,
        .dc_gpio_num = PIN_DC,
        .spi_mode = 0,
        .pclk_hz = CLOCK_HZ,
        .trans_queue_depth = TRANSFER_QUEUE_DEPTH,
        .on_color_trans_done = on_transfer_done,
        .lcd_cmd_bits = QUAD ? 32 : 8,
        .lcd_param_bits = 8,
        .flags.quad_mode = QUAD,
    };
    ESP_ERROR_CHECK(esp_lcd_new_panel_io_spi((esp_lcd_spi_bus_handle_t)SPI2_HOST, &config, &panel_io));
    // The software reset, as the Cranpose panel sends it without a reset line.
    send(0x01, NULL, 0);
    vTaskDelay(pdMS_TO_TICKS(120));
    for (size_t i = 0; i < sizeof(INIT) / sizeof(INIT[0]); i++) {
        send(INIT[i].command, INIT[i].data, INIT[i].length);
        if (INIT[i].delay_ms) {
            vTaskDelay(pdMS_TO_TICKS(INIT[i].delay_ms));
        }
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
    uint8_t columns[4] = {area->x1 >> 8, area->x1 & 0xFF, area->x2 >> 8, area->x2 & 0xFF};
    uint8_t rows[4] = {area->y1 >> 8, area->y1 & 0xFF, area->y2 >> 8, area->y2 & 0xFF};
    send(0x2A, columns, sizeof(columns));
    send(0x2B, rows, sizeof(rows));
    int pixels_command = QUAD ? (0x32 << 24) | (0x2C << 8) : 0x2C;
    ESP_ERROR_CHECK(esp_lcd_panel_io_tx_color(panel_io, pixels_command, pixels, pixel_count * 2));
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
    ESP_LOGI(TAG, "%u frames in %.1f s: busy %.2f ms, slowest %.1f ms", (unsigned)stats->frames,
             window / 1e6, stats->busy / 1e3 / stats->frames, stats->slowest / 1e3);
    *stats = (stats_t){.window_start = now, .started = true};
}

void app_main(void)
{
    open_panel();
    int64_t start = esp_timer_get_time();
    lv_init();
    lv_tick_set_cb(tick_ms);
    display = lv_display_create(WIDTH, HEIGHT);
    lv_display_set_color_format(display, LV_COLOR_FORMAT_RGB565);
    void *first = heap_caps_malloc(STRIP_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
    void *second = heap_caps_malloc(STRIP_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
    lv_display_set_buffers(display, first, second, STRIP_BYTES, LV_DISPLAY_RENDER_MODE_PARTIAL);
    lv_display_set_flush_cb(display, flush);
    lv_display_add_event_cb(display, align_area, LV_EVENT_INVALIDATE_AREA, NULL);
    lv_display_add_event_cb(display, on_render_start, LV_EVENT_RENDER_START, NULL);
    build_screen();

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
                gpio_set_level(PIN_BACKLIGHT, 1);
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
