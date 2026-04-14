# 2026-03-12: GPUI-CE API patterns learned during scaffold

## What happened
Building the first working con binary required iterating on GPUI-CE API usage
patterns that differ from documentation/examples.

## Key findings

### 1. `FluentBuilder::when()` is NOT re-exported
The `when()`, `when_some()` methods exist on `FluentBuilder` trait in GPUI's
internal `util.rs`, but are NOT re-exported in the public `gpui::*` prelude.
**Fix:** Use `if/else` to conditionally build elements instead.

### 2. `cx.spawn()` uses `AsyncFnOnce` with `async move` closures
The correct pattern is:
```rust
cx.spawn(async move |this, cx| {
    // this: WeakEntity<Self>
    // cx: &mut AsyncApp (borrowed, not owned)
    this.update(cx, |this, cx| cx.notify()).ok();
})
.detach();
```
NOT `|this, mut cx| async move { ... }` — the async closure syntax is
different from a closure-returning-future.

### 3. Metal shader compilation needs Xcode.app
GPUI-CE compiles Metal shaders at build time via `xcrun metal`. This requires
full Xcode.app, not just Command Line Tools.
**Fix:** Enable `runtime_shaders` feature to skip precompilation.

### 4. vte 0.15 changed `advance()` signature
`parser.advance(&mut performer, &[u8])` takes a byte slice, not individual bytes.

## What we learned
- Always check the actual GPUI source for exported symbols, not just examples
- The `async move |args|` closure syntax is GPUI's convention via edition 2024's AsyncFnOnce
- Runtime shaders are fine for dev; precompiled shaders for release builds need Xcode
