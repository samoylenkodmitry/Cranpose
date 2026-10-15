// Allocation for the Rust global allocator of cranpose-platform-esp-idf.
//
// ESP-IDF's heap takes a spinlock and walks TLSF lists for each call, and
// an 8-byte aligned block goes through heap_caps_aligned_alloc: about 1,000
// instructions for an allocation and its free. A frame of a Cranpose app
// allocates and frees the same few small blocks again and again.
//
// Each task keeps up to MAX_CACHED freed blocks of each size class in lists
// of its own, in C thread-locals that ESP-IDF places in the task's TLS area:
// a block of up to MAX_SMALL bytes and alignment of up to 8 comes from the
// task's list when it holds one, in a few instructions and without a lock.
// Every block of a class is allocated at the class's size, so any block a
// list holds fits any request of its class. A block may be freed on another
// task than the one that allocated it: it joins that task's list. When a task
// is deleted, a pthread key destructor gives its blocks back to the heap.

#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "esp_heap_caps.h"

#define CLASSES 10
#define MAX_SMALL 128
#define MAX_CACHED 8
#define BLOCK_ALIGN 8

// Size classes: 8 to 64 bytes in steps of 8, then 96 and 128.
static const uint8_t s_class_size[CLASSES] = {8, 16, 24, 32, 40, 48, 56, 64, 96, 128};

typedef struct {
    void *head[CLASSES];
    uint8_t count[CLASSES];
    bool registered;
    // Drained: the task is being deleted, and what it frees from here on
    // goes back to the heap.
    bool closed;
} block_cache_t;

static __thread block_cache_t s_cache;

static pthread_key_t s_cleanup_key;
static pthread_once_t s_cleanup_once = PTHREAD_ONCE_INIT;
static bool s_cleanup_ready;

static inline int class_of(size_t size)
{
    if (size <= 64) {
        return (int)((size + 7) / 8) - 1;
    }
    return size <= 96 ? 8 : 9;
}

static inline bool is_small(size_t size, size_t align)
{
    return size != 0 && size <= MAX_SMALL && align <= BLOCK_ALIGN;
}

// Gives the blocks of a deleted task's lists back to the heap. ESP-IDF may
// run this on another task, so it reaches the lists through `arg`, the
// address of the deleted task's cache, whose TLS area lives until the task's
// stack is freed after its destructors.
static void drain_cache(void *arg)
{
    block_cache_t *cache = arg;
    for (int class = 0; class < CLASSES; class++) {
        void *block = cache->head[class];
        while (block != NULL) {
            void *next = *(void **)block;
            heap_caps_free(block);
            block = next;
        }
        cache->head[class] = NULL;
        cache->count[class] = 0;
    }
    cache->closed = true;
}

static void create_cleanup_key(void)
{
    s_cleanup_ready = pthread_key_create(&s_cleanup_key, drain_cache) == 0;
}

static void *heap_alloc(size_t size, size_t align)
{
    if (align <= 4) {
        return heap_caps_malloc(size, MALLOC_CAP_DEFAULT);
    }
    return heap_caps_aligned_alloc(align, size, MALLOC_CAP_DEFAULT);
}

void *cranpose_small_alloc(size_t size, size_t align)
{
    if (!is_small(size, align)) {
        return heap_alloc(size, align);
    }
    int class = class_of(size);
    block_cache_t *cache = &s_cache;
    void *block = cache->head[class];
    if (block != NULL) {
        cache->head[class] = *(void **)block;
        cache->count[class]--;
        return block;
    }
    return heap_caps_aligned_alloc(BLOCK_ALIGN, s_class_size[class], MALLOC_CAP_DEFAULT);
}

void cranpose_small_free(void *ptr, size_t size, size_t align)
{
    if (ptr == NULL) {
        return;
    }
    if (!is_small(size, align)) {
        heap_caps_free(ptr);
        return;
    }
    int class = class_of(size);
    block_cache_t *cache = &s_cache;
    if (!cache->registered) {
        // A task's first cached block: its destructor drains the lists when
        // the task is deleted. Without the key the block goes back.
        pthread_once(&s_cleanup_once, create_cleanup_key);
        cache->registered = s_cleanup_ready && pthread_setspecific(s_cleanup_key, cache) == 0;
        if (!cache->registered) {
            heap_caps_free(ptr);
            return;
        }
    }
    if (cache->closed || cache->count[class] >= MAX_CACHED) {
        heap_caps_free(ptr);
        return;
    }
    *(void **)ptr = cache->head[class];
    cache->head[class] = ptr;
    cache->count[class]++;
}

void *cranpose_small_realloc(void *ptr, size_t old_size, size_t align, size_t new_size)
{
    // Large blocks of the heap's own alignment grow in place when the heap
    // can; the heap keeps no larger alignment across a resize.
    if (!is_small(old_size, align) && !is_small(new_size, align) && align <= 4) {
        return heap_caps_realloc(ptr, new_size, MALLOC_CAP_DEFAULT);
    }
    void *moved = cranpose_small_alloc(new_size, align);
    if (moved != NULL) {
        memcpy(moved, ptr, old_size < new_size ? old_size : new_size);
        cranpose_small_free(ptr, old_size, align);
    }
    return moved;
}
