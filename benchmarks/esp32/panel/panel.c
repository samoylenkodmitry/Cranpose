#include "panel.h"

#include <stdbool.h>
#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "esp_lcd_panel_io.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "init_sequence.h"

#define CLOCK_HZ 40000000
#define TRANSFER_QUEUE_DEPTH 10

#if CONFIG_IDF_TARGET_ESP32S3
#define QUAD 1
#define PIN_CS 10
#define PIN_CLK 12
#define PIN_DATA0 11
#define PIN_DATA1 13
#define PIN_DATA2 14
#define PIN_DATA3 9
#define PIN_DC (-1)
#define PIN_BACKLIGHT 41
#else
#define QUAD 0
#define PIN_CS 15
#define PIN_CLK 14
#define PIN_DATA0 13
#define PIN_DATA1 12
#define PIN_DATA2 (-1)
#define PIN_DATA3 (-1)
#define PIN_DC 2
#define PIN_BACKLIGHT 27
#endif

static esp_lcd_panel_io_handle_t panel_io;
static panel_drawn_t on_drawn;
static void *drawn_context;

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
    on_drawn(drawn_context);
    return false;
}

void panel_open(panel_drawn_t drawn, void *context)
{
    on_drawn = drawn;
    drawn_context = context;
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
        .max_transfer_sz = PANEL_STRIP_BYTES,
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

void panel_draw(int32_t x1, int32_t y1, int32_t x2, int32_t y2, const void *pixels)
{
    uint8_t columns[4] = {x1 >> 8, x1 & 0xFF, x2 >> 8, x2 & 0xFF};
    uint8_t rows[4] = {y1 >> 8, y1 & 0xFF, y2 >> 8, y2 & 0xFF};
    send(0x2A, columns, sizeof(columns));
    send(0x2B, rows, sizeof(rows));
    int pixels_command = QUAD ? (0x32 << 24) | (0x2C << 8) : 0x2C;
    size_t bytes = (size_t)(x2 - x1 + 1) * (size_t)(y2 - y1 + 1) * 2;
    ESP_ERROR_CHECK(esp_lcd_panel_io_tx_color(panel_io, pixels_command, pixels, bytes));
}

void panel_backlight_on(void)
{
    gpio_set_level(PIN_BACKLIGHT, 1);
}

uint32_t panel_strip_rows(void)
{
    return PANEL_STRIP_ROWS;
}

uint32_t panel_column_alignment(void)
{
    return PANEL_COLUMN_ALIGNMENT;
}
