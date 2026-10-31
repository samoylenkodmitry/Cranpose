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
// alignment is taken from the heap's plain allocation instead.
//
// A block aligned to 8 asks the heap for 4 bytes more. Half the heap's
// blocks start at an address aligned to 8, and the block starts there; the
// rest start 4 bytes off, and the block starts 4 bytes in, past a word that
// holds 1. The word before a block the heap hands out holds the block's
// size, whose lowest bit, the heap's mark of a free block, is clear, so the
// word before the block tells which. With heap poisoning on, that word holds
// the size the caller asked for, of any parity, and blocks aligned to 8 take
// the way blocks of larger alignments take: that alignment longer, starting
// at the first aligned address past the heap block's first word, where the
// distance back to the heap block is kept.

#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "esp_attr.h"
#include "esp_heap_caps.h"
#include "sdkconfig.h"

// Size classes up to 128 bytes: in steps of 4 for blocks of the heap's
// alignment, as the heap rounds sizes, and of 8 for blocks aligned to 8.
#define MAX_SMALL 128
#define HEAP_ALIGN 4
#define BLOCK_ALIGN 8
#define HEAP_CLASSES (MAX_SMALL / HEAP_ALIGN)
#define BLOCK_CLASSES (MAX_SMALL / BLOCK_ALIGN)
// A steady frame frees a few dozen small blocks; larger lists would hold
// memory the heap could give to others.
#define CACHED_BYTES 2048
// What the word before a block aligned to 8 holds when the block starts 4
// bytes into its heap block.
#define SHIFTED_MARK 1u

typedef struct {
    // The lists of blocks of the heap's alignment and of blocks aligned to 8.
    void *heap_aligned[HEAP_CLASSES];
    void *block_aligned[BLOCK_CLASSES];
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

// The class step of blocks of alignment `align`, which is at most 8.
static inline size_t step_of(size_t align)
{
    return align > HEAP_ALIGN ? BLOCK_ALIGN : HEAP_ALIGN;
}

// The step is 4 or 8: shifts by 2 or 3 take the place of a division.
static inline unsigned shift_of(size_t step)
{
    return step == HEAP_ALIGN ? 2 : 3;
}

static inline int class_of(size_t size, size_t step)
{
    return (int)((size + step - 1) >> shift_of(step)) - 1;
}

static inline size_t class_size(int class, size_t step)
{
    return (size_t)(class + 1) << shift_of(step);
}

static inline bool is_small(size_t size, size_t align)
{
    return size != 0 && size <= MAX_SMALL && align <= BLOCK_ALIGN;
}

// The list of the task's cache for blocks of class `class` and step `step`.
static inline void **list_of(block_cache_t *cache, int class, size_t step)
{
    return step == HEAP_ALIGN ? &cache->heap_aligned[class] : &cache->block_aligned[class];
}

#if CONFIG_HEAP_POISONING_DISABLED
// Whether a block of alignment `align` takes 4 bytes more and starts where
// its heap block's alignment lets it.
#define MARKS_SHIFT(align) ((align) == BLOCK_ALIGN)
#else
#define MARKS_SHIFT(align) false
#endif

// The distance from a block aligned to 8 back to its heap block.
static inline uint32_t mark_shift(const uint8_t *block)
{
    return (((const uint32_t *)block)[-1] & SHIFTED_MARK) ? HEAP_ALIGN : 0;
}

// The block that starts where `heap_block`'s alignment lets a block of
// alignment `align` start, with the word that says where the heap block is.
static IRAM_ATTR uint8_t *place_block(uint8_t *heap_block, size_t align)
{
    if (MARKS_SHIFT(align)) {
        if (((uintptr_t)heap_block & HEAP_ALIGN) == 0) {
            return heap_block;
        }
        ((uint32_t *)heap_block)[0] = SHIFTED_MARK;
        return heap_block + HEAP_ALIGN;
    }
    uint8_t *block = (uint8_t *)(((uintptr_t)heap_block + align) & ~(uintptr_t)(align - 1));
    ((uint32_t *)block)[-1] = (uint32_t)(block - heap_block);
    return block;
}

// The heap block of `block`, of alignment `align` past the heap's.
static IRAM_ATTR uint8_t *heap_block_of(uint8_t *block, size_t align)
{
    return block - (MARKS_SHIFT(align) ? mark_shift(block) : ((uint32_t *)block)[-1]);
}

// The bytes past a block of alignment `align` past the heap's that its heap
// block takes.
static inline size_t extra_of(size_t align)
{
    return MARKS_SHIFT(align) ? HEAP_ALIGN : align;
}

static IRAM_ATTR void *heap_alloc(size_t size, size_t align)
{
    if (align <= HEAP_ALIGN) {
        return heap_caps_malloc(size, MALLOC_CAP_DEFAULT);
    }
    uint8_t *heap_block = heap_caps_malloc(size + extra_of(align), MALLOC_CAP_DEFAULT);
    if (heap_block == NULL) {
        return NULL;
    }
    return place_block(heap_block, align);
}

static IRAM_ATTR void heap_free(void *ptr, size_t align)
{
    if (align <= HEAP_ALIGN) {
        heap_caps_free(ptr);
        return;
    }
    heap_caps_free(heap_block_of(ptr, align));
}

// Gives the blocks of a deleted task's lists back to the heap. ESP-IDF may
// run this on another task, so it reaches the lists through `arg`, the
// address of the deleted task's cache, whose TLS area lives until the task's
// stack is freed after its destructors.
static void drain_cache(void *arg)
{
    block_cache_t *cache = arg;
    for (size_t step = HEAP_ALIGN; step <= BLOCK_ALIGN; step *= 2) {
        for (int class = 0; class < (int)(MAX_SMALL / step); class++) {
            void **list = list_of(cache, class, step);
            void *block = *list;
            while (block != NULL) {
                void *next = *(void **)block;
                heap_free(block, step);
                block = next;
            }
            *list = NULL;
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
    size_t step = step_of(align);
    int class = class_of(size, step);
    void **list = list_of(&s_cache, class, step);
    void *block = *list;
    if (block != NULL) {
        *list = *(void **)block;
        s_cache.bytes -= class_size(class, step);
        return block;
    }
    return heap_alloc(class_size(class, step), step);
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
    size_t step = step_of(align);
    int class = class_of(size, step);
    block_cache_t *cache = &s_cache;
    if (!cache->registered) {
        // A task's first cached block: its destructor drains the lists when
        // the task is deleted. Without the key the block goes back.
        pthread_once(&s_cleanup_once, create_cleanup_key);
        cache->registered = s_cleanup_ready && pthread_setspecific(s_cleanup_key, cache) == 0;
        if (!cache->registered) {
            heap_free(ptr, step);
            return;
        }
    }
    if (cache->closed || cache->bytes + class_size(class, step) > CACHED_BYTES) {
        heap_free(ptr, step);
        return;
    }
    void **list = list_of(cache, class, step);
    *(void **)ptr = *list;
    *list = ptr;
    cache->bytes += class_size(class, step);
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
        size_t offset = (size_t)(block - heap_block_of(block, align));
        uint8_t *heap_block =
            heap_caps_realloc(block - offset, new_size + extra_of(align), MALLOC_CAP_DEFAULT);
        if (heap_block == NULL) {
            return NULL;
        }
        // The bytes move before the block's word is written: a block that
        // starts 4 bytes in may now start where its bytes do.
        size_t moved_offset = MARKS_SHIFT(align)
            ? ((uintptr_t)heap_block & HEAP_ALIGN ? HEAP_ALIGN : 0)
            : (size_t)((((uintptr_t)heap_block + align) & ~(uintptr_t)(align - 1)) - (uintptr_t)heap_block);
        if (moved_offset != offset) {
            memmove(heap_block + moved_offset, heap_block + offset, old_size < new_size ? old_size : new_size);
        }
        return place_block(heap_block, align);
    }
    void *moved = cranpose_small_alloc(new_size, align);
    if (moved != NULL) {
        memcpy(moved, ptr, old_size < new_size ? old_size : new_size);
        cranpose_small_free(ptr, old_size, align);
    }
    return moved;
}
