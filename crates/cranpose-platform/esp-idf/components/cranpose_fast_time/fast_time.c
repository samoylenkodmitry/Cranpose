// Rust's `Instant::now` reads CLOCK_MONOTONIC through clock_gettime, which
// ESP-IDF answers from the microseconds since boot, split with a 64-bit `/`
// and `%` by 1000000. The S3 has no 64-bit division: libgcc's __udivdi3 and
// __umoddi3 take about 90 instructions together.
//
// The link of this component wraps clock_gettime. For CLOCK_MONOTONIC it
// reads the same microseconds and splits them with 32-bit divisions by a
// constant, which compile to multiplications: us = 64 * x + low and
// x = 15625 * q + r give us = 1000000 * q + (64 * r + low), where
// 64 * r + low < 1000000. The values are the ones ESP-IDF gives. Other
// clocks go to ESP-IDF's clock_gettime.

#include <errno.h>
#include <stdint.h>
#include <time.h>

// ESP-IDF's source of CLOCK_MONOTONIC (newlib's private esp_time_impl.h).
uint64_t esp_time_impl_get_time(void);

int __real_clock_gettime(clockid_t clock_id, struct timespec *tp);

int __wrap_clock_gettime(clockid_t clock_id, struct timespec *tp)
{
    if (clock_id != CLOCK_MONOTONIC || tp == NULL) {
        return __real_clock_gettime(clock_id, tp);
    }
    uint64_t us = esp_time_impl_get_time();
    uint64_t x = us >> 6;
    uint32_t high = (uint32_t)(x >> 32);
    uint32_t low = (uint32_t)x;
    // Long division of x by 15625, the high word first, then the low word
    // in 16-bit digits: each partial dividend stays below 15625 << 16.
    uint32_t q_high = high / 15625u;
    uint32_t r = high % 15625u;
    uint32_t part = (r << 16) | (low >> 16);
    uint32_t q_mid = part / 15625u;
    r = part % 15625u;
    part = (r << 16) | (low & 0xFFFFu);
    uint32_t q_low = part / 15625u;
    r = part % 15625u;
    tp->tv_sec = (time_t)(((uint64_t)q_high << 32) | (q_mid << 16) | q_low);
    tp->tv_nsec = (long)(((r << 6) | (uint32_t)(us & 63u)) * 1000u);
    return 0;
}
