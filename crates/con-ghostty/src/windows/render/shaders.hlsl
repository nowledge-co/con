// Con Windows terminal renderer — HLSL.
//
// Single DrawIndexedInstanced(6, cells) per frame. Each cell is a quad
// with per-instance inputs (IA layout, matching `VS_INSTANCE` in
// pipeline.rs). VS computes the cell's screen-space position from its
// grid coords; PS samples the ClearType-rendered glyph atlas (BGRA8,
// per-subpixel coverage in R,G,B) and lerps fg→bg per channel for a
// subpixel-accurate composite.
//
// Atlas contents: Direct2D draws a white brush through a ClearType-
// enabled render target onto an opaque-black background. Result per
// channel = subpixel coverage. The PS lerps (bg[c], fg[c], coverage[c])
// per channel c in {R,G,B}.

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
    // (x, y) of this cell's glyph in atlas pixels.
    uint2  atlasPos      : ATLAS_POS;
    // (w, h) of this cell's glyph in atlas pixels.
    uint2  atlasSize     : ATLAS_SIZE;
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
    // Cell-local UV, (0,0)=top-left .. (1,1)=bottom-right. Used by the
    // PS to draw underline / strikethrough bands without needing to
    // know the viewport.
    float2 cellUV        : TEXCOORD1;
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

    // Wide-glyph handling: Nerd-Font PUA icons are authored with
    // advance=1 cell but ink wider than that. The atlas allocates them
    // in slots up to 2 cells wide; honoring `atlasSize.x` as the quad
    // width lets those icons render at their natural size on screen.
    // Empty cells (codepoint=0 or space) set atlasSize=(0,0), so the
    // max keeps their quad exactly cellSize — no visual change for the
    // common case.
    float2 quadSize = float2(
        max(cellSize.x, float(inst.atlasSize.x)),
        cellSize.y
    );
    float2 px = float2(inst.cellPos) * cellSize + float2(corner) * quadSize;

    VSOut o;
    o.pos = float4(px * invViewport + float2(-1.0,  1.0), 0.0, 1.0);
    // NDC y is up; our logical y is down. invViewport.y is negative to flip.

    float2 atlasTopLeft = float2(inst.atlasPos);
    float2 atlasPixels  = float2(inst.atlasSize);
    // Pixel coords for the glyph rect, then normalize by the atlas
    // texture dims so the Sample() call gets UVs in [0,1].
    o.atlasUV = (atlasTopLeft + atlasPixels * float2(corner)) * invAtlasSize;
    o.cellUV  = float2(corner);

    o.fg    = unpackRGBA(inst.fg);
    o.bg    = unpackRGBA(inst.bg);
    o.attrs = inst.attrs;
    return o;
}

// ── PS ─────────────────────────────────────────────────────────────────
float4 ps_main(VSOut i) : SV_Target {
    // atlasUV arrives already normalized (VS divides pixel coords by
    // invAtlasSize). ClearType wrote per-subpixel coverage to R/G/B;
    // sample all three channels for subpixel antialiasing.
    float3 coverage = atlas.Sample(samp, i.atlasUV).rgb;

    // Inverse handling: swap fg/bg when attr bit 4 is set.
    float4 fg = i.fg;
    float4 bg = i.bg;
    if (i.attrs & 16u) {
        float4 tmp = fg;
        fg = bg;
        bg = tmp;
    }

    // Underline / strikethrough: draw a 1-pixel-tall fg band inside
    // the cell. Bands are in cell-local UV space:
    //   underline: bottom ~10% of the cell (UV.y in [0.90, 0.97])
    //   strike:    vertical middle (UV.y in [0.48, 0.55])
    // The cellSize in px tells us how wide a pixel is in UV, which
    // lets us build a crisp 1-px band without AA fringing.
    float pxUV = 1.0 / max(cellSize.y, 1.0);
    float band_coverage = 0.0;
    if (i.attrs & 4u) {
        // Underline.
        float center = 0.92;
        if (abs(i.cellUV.y - center) < pxUV) {
            band_coverage = 1.0;
        }
    }
    if (i.attrs & 8u) {
        // Strikethrough.
        float center = 0.52;
        if (abs(i.cellUV.y - center) < pxUV) {
            band_coverage = 1.0;
        }
    }

    // Per-channel lerp: subpixel ClearType for glyph coverage, mixed
    // with a flat `band_coverage` for decorations so underline /
    // strike bands don't show subpixel fringing.
    float3 comp = max(coverage, float3(band_coverage, band_coverage, band_coverage));
    float3 rgb;
    rgb.r = lerp(bg.r, fg.r, comp.r);
    rgb.g = lerp(bg.g, fg.g, comp.g);
    rgb.b = lerp(bg.b, fg.b, comp.b);
    return float4(rgb, 1.0);
}
