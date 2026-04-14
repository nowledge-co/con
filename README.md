<p align="center">
  <img src="assets/Con-macOS-Dark-256x256@2x.png" width="120" alt="con logo">
</p>
<h1 align="center">con</h1>
<p align="center"><strong>The terminal emulator with AI harness, nothing more</strong></p>

<p align="center">
  Open source. Fast. Terminal-first.
</p>
<p align="center">
  Built for SSH, tmux, and agent-native workflows.
</p>




<p align="center">
  <a href="https://opensource.org/licenses/MIT">
    <img alt="License: MIT" src="https://img.shields.io/badge/License-MIT-blue.svg">
  </a>
  <a href="https://github.com/nowledge-co/con/releases">
    <img alt="Releases" src="https://img.shields.io/badge/Releases-download-black">
  </a>
</p>

## Why con?

If you're an old-school terminal user and only want enough AI harness when needed, nothing more or less, `con` is for you.

Think of it like a beloved old beast with modern Bluetooth done right, or a truck lover who still wants an electric future.

## What it does

- a terminal emulator
- built-in AI harness aware of panes within a tab
- works well with `ssh`, `tmux`, and agent CLIs running directly in the terminal

## Status

`con` is in active pre-release development.

Best-supported target today: **macOS**.

## Screenshot

<p align="center">
  <a href="docs/screenshots.md">
    <img width="1080" alt="con screenshot" src="https://github.com/user-attachments/assets/e1972fac-9df3-443b-be08-c0eec4697cf3" />
  </a>
</p>

<p align="center">
  <a href="docs/screenshots.md">View the full screenshot gallery</a>
</p>

## Download

Grab the latest binary from [Releases](https://github.com/nowledge-co/con/releases).

If you want to build from source or hack on `con`, see `HACKING.md`.

## Docs

- `DESIGN.md` vision and architecture
- `HACKING.md` build and contributor quickstart
- `docs/screenshots.md` UI gallery

## License

[MIT](LICENSE)

## Credits ♥️

`con` stands on the work of several upstream projects we rely on directly and respect deeply:

- [Ghostty](https://github.com/ghostty-org/ghostty) for the terminal runtime and rendering foundation that powers our embedded terminal surfaces.
- [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) from the Zed team for the native GPU UI framework we build the shell on.
- [gpui-component](https://github.com/longbridge/gpui-component) from the Longbridge team for the component library that accelerates much of the UI layer.
- [Rig](https://github.com/0xPlaygrounds/rig) for the Rust AI agent framework behind Con's agent harness, along with the provider work we maintain in our own fork(before our upstream first work merged).
- [Phosphor Icons](https://phosphoricons.com/) for the icon system used across the app.
- [Flexoki](https://stephango.com/flexoki) for the default visual theme direction.
- [Iosevka](https://typeof.net/Iosevka/) and [Ioskeley Mono](https://github.com/jewlexx/ioskeley) for the mono type foundation used in terminal chrome and code-heavy UI.

If you build or use `con`, you are also benefiting from the care and engineering work of those communities.



`con` was initially inspired of [warp.dev](http://warp.dev/), but is doing less than warp, if you need more, you proababbly should go for warp instead.
