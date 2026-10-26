// The dashboard of crates/cranpose/tests/support/dashboard.rs in LVGL 9: the
// same layout, colors and values, which the functions below compute from
// the milliseconds since the first frame as the Rust `timeline` does. A
// timer sets them every frame, and each label only when its text changes.

#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include "dashboard.h"

#define TAU 6.2831855f
#define BARS 24
#define POINTS 32
#define ROWS 40
#define ROW_HEIGHT 28

static float wave(uint32_t t, uint32_t period, float phase)
{
    return sinf(TAU * (float)(t % period) / (float)period + phase);
}

static float gauge(uint32_t t) { return 0.5f + 0.45f * wave(t, 4000, 0.0f); }
static uint32_t gauge_percent(uint32_t t) { return (uint32_t)(gauge(t) * 100.0f + 0.5f); }
static float level(uint32_t t, int index) { return 0.5f + 0.45f * wave(t, 2000 + 700 * index, (float)index); }
static float bar(uint32_t t, int index) { return 0.55f + 0.4f * wave(t, 1600, index * 0.5f); }
static float line(uint32_t t, int index) { return 0.5f + 0.35f * wave(t, 2500, index * 0.35f); }
static float slider(uint32_t t) { return 0.5f + 0.5f * wave(t, 3000, 0.0f); }
static bool switch_on(uint32_t t, uint32_t period) { return (t / period) % 2 == 0; }
static uint32_t scroll(uint32_t t) { return (uint32_t)((uint64_t)t * 60 / 1000 % 1000); }
static uint32_t cpu(uint32_t t) { return 20 + (t / 200 * 37) % 61; }
static uint32_t temperature(uint32_t t) { return 360 + (t / 330 * 7) % 30; }
static uint32_t network(uint32_t t) { return 5 + (t / 500 * 13) % 90; }
static uint32_t row_value(int index) { return (index * 37 + 11) % 100; }

#define RGB(r, g, b) lv_color_make(r, g, b)
#define BACKGROUND RGB(15, 18, 32)
#define CARD RGB(27, 31, 51)
#define TRACK RGB(42, 48, 80)
#define MUTED RGB(140, 147, 176)
#define WHITE RGB(255, 255, 255)
#define BLUE RGB(67, 97, 238)
#define VIOLET RGB(123, 47, 247)
#define CYAN RGB(76, 201, 240)
#define PINK RGB(247, 37, 133)
#define PURPLE RGB(114, 9, 183)
#define YELLOW RGB(255, 209, 102)
#define ORANGE RGB(247, 127, 0)
#define RED RGB(255, 107, 107)
#define TEAL RGB(78, 205, 196)

typedef struct {
    lv_obj_t *label;
    uint32_t shown;
} reading_t;

static uint32_t start_ms;
static reading_t clock_text;
static reading_t stats[3];
static reading_t percent;
static lv_obj_t *ring;
static lv_obj_t *levels[3];
static lv_obj_t *chart;
static lv_obj_t *bars[BARS];
static lv_obj_t *chart_line;
static lv_point_precise_t points[POINTS];
static lv_obj_t *switches[2];
static lv_obj_t *slider_bar;
static lv_obj_t *list;

static lv_obj_t *plain(lv_obj_t *parent)
{
    lv_obj_t *object = lv_obj_create(parent);
    lv_obj_remove_style_all(object);
    lv_obj_remove_flag(object, LV_OBJ_FLAG_SCROLLABLE);
    return object;
}

static void fill(lv_obj_t *object, lv_color_t color, int32_t radius)
{
    lv_obj_set_style_bg_color(object, color, 0);
    lv_obj_set_style_bg_opa(object, LV_OPA_COVER, 0);
    lv_obj_set_style_radius(object, radius, 0);
}

static lv_obj_t *card(lv_obj_t *parent, int32_t height)
{
    lv_obj_t *object = plain(parent);
    lv_obj_set_size(object, LV_PCT(100), height);
    fill(object, CARD, 12);
    return object;
}

static void row_layout(lv_obj_t *object)
{
    lv_obj_set_flex_flow(object, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(object, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
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

static void build_header(lv_obj_t *screen, const char *title)
{
    lv_obj_t *header = plain(screen);
    lv_obj_set_size(header, LV_PCT(100), 40);
    fill(header, BLUE, 12);
    lv_obj_set_style_bg_grad_color(header, VIOLET, 0);
    lv_obj_set_style_bg_grad_dir(header, LV_GRAD_DIR_HOR, 0);
    lv_obj_set_style_pad_hor(header, 12, 0);
    row_layout(header);
    lv_obj_t *name = text(header, &lv_font_montserrat_18, WHITE, title);
    lv_obj_set_flex_grow(name, 1);
    clock_text.label = text(header, &lv_font_montserrat_18, WHITE, "");
    clock_text.shown = UINT32_MAX;
}

static void build_stat(lv_obj_t *row, int index, const char *label, lv_color_t dot_color)
{
    lv_obj_t *tile = plain(row);
    lv_obj_set_height(tile, LV_PCT(100));
    lv_obj_set_flex_grow(tile, 1);
    fill(tile, CARD, 12);
    lv_obj_set_style_pad_all(tile, 8, 0);
    lv_obj_set_style_pad_row(tile, 2, 0);
    lv_obj_set_flex_flow(tile, LV_FLEX_FLOW_COLUMN);
    lv_obj_t *title = plain(tile);
    lv_obj_set_size(title, LV_SIZE_CONTENT, LV_SIZE_CONTENT);
    row_layout(title);
    lv_obj_t *dot = plain(title);
    lv_obj_set_size(dot, 8, 8);
    fill(dot, dot_color, 4);
    lv_obj_t *name = text(title, &lv_font_montserrat_12, MUTED, label);
    lv_obj_set_style_pad_left(name, 6, 0);
    stats[index].label = text(tile, &lv_font_montserrat_16, WHITE, "");
    stats[index].shown = UINT32_MAX;
}

static void build_level(lv_obj_t *column, int index, const char *label, lv_color_t from, lv_color_t to)
{
    text(column, &lv_font_montserrat_12, MUTED, label);
    lv_obj_t *track = lv_bar_create(column);
    lv_obj_remove_style_all(track);
    lv_obj_set_size(track, LV_PCT(100), 8);
    lv_bar_set_range(track, 0, 1000);
    fill(track, TRACK, 4);
    lv_obj_set_style_bg_color(track, from, LV_PART_INDICATOR);
    lv_obj_set_style_bg_opa(track, LV_OPA_COVER, LV_PART_INDICATOR);
    lv_obj_set_style_bg_grad_color(track, to, LV_PART_INDICATOR);
    lv_obj_set_style_bg_grad_dir(track, LV_GRAD_DIR_HOR, LV_PART_INDICATOR);
    lv_obj_set_style_radius(track, 4, LV_PART_INDICATOR);
    levels[index] = track;
}

static void build_gauge(lv_obj_t *screen)
{
    lv_obj_t *gauge_card = card(screen, 112);
    lv_obj_set_style_pad_all(gauge_card, 10, 0);
    row_layout(gauge_card);

    ring = lv_arc_create(gauge_card);
    lv_obj_remove_style_all(ring);
    lv_obj_remove_flag(ring, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_size(ring, 84, 84);
    lv_arc_set_bg_angles(ring, 0, 360);
    lv_arc_set_rotation(ring, 270);
    lv_arc_set_range(ring, 0, 1000);
    lv_obj_set_style_arc_width(ring, 10, LV_PART_MAIN);
    lv_obj_set_style_arc_color(ring, TRACK, LV_PART_MAIN);
    lv_obj_set_style_arc_width(ring, 10, LV_PART_INDICATOR);
    lv_obj_set_style_arc_color(ring, CYAN, LV_PART_INDICATOR);
    lv_obj_set_style_arc_rounded(ring, true, LV_PART_INDICATOR);
    lv_obj_t *center = plain(ring);
    lv_obj_set_size(center, LV_SIZE_CONTENT, LV_SIZE_CONTENT);
    lv_obj_set_flex_flow(center, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(center, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_center(center);
    percent.label = text(center, &lv_font_montserrat_20, WHITE, "");
    percent.shown = UINT32_MAX;
    text(center, &lv_font_montserrat_12, MUTED, "load");

    lv_obj_t *column = plain(gauge_card);
    lv_obj_set_height(column, LV_SIZE_CONTENT);
    lv_obj_set_flex_grow(column, 1);
    lv_obj_set_style_pad_left(column, 14, 0);
    lv_obj_set_style_pad_row(column, 3, 0);
    lv_obj_set_flex_flow(column, LV_FLEX_FLOW_COLUMN);
    build_level(column, 0, "Fan", PINK, PURPLE);
    build_level(column, 1, "Pump", CYAN, BLUE);
    build_level(column, 2, "Light", YELLOW, ORANGE);
}

static void build_chart(lv_obj_t *screen)
{
    lv_obj_t *chart_card = card(screen, 100);
    lv_obj_set_style_pad_all(chart_card, 10, 0);
    lv_obj_set_style_pad_row(chart_card, 4, 0);
    lv_obj_set_flex_flow(chart_card, LV_FLEX_FLOW_COLUMN);
    text(chart_card, &lv_font_montserrat_12, MUTED, "Activity");
    chart = plain(chart_card);
    lv_obj_set_width(chart, LV_PCT(100));
    lv_obj_set_flex_grow(chart, 1);
    for (int index = 0; index < BARS; index++) {
        lv_obj_t *column = plain(chart);
        fill(column, CYAN, 2);
        lv_obj_set_style_bg_grad_color(column, BLUE, 0);
        lv_obj_set_style_bg_grad_dir(column, LV_GRAD_DIR_VER, 0);
        bars[index] = column;
    }
    chart_line = lv_line_create(chart);
    lv_obj_remove_style_all(chart_line);
    // As big as the chart, so each frame redraws where the line was and is.
    lv_obj_set_size(chart_line, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_line_width(chart_line, 2, 0);
    lv_obj_set_style_line_color(chart_line, YELLOW, 0);
    lv_obj_set_style_line_rounded(chart_line, true, 0);
}

static lv_obj_t *build_switch(lv_obj_t *row)
{
    lv_obj_t *toggle = lv_switch_create(row);
    lv_obj_remove_style_all(toggle);
    lv_obj_remove_flag(toggle, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_size(toggle, 40, 22);
    fill(toggle, TRACK, LV_RADIUS_CIRCLE);
    lv_obj_set_style_anim_duration(toggle, 200, 0);
    lv_obj_set_style_bg_color(toggle, CYAN, LV_PART_INDICATOR | LV_STATE_CHECKED);
    lv_obj_set_style_bg_opa(toggle, LV_OPA_COVER, LV_PART_INDICATOR | LV_STATE_CHECKED);
    lv_obj_set_style_radius(toggle, LV_RADIUS_CIRCLE, LV_PART_INDICATOR);
    lv_obj_set_style_bg_color(toggle, WHITE, LV_PART_KNOB);
    lv_obj_set_style_bg_opa(toggle, LV_OPA_COVER, LV_PART_KNOB);
    lv_obj_set_style_radius(toggle, LV_RADIUS_CIRCLE, LV_PART_KNOB);
    lv_obj_set_style_pad_all(toggle, 2, LV_PART_KNOB);
    return toggle;
}

static void build_controls(lv_obj_t *screen)
{
    lv_obj_t *controls = card(screen, 44);
    lv_obj_set_style_pad_hor(controls, 12, 0);
    lv_obj_set_style_pad_column(controls, 12, 0);
    row_layout(controls);
    switches[0] = build_switch(controls);
    switches[1] = build_switch(controls);
    slider_bar = lv_slider_create(controls);
    lv_obj_remove_style_all(slider_bar);
    lv_obj_remove_flag(slider_bar, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_height(slider_bar, 4);
    lv_obj_set_flex_grow(slider_bar, 1);
    lv_slider_set_range(slider_bar, 0, 1000);
    fill(slider_bar, TRACK, 2);
    lv_obj_set_style_bg_color(slider_bar, BLUE, LV_PART_INDICATOR);
    lv_obj_set_style_bg_opa(slider_bar, LV_OPA_COVER, LV_PART_INDICATOR);
    lv_obj_set_style_radius(slider_bar, 2, LV_PART_INDICATOR);
    lv_obj_set_style_bg_color(slider_bar, WHITE, LV_PART_KNOB);
    lv_obj_set_style_bg_opa(slider_bar, LV_OPA_COVER, LV_PART_KNOB);
    lv_obj_set_style_radius(slider_bar, LV_RADIUS_CIRCLE, LV_PART_KNOB);
    lv_obj_set_style_pad_all(slider_bar, 6, LV_PART_KNOB);
}

static void build_list(lv_obj_t *screen)
{
    lv_obj_t *list_card = card(screen, 0);
    lv_obj_set_flex_grow(list_card, 1);
    lv_obj_set_style_pad_ver(list_card, 6, 0);
    list = lv_obj_create(list_card);
    lv_obj_remove_style_all(list);
    lv_obj_set_size(list, LV_PCT(100), LV_PCT(100));
    lv_obj_set_scrollbar_mode(list, LV_SCROLLBAR_MODE_OFF);
    lv_obj_set_flex_flow(list, LV_FLEX_FLOW_COLUMN);
    const lv_color_t colors[5] = {BLUE, PINK, TEAL, ORANGE, VIOLET};
    char line_text[16];
    for (int index = 0; index < ROWS; index++) {
        lv_obj_t *row = plain(list);
        lv_obj_set_size(row, LV_PCT(100), ROW_HEIGHT);
        lv_obj_set_style_pad_hor(row, 10, 0);
        row_layout(row);
        lv_obj_t *avatar = plain(row);
        lv_obj_set_size(avatar, 20, 20);
        fill(avatar, colors[index % 5], 10);
        char letter[2] = {(char)('A' + index % 5), 0};
        lv_obj_center(text(avatar, &lv_font_montserrat_12, WHITE, letter));
        snprintf(line_text, sizeof(line_text), "Sensor %02d", index + 1);
        lv_obj_t *name = text(row, &lv_font_montserrat_14, WHITE, line_text);
        lv_obj_set_flex_grow(name, 1);
        lv_obj_set_style_pad_left(name, 8, 0);
        snprintf(line_text, sizeof(line_text), "%u kWh", (unsigned)row_value(index));
        text(row, &lv_font_montserrat_12, MUTED, line_text);
    }
}

// Sets `reading`'s label to `value` formatted by `format`, when it changed.
static void show(reading_t *reading, uint32_t value, const char *format, uint32_t a, uint32_t b)
{
    if (reading->shown == value) {
        return;
    }
    reading->shown = value;
    lv_label_set_text_fmt(reading->label, format, (unsigned)a, (unsigned)b);
}

static void update(lv_timer_t *timer)
{
    uint32_t t = lv_tick_elaps(start_ms);
    uint32_t seconds = t / 1000;
    show(&clock_text, seconds, "%02u:%02u", seconds / 60 % 60, seconds % 60);
    uint32_t value = cpu(t);
    show(&stats[0], value, "%u%%", value, 0);
    value = temperature(t);
    show(&stats[1], value, "%u.%u C", value / 10, value % 10);
    value = network(t);
    show(&stats[2], value, "%u.%u MB/s", value / 10, value % 10);
    value = gauge_percent(t);
    show(&percent, value, "%u%%", value, 0);
    lv_arc_set_value(ring, (int32_t)(gauge(t) * 1000.0f));
    for (int index = 0; index < 3; index++) {
        lv_bar_set_value(levels[index], (int32_t)(level(t, index) * 1000.0f), LV_ANIM_OFF);
    }

    int32_t width = lv_obj_get_content_width(chart);
    int32_t height = lv_obj_get_content_height(chart);
    float gap = 3.0f;
    float bar_width = ((float)width - gap * (BARS - 1)) / BARS;
    for (int index = 0; index < BARS; index++) {
        int32_t bar_height = (int32_t)(height * bar(t, index));
        int32_t x = (int32_t)(index * (bar_width + gap));
        lv_obj_set_pos(bars[index], x, height - bar_height);
        lv_obj_set_size(bars[index], (int32_t)((index + 1) * (bar_width + gap) - gap) - x, bar_height);
    }
    float step = (float)width / (POINTS - 1);
    for (int index = 0; index < POINTS; index++) {
        points[index].x = index * step;
        points[index].y = height * (1.0f - line(t, index));
    }
    lv_line_set_points(chart_line, points, POINTS);

    for (int index = 0; index < 2; index++) {
        bool on = switch_on(t, index == 0 ? 1000 : 1500);
        if (on != lv_obj_has_state(switches[index], LV_STATE_CHECKED)) {
            lv_obj_set_state(switches[index], LV_STATE_CHECKED, on);
        }
    }
    lv_slider_set_value(slider_bar, (int32_t)(slider(t) * 1000.0f), LV_ANIM_OFF);
    lv_obj_scroll_to_y(list, scroll(t), LV_ANIM_OFF);
}

void dashboard_build(const char *title)
{
    lv_obj_t *screen = lv_screen_active();
    lv_obj_remove_style_all(screen);
    lv_obj_remove_flag(screen, LV_OBJ_FLAG_SCROLLABLE);
    fill(screen, BACKGROUND, 0);
    lv_obj_set_style_pad_all(screen, 10, 0);
    lv_obj_set_style_pad_row(screen, 6, 0);
    lv_obj_set_flex_flow(screen, LV_FLEX_FLOW_COLUMN);

    build_header(screen, title);
    lv_obj_t *row = plain(screen);
    lv_obj_set_size(row, LV_PCT(100), 52);
    lv_obj_set_style_pad_column(row, 6, 0);
    lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
    build_stat(row, 0, "CPU", RED);
    build_stat(row, 1, "TEMP", TEAL);
    build_stat(row, 2, "NET", YELLOW);
    build_gauge(screen);
    build_chart(screen);
    build_controls(screen);
    build_list(screen);

    lv_obj_update_layout(screen);
    start_ms = lv_tick_get();
    lv_timer_create(update, 16, NULL);
    update(NULL);
}
