// A QEMU TCG plugin that counts the instruction cache misses of core 0.
//
// Code fetched from [lo, hi), the range the chip reads through its cache,
// goes through a set-associative LRU cache of `size` bytes, `ways` ways and
// `line`-byte lines; IRAM and ROM need no cache. A frame starts each time
// core 0 runs the instruction at `frame`. After `skip` frames the plugin
// counts `frames` frames and then writes to `out`:
//
//   frame N insn I misses M
//   line 0xADDR misses M
//
// for each frame, then for each line that missed. Options: lo, hi, size,
// ways, line, frame, skip, frames, out. The ESP32-S3's defaults: its 32 KB
// instruction cache of 8 ways and 32-byte lines over 0x42000000-0x44000000.
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <glib.h>
#include <qemu-plugin.h>

QEMU_PLUGIN_EXPORT int qemu_plugin_version = QEMU_PLUGIN_VERSION;

static uint64_t lo = 0x42000000, hi = 0x44000000;
static uint64_t cache_size = 32768, ways = 8, line_size = 32;
static uint64_t frame_pc;
static uint64_t skip = 60, frames = 60;
static char out_path[1024] = "icache.txt";

static unsigned line_shift;
static uint64_t sets;
// For each set, its ways: the line held plus one (0 when empty) and when it
// was last used.
static uint64_t *tags, *stamps, clock_;
static uint32_t *line_misses;

typedef struct {
    uint64_t insn, misses;
} FrameCount;

static FrameCount current, *counted;
static uint64_t frame_no, n_counted;
static bool done;

typedef struct {
    uint64_t first_line, last_line;
    uint32_t insns;
    bool cached, starts_frame;
} Block;

static bool counting(void)
{
    return frame_no > skip && !done;
}

static void write_out(void)
{
    FILE *f = fopen(out_path, "w");
    if (!f) {
        return;
    }
    for (uint64_t i = 0; i < n_counted; i++) {
        fprintf(f, "frame %" PRIu64 " insn %" PRIu64 " misses %" PRIu64 "\n", i, counted[i].insn,
                counted[i].misses);
    }
    uint64_t n = (hi - lo) >> line_shift;
    for (uint64_t i = 0; i < n; i++) {
        if (line_misses[i]) {
            fprintf(f, "line 0x%" PRIx64 " misses %u\n", lo + (i << line_shift), line_misses[i]);
        }
    }
    fclose(f);
}

static void next_frame(void)
{
    if (counting()) {
        counted[n_counted++] = current;
        if (n_counted == frames) {
            done = true;
            write_out();
        }
    }
    memset(&current, 0, sizeof(current));
    frame_no++;
}

static void fetch(uint64_t line)
{
    uint64_t *tag = &tags[(line % sets) * ways];
    uint64_t *stamp = &stamps[(line % sets) * ways];
    uint64_t victim = 0;
    clock_++;
    for (uint64_t w = 0; w < ways; w++) {
        if (tag[w] == line + 1) {
            stamp[w] = clock_;
            return;
        }
        if (stamp[w] < stamp[victim]) {
            victim = w;
        }
    }
    tag[victim] = line + 1;
    stamp[victim] = clock_;
    if (counting()) {
        current.misses++;
        line_misses[line - (lo >> line_shift)]++;
    }
}

static void block_exec(unsigned int vcpu, void *data)
{
    Block *block = data;
    if (vcpu != 0) {
        return;
    }
    if (block->starts_frame) {
        next_frame();
    }
    if (counting()) {
        current.insn += block->insns;
    }
    if (block->cached) {
        for (uint64_t line = block->first_line; line <= block->last_line; line++) {
            fetch(line);
        }
    }
}

static void block_trans(qemu_plugin_id_t id, struct qemu_plugin_tb *tb)
{
    size_t n = qemu_plugin_tb_n_insns(tb);
    if (n == 0) {
        return;
    }
    Block *block = g_new0(Block, 1);
    uint64_t start = qemu_plugin_tb_vaddr(tb);
    struct qemu_plugin_insn *last = qemu_plugin_tb_get_insn(tb, n - 1);
    uint64_t end = qemu_plugin_insn_vaddr(last) + qemu_plugin_insn_size(last);
    block->insns = n;
    block->cached = start >= lo && end <= hi;
    block->first_line = start >> line_shift;
    block->last_line = (end - 1) >> line_shift;
    for (size_t i = 0; i < n; i++) {
        if (frame_pc && qemu_plugin_insn_vaddr(qemu_plugin_tb_get_insn(tb, i)) == frame_pc) {
            block->starts_frame = true;
        }
    }
    qemu_plugin_register_vcpu_tb_exec_cb(tb, block_exec, QEMU_PLUGIN_CB_NO_REGS, block);
}

static void at_exit(qemu_plugin_id_t id, void *data)
{
    if (!done) {
        write_out();
    }
}

QEMU_PLUGIN_EXPORT int qemu_plugin_install(qemu_plugin_id_t id, const qemu_info_t *info,
                                           int argc, char **argv)
{
    for (int i = 0; i < argc; i++) {
        char *eq = strchr(argv[i], '=');
        if (!eq) {
            continue;
        }
        *eq = 0;
        const char *key = argv[i], *value = eq + 1;
        uint64_t number = strtoull(value, NULL, 0);
        if (!strcmp(key, "lo")) {
            lo = number;
        } else if (!strcmp(key, "hi")) {
            hi = number;
        } else if (!strcmp(key, "size")) {
            cache_size = number;
        } else if (!strcmp(key, "ways")) {
            ways = number;
        } else if (!strcmp(key, "line")) {
            line_size = number;
        } else if (!strcmp(key, "frame")) {
            frame_pc = number;
        } else if (!strcmp(key, "skip")) {
            skip = number;
        } else if (!strcmp(key, "frames")) {
            frames = number;
        } else if (!strcmp(key, "out")) {
            snprintf(out_path, sizeof(out_path), "%s", value);
        }
    }
    line_shift = __builtin_ctzll(line_size);
    sets = cache_size / (ways * line_size);
    tags = calloc(sets * ways, sizeof(uint64_t));
    stamps = calloc(sets * ways, sizeof(uint64_t));
    line_misses = calloc((hi - lo) >> line_shift, sizeof(uint32_t));
    counted = calloc(frames, sizeof(FrameCount));
    if (!tags || !stamps || !line_misses || !counted) {
        return -1;
    }
    qemu_plugin_register_vcpu_tb_trans_cb(id, block_trans);
    qemu_plugin_register_atexit_cb(id, at_exit, NULL);
    return 0;
}
