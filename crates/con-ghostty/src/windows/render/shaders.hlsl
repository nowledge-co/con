// Con Windows terminal renderer — HLSL.
//
// Single DrawIndexedInstanced(6, cells) per frame. Each cell is a quad
// with per-instance inputs (IA layout, matching `VS_INSTANCE` in
// pipeline.rs). VS computes the cell's screen-space position from its
// grid coords; PS samples the grayscale glyph atlas (BGRA8, coverage in
// the red channel) and lerps bg→fg by coverage.
//
// Grayscale, not ClearType: portable across rotated panels / OLED pentile
// / external monitors, avoids dual-source blending, matches Alacritty /
// Kitty / Ghostty defaults.

// ── Constant buffer (per-frame) ────────────────────────────────────────
cbuffer Globals : register(b0) {
    // Pixel-to-NDC factors: 2.0/viewport_px per axis; also flips Y.
    float2 invViewport;
    // Logical cell size in physical pixels.
    float2 cellSize;
    // Grid dims — for cursor / debug overlays.
    uint   gridCols;
    uint   gridRows;
    // Inverse of atlas texture dimensions (1/width, 1/height). Used
    // to normalize the atlas UV (which we pass in pixel coords for
    // simplicity) into the [0,1] range the D3D sampler expects.
    float2 invAtlasSize;
};

// ── Per-instance inputs ────────────────────────────────────────────────
struct VSInstance {
    // (col, row) of this cell in the grid.
    uint2  cellPos       : CELLPOS;
    // (x, y, w, h) of this cell's glyph in atlas pixels.
    uint4  atlasRect     : ATLAS;
    // Foreground RGBA8 (srgb).
    uint   fg            : FGCOLOR;
    // Background RGBA8.
    uint   bg            : BGCOLOR;
    // attrs: bit 0 = bold, 1 = italic, 2 = underline, 3 = strike,
    // 4 = inverse. Unused low bits reserved.
    uint   attrs         : ATTRS;
};

struct VSVertex {
    // Built-in quad corner id (0..3). We use a 6-index strip, so vid %
    // 4 maps to the corner: 0 = top-left, 1 = top-right, 2 = bot-left,
    // 3 = bot-right.
    uint   vid           : SV_VertexID;
};

struct VSOut {
    float4 pos           : SV_Position;
    float2 atlasUV       : TEXCOORD0;
    nointerpolation float4 fg : FGCOLOR;
    nointerpolation float4 bg : BGCOLOR;
    nointerpolation uint   attrs : ATTRS;
};

// ── Helpers ────────────────────────────────────────────────────────────
float4 unpackRGBA(uint v) {
    return float4(
        float((v >> 24) & 0xFF),
        float((v >> 16) & 0xFF),
        float((v >>  8) & 0xFF),
        float( v        & 0xFF)
    ) / 255.0;
}

// ── Atlas binding ──────────────────────────────────────────────────────
Texture2D<float4> atlas : register(t0);
SamplerState      samp  : register(s0);

// ── VS ─────────────────────────────────────────────────────────────────
VSOut vs_main(uint vid : SV_VertexID, VSInstance inst) {
    // The index buffer is `[0, 1, 2, 2, 1, 3]` — two triangles making
    // a quad. `SV_VertexID` is the VALUE from the index buffer (one of
    // 0, 1, 2, 3 — never higher), not the position-in-strip. Four
    // entries in the mapping, one per corner:
    //   0 = top-left (0, 0)
    //   1 = top-right (1, 0)
    //   2 = bottom-left (0, 1)
    //   3 = bottom-right (1, 1)
    const uint2 mapping[4] = {
        uint2(0, 0),
        uint2(1, 0),
        uint2(0, 1),
        uint2(1, 1),
    };
    uint2 corner = mapping[vid % 4];

    float2 px = (float2(inst.cellPos) + float2(corner)) * cellSize;

    VSOut o;
    o.pos = float4(px * invViewport + float2(-1.0,  1.0), 0.0, 1.0);
    // NDC y is up; our logical y is down. invViewport.y is negative to flip.

    float2 atlasTopLeft = float2(inst.atlasRect.xy);
    float2 atlasSize    = float2(inst.atlasRect.zw);
    // Pixel coords for the glyph rect, then normalize by the atlas
    // texture dims so the Sample() call gets UVs in [0,1].
    o.atlasUV = (atlasTopLeft + atlasSize * float2(corner)) * invAtlasSize;

    o.fg    = unpackRGBA(inst.fg);
    o.bg    = unpackRGBA(inst.bg);
    o.attrs = inst.attrs;
    return o;
}

// ── PS ─────────────────────────────────────────────────────────────────
float4 ps_main(VSOut i) : SV_Target {
    // atlasUV arrives already normalized (VS divides pixel coords by
    // invAtlasSize). Sample the grayscale coverage from the red channel —
    // Direct2D's DrawText with GRAYSCALE antialias writes coverage into
    // all RGB channels of the atlas; we pick R arbitrarily.
    float coverage = atlas.Sample(samp, i.atlasUV).r;

    // Inverse handling: swap fg/bg when attr bit 4 is set.
    float4 fg = i.fg;
    float4 bg = i.bg;
    if (i.attrs & 16u) {
        float4 tmp = fg;
        fg = bg;
        bg = tmp;
    }

    // Grayscale compositing: opaque background with fg painted on top
    // by coverage. One draw call, no separate bg pass.
    float3 rgb = lerp(bg.rgb, fg.rgb, coverage);
    return float4(rgb, 1.0);
}
