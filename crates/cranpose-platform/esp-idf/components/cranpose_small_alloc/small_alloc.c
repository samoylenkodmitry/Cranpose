// Allocation for the Rust global allocator of cranpose-platform-esp-idf.
//
// ESP-IDF's heap takes a spinlock and walks TLSF lists for each call: about
// 500 instructions for an allocation and its free. A frame of a Cranpose app
// allocates and frees the same few small blocks again and again.
//
// Each task keeps up to CACHED_BYTES of freed blocks in lists of its own, one
// for each size class, in C thread-locals that ESP-IDF places in the task's
// TLS area: a block of up to MAX_SMALL bytes and alignment of up to 8 comes
// from the task's list when it holds one, in a few instructions and without a
// lock.
// Every block of a class is allocated at the class's size, so any block a
// list holds fits any request of its class. A block may be freed on another
// task than the one that allocated it: it joins that task's list. When a task
// is deleted, a pthread key destructor gives its blocks back to the heap.
//
// The heap aligns its blocks to 4 bytes. Its own aligned allocation looks for
// a free block 24 bytes longer than asked and, for a block it finds 4 bytes
// off, cuts the 20 bytes before it off as a free block: the hole a freed
// block leaves is then too short for the next request of its size, and the
// free memory ends up in holes that no request fits. A block of a larger
// alignment is taken from the heap's plain allocation instead, that alignment
// longer, and starts at the first aligned address past the heap block's first
// word, where the distance back to the heap block is kept.

#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "esp_attr.h"
#include "esp_heap_caps.h"

// Size classes: 8 to 128 bytes in steps of 8, for blocks of either alignment.
#define CLASS_STEP 8
#define CLASSES 16
#define MAX_SMALL (CLASS_STEP * CLASSES)
// A steady frame frees a few dozen small blocks; larger lists would hold
// memory the heap could give to others.
#define CACHED_BYTES 2048
#define HEAP_ALIGN 4
#define BLOCK_ALIGN 8

// The lists of blocks of the heap's alignment and of blocks aligned to 8.
#define KINDS 2

typedef struct {
    void *head[KINDS][CLASSES];
    uint16_t bytes;
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
    return (int)((size + CLASS_STEP - 1) / CLASS_STEP) - 1;
}

static inline size_t class_size(int class)
{
    return (size_t)(class + 1) * CLASS_STEP;
}

static inline bool is_small(size_t size, size_t align)
{
    return size != 0 && size <= MAX_SMALL && align <= BLOCK_ALIGN;
}

static inline int kind_of(size_t align)
{
    return align > HEAP_ALIGN;
}

static void *heap_alloc(size_t size, size_t align)
{
    if (align <= HEAP_ALIGN) {
        return heap_caps_malloc(size, MALLOC_CAP_DEFAULT);
    }
    uint8_t *heap_block = heap_caps_malloc(size + align, MALLOC_CAP_DEFAULT);
    if (heap_block == NULL) {
        return NULL;
    }
    uint8_t *block = (uint8_t *)(((uintptr_t)heap_block + align) & ~(uintptr_t)(align - 1));
    ((uint32_t *)block)[-1] = (uint32_t)(block - heap_block);
    return block;
}

static void heap_free(void *ptr, size_t align)
{
    if (align <= HEAP_ALIGN) {
        heap_caps_free(ptr);
        return;
    }
    uint8_t *block = ptr;
    heap_caps_free(block - ((uint32_t *)block)[-1]);
}

// Gives the blocks of a deleted task's lists back to the heap. ESP-IDF may
// run this on another task, so it reaches the lists through `arg`, the
// address of the deleted task's cache, whose TLS area lives until the task's
// stack is freed after its destructors.
static void drain_cache(void *arg)
{
    block_cache_t *cache = arg;
    for (int kind = 0; kind < KINDS; kind++) {
        for (int class = 0; class < CLASSES; class++) {
            void *block = cache->head[kind][class];
            while (block != NULL) {
                void *next = *(void **)block;
                heap_free(block, kind ? BLOCK_ALIGN : HEAP_ALIGN);
                block = next;
            }
            cache->head[kind][class] = NULL;
        }
    }
    cache->bytes = 0;
    cache->closed = true;
}

static void create_cleanup_key(void)
{
    s_cleanup_ready = pthread_key_create(&s_cleanup_key, drain_cache) == 0;
}

// Every Rust allocation runs these three; from IRAM they take no room in the
// instruction cache.
IRAM_ATTR void *cranpose_small_alloc(size_t size, size_t align)
{
    if (!is_small(size, align)) {
        return heap_alloc(size, align);
    }
    int kind = kind_of(align);
    int class = class_of(size);
    block_cache_t *cache = &s_cache;
    void *block = cache->head[kind][class];
    if (block != NULL) {
        cache->head[kind][class] = *(void **)block;
        cache->bytes -= class_size(class);
        return block;
    }
    return heap_alloc(class_size(class), kind ? BLOCK_ALIGN : HEAP_ALIGN);
}

IRAM_ATTR void cranpose_small_free(void *ptr, size_t size, size_t align)
{
    if (ptr == NULL) {
        return;
    }
    if (!is_small(size, align)) {
        heap_free(ptr, align);
        return;
    }
    int kind = kind_of(align);
    int class = class_of(size);
    block_cache_t *cache = &s_cache;
    if (!cache->registered) {
        // A task's first cached block: its destructor drains the lists when
        // the task is deleted. Without the key the block goes back.
        pthread_once(&s_cleanup_once, create_cleanup_key);
        cache->registered = s_cleanup_ready && pthread_setspecific(s_cleanup_key, cache) == 0;
        if (!cache->registered) {
            heap_free(ptr, kind ? BLOCK_ALIGN : HEAP_ALIGN);
            return;
        }
    }
    if (cache->closed || cache->bytes + class_size(class) > CACHED_BYTES) {
        heap_free(ptr, kind ? BLOCK_ALIGN : HEAP_ALIGN);
        return;
    }
    *(void **)ptr = cache->head[kind][class];
    cache->head[kind][class] = ptr;
    cache->bytes += class_size(class);
}

IRAM_ATTR void *cranpose_small_realloc(void *ptr, size_t old_size, size_t align, size_t new_size)
{
    // Large blocks grow in place when the heap can. A block of a larger
    // alignment than the heap's may land at another distance from its new
    // heap block's aligned address, and its bytes move there.
    if (!is_small(old_size, align) && !is_small(new_size, align)) {
        if (align <= HEAP_ALIGN) {
            return heap_caps_realloc(ptr, new_size, MALLOC_CAP_DEFAULT);
        }
        uint8_t *block = ptr;
        uint32_t offset = ((uint32_t *)block)[-1];
        uint8_t *heap_block = heap_caps_realloc(block - offset, new_size + align, MALLOC_CAP_DEFAULT);
        if (heap_block == NULL) {
            return NULL;
        }
        uint8_t *moved = (uint8_t *)(((uintptr_t)heap_block + align) & ~(uintptr_t)(align - 1));
        if (moved != heap_block + offset) {
            memmove(moved, heap_block + offset, old_size < new_size ? old_size : new_size);
        }
        ((uint32_t *)moved)[-1] = (uint32_t)(moved - heap_block);
        return moved;
    }
    void *moved = cranpose_small_alloc(new_size, align);
    if (moved != NULL) {
        memcpy(moved, ptr, old_size < new_size ? old_size : new_size);
        cranpose_small_free(ptr, old_size, align);
    }
    return moved;
}
