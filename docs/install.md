# Install con

con is in beta. macOS is the primary supported platform today. Windows and Linux
builds are available as previews.

Every installer includes the app and `con-cli`. The CLI is used by scripts,
test runners, and external agent orchestrators to talk to a running con session.

Use the official installer when you want the newest beta as soon as it ships.
Community packages are convenient if they already fit your system workflow, but
they may trail the GitHub release for a short time.

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

### Official installer

```powershell
irm https://con-releases.nowledge.co/install.ps1 | iex
```

This installs `con-app.exe` and `con-cli.exe` into the same PATH directory.

### Scoop

Scoop users can install Con from the community-maintained `jam` bucket:

```powershell
scoop bucket add jam https://github.com/EFLKumo/jam
scoop install jam/con-terminal
```

The Scoop manifest is maintained by
[`EFLKumo`](https://github.com/EFLKumo) in
[`EFLKumo/jam`](https://github.com/EFLKumo/jam). It installs Con as a portable
app and adds both `con-app` and `con-cli` to your PATH.

Windows is still early. Follow the
[Windows tracker](https://github.com/nowledge-co/con-terminal/issues/34) for
current limits and fixes.

## Linux preview

### Official installer

```sh
curl -fsSL https://con-releases.nowledge.co/install.sh | sh
```

This installs `con` and `con-cli` into `~/.local/bin`, registers the desktop
launcher, and refreshes the app icon. You can also download the Linux tarball
from the latest
[Release](https://github.com/nowledge-co/con-terminal/releases).

### Arch Linux / AUR

Arch users can install the community AUR package:

```sh
yay -S con-bin
```

`paru -S con-bin` works as well. The AUR package is maintained by
[`czyt`](https://aur.archlinux.org/account/czyt), and the package page is
[`con-bin`](https://aur.archlinux.org/packages/con-bin).

Linux is in preview. Follow the
[Linux tracker](https://github.com/nowledge-co/con-terminal/issues/18) for
current limits and fixes.

## Build from source

If you want to build or change con itself, use the
[contributor quickstart](https://github.com/nowledge-co/con-terminal/blob/main/HACKING.md)
in the source repo.
