/*
 * Stub C implementations of the libghostty-vt symbols con-ghostty's
 * `src/windows/vt.rs` binds. Used when `CON_STUB_GHOSTTY_VT=1` so a
 * cargo build can link without a working Zig/libghostty-vt toolchain.
 *
 * All functions return empty/failure results; the rest of the con
 * Windows backend (GPUI host view, ConPTY spawn, D3D11 swapchain,
 * atlas setup) still exercises fully so you can iterate those paths
 * on real Windows hardware while libghostty-vt build issues are
 * resolved separately.
 *
 * Signatures MUST stay in sync with crates/con-ghostty/src/windows/vt.rs.
 */

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
    if (out_terminal) { *out_terminal = NULL; }
    /* Return a non-null sentinel so con-ghostty's error path doesn't
     * trigger (which would disable the HostView entirely). Callers
     * only test equality against NULL; the pointer value is opaque. */
    if (out_terminal) { *out_terminal = (void*)(uintptr_t)1; }
    return 0;
}

void ghostty_terminal_free(GhosttyTerminal terminal) { (void)terminal; }
void ghostty_terminal_reset(GhosttyTerminal terminal) { (void)terminal; }

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
    /* Zero-fill the caller's out-param so cursor reads return 0/0/false. */
    if (out) {
        /* Conservative: write 1 byte. The rs side zero-inits most of
         * these locals before the call anyway. */
        *(uint8_t*)out = 0;
    }
    return 0;
}

GhosttyResult ghostty_terminal_set(
    GhosttyTerminal terminal, int key, const void* value
) {
    (void)terminal; (void)key; (void)value;
    return 0;
}

/* ── Render state ───────────────────────────────────────────────── */

GhosttyResult ghostty_render_state_new(
    GhosttyTerminal terminal, GhosttyRenderState* out_state
) {
    (void)terminal;
    if (out_state) { *out_state = (void*)(uintptr_t)2; }
    return 0;
}

void ghostty_render_state_free(GhosttyRenderState state) { (void)state; }
GhosttyResult ghostty_render_state_update(GhosttyRenderState state) { (void)state; return 0; }

GhosttyResult ghostty_render_state_row_iterator_new(
    GhosttyRenderState state, GhosttyRowIterator* out_iter
) {
    (void)state;
    if (out_iter) { *out_iter = NULL; }
    return 0;
}
void ghostty_render_state_row_iterator_free(GhosttyRowIterator iter) { (void)iter; }
/* Return 0 = no more rows. con-ghostty's snapshot loop then does
 * nothing and returns an all-defaults snapshot — renderer clears to
 * the background color and presents. */
int ghostty_render_state_row_iterator_next(GhosttyRowIterator iter) { (void)iter; return 0; }

GhosttyResult ghostty_render_state_row_get(
    GhosttyRowIterator iter, int key, void* out
) {
    (void)iter; (void)key;
    if (out) { *(uint8_t*)out = 0; }
    return 0;
}

GhosttyResult ghostty_render_state_row_cells_new(
    GhosttyRowIterator iter, GhosttyRowCells* out_cells
) {
    (void)iter;
    if (out_cells) { *out_cells = NULL; }
    return 0;
}
void ghostty_render_state_row_cells_free(GhosttyRowCells cells) { (void)cells; }
int ghostty_render_state_row_cells_next(GhosttyRowCells cells) { (void)cells; return 0; }

GhosttyResult ghostty_render_state_row_cells_get(
    GhosttyRowCells cells, int key, void* out
) {
    (void)cells; (void)key;
    if (out) { *(uint8_t*)out = 0; }
    return 0;
}
