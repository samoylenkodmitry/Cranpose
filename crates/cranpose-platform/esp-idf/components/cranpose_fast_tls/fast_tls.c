// Rust's std keeps each `thread_local!` of an ESP-IDF build in a pthread
// key. ESP-IDF's pthread_getspecific asks FreeRTOS for the current task,
// which masks interrupts, and walks the task's list of key values: about
// 90 instructions, paid by every thread-local access.
//
// The links of this component wrap both calls. Each task keeps a copy of
// its values of the first MIRRORED_KEYS keys in a C11 thread-local array,
// which ESP-IDF places in the task's own TLS area and reaches through the
// THREADPTR register. Setting a value updates the pthread list as before and
// the copy; reading one of those keys reads the copy. ESP-IDF numbers keys
// from 1, keeps a deleted key's values in every task's list and may hand the
// number out again: the copy keeps them the same way, so both always answer
// alike.

#include <pthread.h>

#define MIRRORED_KEYS 32

static __thread void *s_values[MIRRORED_KEYS];

void *__real_pthread_getspecific(pthread_key_t key);
int __real_pthread_setspecific(pthread_key_t key, const void *value);

void *__wrap_pthread_getspecific(pthread_key_t key)
{
    if (key < MIRRORED_KEYS) {
        return s_values[key];
    }
    return __real_pthread_getspecific(key);
}

int __wrap_pthread_setspecific(pthread_key_t key, const void *value)
{
    int result = __real_pthread_setspecific(key, value);
    if (result == 0 && key < MIRRORED_KEYS) {
        s_values[key] = (void *)value;
    }
    return result;
}
