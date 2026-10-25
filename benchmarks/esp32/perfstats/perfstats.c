// Counts events of core 0, where the UI task runs, with two Xtensa
// performance counters at a time, over 5 s windows after a first one that
// warms up: each window prints one pair, the time core 0 spent idle and the
// heap, and sets up the next pair. An esp_timer callback does it; the
// esp_timer task runs on core 0.
#include <stdbool.h>
#include <stdio.h>
#include "esp_heap_caps.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "xtensa/xt_perf_consts.h"
#include "xtensa_perfmon_access.h"

typedef struct {
    const char *name;
    uint16_t select;
    uint16_t mask;
} event_t;

static const event_t PAIRS[][2] = {
    {{"cycles", XTPERF_CNT_CYCLES, XTPERF_MASK_CYCLES}, {"instructions", XTPERF_CNT_INSN, XTPERF_MASK_INSN_ALL}},
    {{"i-stall", XTPERF_CNT_I_STALL, XTPERF_MASK_I_STALL_ALL}, {"d-stall", XTPERF_CNT_D_STALL, XTPERF_MASK_D_STALL_ALL}},
    {{"i-stall-busy", XTPERF_CNT_I_STALL, XTPERF_MASK_I_STALL_BUSY}, {"d-stall-busy", XTPERF_CNT_D_STALL, XTPERF_MASK_D_STALL_BUSY}},
};
#define PAIR_COUNT (sizeof(PAIRS) / sizeof(PAIRS[0]))

static unsigned pair;
static int64_t window_start;
static configRUN_TIME_COUNTER_TYPE idle_start;
// The first window holds the start and the first frame, so it only warms up.
static bool warm;

// The microseconds core 0's idle task has run: the rest of a window is the
// work of the UI and of the drivers and timers it uses.
static configRUN_TIME_COUNTER_TYPE idle_time(void)
{
    return ulTaskGetIdleRunTimeCounterForCore(0);
}

static void arm(void)
{
    for (int id = 0; id < 2; id++) {
        xtensa_perfmon_init(id, PAIRS[pair][id].select, PAIRS[pair][id].mask, 0, 15);
        xtensa_perfmon_reset(id);
    }
    window_start = esp_timer_get_time();
    idle_start = idle_time();
    xtensa_perfmon_start();
}

static void report(void *arg)
{
    xtensa_perfmon_stop();
    int64_t now = esp_timer_get_time();
    if (!warm) {
        warm = true;
        arm();
        return;
    }
    printf("PERF %.3f s %s %u %s %u idle %.3f s\n", (now - window_start) / 1e6,
           PAIRS[pair][0].name, (unsigned)xtensa_perfmon_value(0),
           PAIRS[pair][1].name, (unsigned)xtensa_perfmon_value(1),
           (idle_time() - idle_start) / 1e6);
    // Byte-addressable internal RAM: the IRAM heap of the classic ESP32 only
    // takes 32-bit allocations, which neither app uses.
    const uint32_t ram = MALLOC_CAP_INTERNAL | MALLOC_CAP_8BIT;
    printf("HEAP internal free %u lowest %u largest %u, psram free %u\n",
           (unsigned)heap_caps_get_free_size(ram),
           (unsigned)heap_caps_get_minimum_free_size(ram),
           (unsigned)heap_caps_get_largest_free_block(ram),
           (unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));
    pair = (pair + 1) % PAIR_COUNT;
    arm();
}

extern void __real_app_main(void);

void __wrap_app_main(void)
{
    esp_timer_handle_t timer;
    esp_timer_create_args_t args = {.callback = report, .name = "perfstats"};
    if (esp_timer_create(&args, &timer) == ESP_OK) {
        arm();
        esp_timer_start_periodic(timer, 5000000);
    }
    __real_app_main();
}
