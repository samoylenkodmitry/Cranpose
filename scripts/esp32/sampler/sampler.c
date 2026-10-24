// A sampling profiler for ESP-IDF apps on the ESP32 and ESP32-S3. A timer
// interrupt reads the PC of the task it interrupted on core 0, where the UI
// runs, 5000 times a second from 4 s to 14 s after start, and counts each PC
// in a table. At 14.5 s it prints `SPC <pc> <count>` and, with PSRAM,
// `SCALLER <return address> <count>` lines, which scripts/esp32/profile.py
// and hot_code.py read with the app's ELF.
//
// FreeRTOS saves the interrupted task's registers on its stack at interrupt
// entry and the stack pointer in its TCB, unless the interrupt nested in
// another one, which the sampler then skips.

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include "driver/gptimer.h"
#include "esp_attr.h"
#include "esp_heap_caps.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "xtensa_context.h"

#define SAMPLER_HZ 5000
#define START_US 4000000
#define STOP_US 14000000

extern volatile unsigned port_interruptNesting[portNUM_PROCESSORS];

typedef struct {
    uint32_t key;
    uint32_t count;
} slot_t;

static slot_t *pcs;
static slot_t *callers;
static uint32_t slots;
static uint32_t shift;
static uint32_t bucket_mask = ~0u;
static volatile uint32_t total, dropped, nested;

static IRAM_ATTR void add(slot_t *table, uint32_t key)
{
    uint32_t index = ((key >> 1) * 2654435761u) >> shift;
    for (int probe = 0; probe < 32; probe++) {
        slot_t *slot = &table[(index + probe) & (slots - 1)];
        if (slot->key == key) {
            slot->count++;
            return;
        }
        if (slot->key == 0) {
            slot->key = key;
            slot->count = 1;
            return;
        }
    }
    dropped++;
}

static IRAM_ATTR bool on_alarm(gptimer_handle_t timer, const gptimer_alarm_event_data_t *event, void *context)
{
    int64_t now = esp_timer_get_time();
    if (now < START_US || now >= STOP_US) {
        return false;
    }
    if (port_interruptNesting[xPortGetCoreID()] > 1) {
        nested++;
        return false;
    }
    TaskHandle_t task = xTaskGetCurrentTaskHandle();
    if (!task) {
        return false;
    }
    // The first word of a TCB is its saved stack pointer.
    XtExcFrame *frame = (XtExcFrame *)*(StackType_t **)task;
    uint32_t pc = frame->pc;
    add(pcs, pc & bucket_mask);
    if (callers) {
        // A windowed call keeps the window increment in the top two bits.
        add(callers, ((uint32_t)frame->a0 & 0x3FFFFFFF) | (pc & 0xC0000000));
    }
    total++;
    return false;
}

static void dump(void *arg)
{
    printf("SAMPLER total %u dropped %u nested %u hz %d slots %u\n", (unsigned)total,
           (unsigned)dropped, (unsigned)nested, SAMPLER_HZ, (unsigned)slots);
    for (uint32_t i = 0; i < slots; i++) {
        if (pcs[i].key) {
            printf("SPC %08x %u\n", (unsigned)pcs[i].key, (unsigned)pcs[i].count);
        }
    }
    for (uint32_t i = 0; callers && i < slots; i++) {
        if (callers[i].key) {
            printf("SCALLER %08x %u\n", (unsigned)callers[i].key, (unsigned)callers[i].count);
        }
    }
    printf("SAMPLER done\n");
}

static bool allocate_tables(void)
{
    slots = 32768;
    pcs = heap_caps_calloc(slots, sizeof(slot_t), MALLOC_CAP_SPIRAM);
    callers = heap_caps_calloc(slots, sizeof(slot_t), MALLOC_CAP_SPIRAM);
    if (pcs && callers) {
        return true;
    }
    // Without PSRAM: one table of 8-byte PC buckets in the IRAM heap, which
    // takes the 32-bit accesses the table makes and leaves the app's RAM.
    heap_caps_free(pcs);
    heap_caps_free(callers);
    callers = NULL;
    slots = 2048;
    bucket_mask = ~7u;
    pcs = heap_caps_malloc(slots * sizeof(slot_t), MALLOC_CAP_EXEC | MALLOC_CAP_32BIT);
    for (uint32_t i = 0; pcs && i < slots; i++) {
        pcs[i].key = 0;
        pcs[i].count = 0;
    }
    return pcs != NULL;
}

extern void __real_app_main(void);

void __wrap_app_main(void)
{
    if (!allocate_tables()) {
        printf("SAMPLER no memory\n");
        __real_app_main();
        return;
    }
    shift = 32 - __builtin_ctz(slots);
    gptimer_handle_t timer = NULL;
    gptimer_config_t config = {
        .clk_src = GPTIMER_CLK_SRC_DEFAULT,
        .direction = GPTIMER_COUNT_UP,
        .resolution_hz = 1000000,
    };
    gptimer_alarm_config_t alarm = {
        .alarm_count = 1000000 / SAMPLER_HZ,
        .reload_count = 0,
        .flags.auto_reload_on_alarm = true,
    };
    gptimer_event_callbacks_t callbacks = {.on_alarm = on_alarm};
    esp_timer_handle_t dumper;
    esp_timer_create_args_t dump_args = {.callback = dump, .name = "sampler"};
    // The table is printed from the esp_timer task: no task of its own,
    // whose stack would change the app's memory.
    if (gptimer_new_timer(&config, &timer) == ESP_OK
        && gptimer_set_alarm_action(timer, &alarm) == ESP_OK
        && gptimer_register_event_callbacks(timer, &callbacks, NULL) == ESP_OK
        && gptimer_enable(timer) == ESP_OK
        && gptimer_start(timer) == ESP_OK
        && esp_timer_create(&dump_args, &dumper) == ESP_OK) {
        esp_timer_start_once(dumper, STOP_US + 500000);
    } else {
        printf("SAMPLER no timer\n");
    }
    __real_app_main();
}
