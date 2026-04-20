/*
 * Stub C implementations of the libghostty-vt symbols con-ghostty's
 * `src/windows/vt.rs` binds. Used when `CON_STUB_GHOSTTY_VT=1` so a
 * cargo build can link without a working Zig/libghostty-vt toolchain.
 *
 * Signatures mirror `include/ghostty/vt/{terminal,render,allocator}.h`
 * at GHOSTTY_REV `ca7516bea60190ee2e9a4f9182b61d318d107c6e` — keep in
 * sync with vt.rs on upstream bumps.
 *
 * All calls return empty / false / zero so downstream code degrades
 * gracefully to "empty terminal grid, clear-color render". The rest
 * of the Windows backend (GPUI host view, ConPTY spawn, D3D11
 * swapchain, atlas setup) still exercises fully.
 */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef void* GhosttyTerminal;
typedef void* GhosttyRenderState;
typedef void* GhosttyRowIterator;
typedef void* GhosttyRowCells;
typedef int   GhosttyResult;

struct GhosttyTerminalOptions {
    uint16_t cols;
    uint16_t rows;
    size_t   max_scrollback;
};

/* ── Terminal lifecycle ─────────────────────────────────────────── */

GhosttyResult ghostty_terminal_new(
    const void* allocator,
    GhosttyTerminal* out_terminal,
    struct GhosttyTerminalOptions options
) {
    (void)allocator; (void)options;
    if (out_terminal) { *out_terminal = (void*)(uintptr_t)1; }
    return 0;
}

void ghostty_terminal_free(GhosttyTerminal terminal) { (void)terminal; }

GhosttyResult ghostty_terminal_resize(
    GhosttyTerminal terminal, uint16_t cols, uint16_t rows,
    uint32_t cell_width_px, uint32_t cell_height_px
) {
    (void)terminal; (void)cols; (void)rows;
    (void)cell_width_px; (void)cell_height_px;
    return 0;
}

void ghostty_terminal_vt_write(GhosttyTerminal terminal, const uint8_t* data, size_t len) {
    (void)terminal; (void)data; (void)len;
}

GhosttyResult ghostty_terminal_get(
    GhosttyTerminal terminal, int key, void* out
) {
    (void)terminal; (void)key;
    if (out) { *(uint8_t*)out = 0; }
    return 0;
}

GhosttyResult ghostty_terminal_mode_get(
    GhosttyTerminal terminal, uint16_t mode, bool* out_value
) {
    (void)terminal; (void)mode;
    if (out_value) { *out_value = false; }
    return 0;
}

/* ── Cell accessor ──────────────────────────────────────────────── */

GhosttyResult ghostty_cell_get(uint64_t cell, int key, void* out) {
    (void)cell; (void)key;
    if (out) { *(uint8_t*)out = 0; }
    return 0;
}

/* ── Render state ───────────────────────────────────────────────── */

GhosttyResult ghostty_render_state_new(
    const void* allocator, GhosttyRenderState* out_state
) {
    (void)allocator;
    if (out_state) { *out_state = (void*)(uintptr_t)2; }
    return 0;
}

void ghostty_render_state_free(GhosttyRenderState state) { (void)state; }

GhosttyResult ghostty_render_state_update(
    GhosttyRenderState state, GhosttyTerminal terminal
) {
    (void)state; (void)terminal;
    return 0;
}

GhosttyResult ghostty_render_state_get(
    GhosttyRenderState state, int key, void* out
) {
    (void)state; (void)key;
    if (out) { *(uint8_t*)out = 0; }
    return 0;
}

GhosttyResult ghostty_render_state_row_iterator_new(
    const void* allocator, GhosttyRowIterator* out_iter
) {
    (void)allocator;
    if (out_iter) { *out_iter = (void*)(uintptr_t)3; }
    return 0;
}
void ghostty_render_state_row_iterator_free(GhosttyRowIterator iter) { (void)iter; }
bool ghostty_render_state_row_iterator_next(GhosttyRowIterator iter) { (void)iter; return false; }

GhosttyResult ghostty_render_state_row_get(
    GhosttyRowIterator iter, int key, void* out
) {
    (void)iter; (void)key;
    if (out) { *(uint8_t*)out = 0; }
    return 0;
}

GhosttyResult ghostty_render_state_row_cells_new(
    const void* allocator, GhosttyRowCells* out_cells
) {
    (void)allocator;
    if (out_cells) { *out_cells = (void*)(uintptr_t)4; }
    return 0;
}
void ghostty_render_state_row_cells_free(GhosttyRowCells cells) { (void)cells; }
bool ghostty_render_state_row_cells_next(GhosttyRowCells cells) { (void)cells; return false; }

GhosttyResult ghostty_render_state_row_cells_get(
    GhosttyRowCells cells, int key, void* out
) {
    (void)cells; (void)key;
    if (out) { *(uint8_t*)out = 0; }
    return 0;
}
