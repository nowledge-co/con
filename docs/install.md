# Install con

con is in beta. macOS is the primary supported platform today. Windows and Linux
builds are available as previews.

## macOS with Homebrew

```sh
brew install --cask nowledge-co/tap/con-beta
```

This installs con and adds `con-cli` to your PATH for automation.

## macOS direct install

```sh
curl -fsSL https://con-releases.nowledge.co/install.sh | sh
```

This installs con into `/Applications` and links `con-cli` into `~/.local/bin`.
You can also download the DMG from
[Releases](https://github.com/nowledge-co/con-terminal/releases).

## Windows preview

```powershell
irm https://con-releases.nowledge.co/install.ps1 | iex
```

This installs `con-app.exe` and `con-cli.exe` into the same PATH directory.

Scoop users can install the community package:

```powershell
scoop bucket add jam https://github.com/EFLKumo/jam
scoop install jam/con-terminal
```

Windows is still early. Follow the
[Windows tracker](https://github.com/nowledge-co/con-terminal/issues/34) for
current limits and fixes.

## Linux preview

```sh
curl -fsSL https://con-releases.nowledge.co/install.sh | sh
```

This installs `con` and `con-cli` into `~/.local/bin`. You can also download the
Linux tarball from the latest
[Release](https://github.com/nowledge-co/con-terminal/releases).

Linux is in preview. Follow the
[Linux tracker](https://github.com/nowledge-co/con-terminal/issues/18) for
current limits and fixes.

## Build from source

If you want to build or change con itself, use the
[contributor quickstart](https://github.com/nowledge-co/con-terminal/blob/main/HACKING.md)
in the source repo.
