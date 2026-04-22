<p align="center">
  <img src="assets/Con-macOS-Dark-256x256@2x.png" width="120" alt="con logo">
</p>
<h1 align="center">con</h1>
<p align="center"><strong>The terminal emulator with AI harness, nothing more</strong></p>

<p align="center">
  Open source. GPU-accelerated. Terminal-first.
</p>
<p align="center">
  Built for SSH, tmux, and agent-native workflows.
</p>

<p align="center">
  <a href="https://opensource.org/licenses/MIT"><img alt="License: MIT" src="https://img.shields.io/badge/License-MIT-blue?style=flat"></a>
  <a href="https://github.com/nowledge-co/con-terminal/releases/latest"><img alt="Latest GitHub release" src="https://img.shields.io/github/v/release/nowledge-co/con-terminal?sort=semver&logo=github&style=flat"></a>
  <a href="https://developer.apple.com/macos/"><img alt="Native on macOS (Metal)" src="https://img.shields.io/badge/native-Metal-3D3D3D?logo=apple&logoColor=white&style=flat"></a>
  <a href="https://github.com/nowledge-co/con-terminal/issues/34"><img alt="Windows beta (tracker)" src="https://img.shields.io/badge/Windows-beta-0078D4?logo=windows&logoColor=white&style=flat"></a>
  <a href="https://github.com/nowledge-co/con-terminal/issues/18"><img alt="Linux support planned (tracker)" src="https://img.shields.io/badge/Linux-planned-1D4ED8?logo=linux&logoColor=white&style=flat"></a>
  <a href="https://www.rust-lang.org/"><img alt="Rust" src="https://img.shields.io/badge/Rust-CE422B?logo=rust&logoColor=white&style=flat"></a>
</p>

## Why con?

`con` is for people who want a serious terminal first and AI help only when it earns its place.

It keeps the PTY real, the shell visible, and the agent accountable.

If you're an old-school terminal user and only want enough AI harness when needed, nothing more or less, `con` is for you.

## What it does

- native terminal windows, tabs, and split panes
- a built-in AI harness that can read context, ask before acting, and work directly in the terminal you can already see
- terminal-native workflows for `ssh`, `tmux`, and coding-agent CLIs

## Status

`con` is in active beta development.

- **macOS** fully supported, beta.
- **Windows** early beta. Tracker: [#34](https://github.com/nowledge-co/con-terminal/issues/34).
- **Linux** planned. Tracker: [#18](https://github.com/nowledge-co/con-terminal/issues/18).

## Screenshot

<p align="center">
  <a href="docs/screenshots.md">
    <img width="1080" alt="con screenshot" src="https://github.com/user-attachments/assets/389898d6-56bf-46aa-9279-65e59a57ed23" />
  </a>
</p>



<p align="center">
  <a href="docs/screenshots.md">View the full screenshot gallery</a>
</p>

## Install

**macOS, Homebrew**

```sh
brew install --cask nowledge-co/tap/con-beta
```

**macOS**

```sh
curl -fsSL https://con-releases.nowledge.co/install.sh | sh
```

Or download the DMG directly from [Releases](https://github.com/nowledge-co/con-terminal/releases).

**Windows**

```powershell
irm https://con-releases.nowledge.co/install.ps1 | iex
```

Or download `con-windows-x86_64.zip` from the latest [Release](https://github.com/nowledge-co/con-terminal/releases).

To build from source, see `HACKING.md`.

## Docs

- `DESIGN.md` vision and architecture
- `HACKING.md` build and contributor quickstart
- `docs/screenshots.md` UI gallery
- `CHANGELOG.md` release notes and product changes

## License

[MIT](LICENSE)

## Credits ♥️

`con` depends on upstream projects we rely on directly and respect deeply:

- [Ghostty](https://github.com/ghostty-org/ghostty) for the terminal runtime and rendering foundation that powers our embedded terminal surfaces.
- [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) from the Zed team for the native GPU UI framework we build the shell on.
- [gpui-component](https://github.com/longbridge/gpui-component) from the Longbridge team for the component library that accelerates much of the UI layer.
- [Rig](https://github.com/0xPlaygrounds/rig) for the lovely Rust agent framework behind `con`'s AI harness.
- [Phosphor Icons](https://phosphoricons.com/) for the icon system used across the app.
- [Flexoki](https://stephango.com/flexoki) for the default visual theme direction.
- [Iosevka](https://typeof.net/Iosevka/) and [Ioskeley Mono](https://github.com/jewlexx/ioskeley) for the mono type foundation used in terminal chrome and code-heavy UI.

`con` was initially inspired by [warp.dev](http://warp.dev/), but is doing less than warp, if you need more, you should go for warp instead.