# dimtune

[![CI](https://github.com/Abemilek/dimtune/actions/workflows/ci.yml/badge.svg)](https://github.com/Abemilek/dimtune/actions/workflows/ci.yml)

An ultra-minimal music player written in Rust. Fixed dark theme, no animations,
no network access, and no telemetry. It plays a local music folder, reads tags and
embedded cover art, and stays out of the way.

Tested on **Debian 13 (Trixie) + GNOME + Wayland**, but it is **not** tied to that
environment. It runs on other Linux distributions and desktop environments as long
as the requirements below are met.

## Features

- Plays `mp3`, `flac`, `ogg` (Vorbis), `wav`, and `m4a` (AAC).
- Recursively scans a music folder, up to 12 levels deep.
- Reads title, artist, album, duration, and embedded cover art.
- Falls back to `cover`, `folder`, `front`, or `album` images (`.jpg`, `.jpeg`,
  `.png`) in the track's folder, case-insensitively.
- Shuffle mode, automatic next track, seek bar, volume control, and keyboard
  shortcuts.
- Remembers the last folder, shuffle state, and volume.
- Single small native binary. No web view, no runtime services, no network.
- Per-user install; no `sudo` required to install the app itself.

The user interface is currently in Spanish.

## Platform support

| Environment | Status |
| --- | --- |
| Debian 13 + GNOME + Wayland | Tested |
| Other Debian-based distributions with GNOME | Expected to work |
| KDE Plasma, Xfce, Cinnamon, MATE, etc. | Expected to work |
| X11 or Wayland sessions | Supported |
| Fedora, Arch Linux, openSUSE, etc. | Expected to work |
| Windows / macOS | Not supported |

The app is a normal Linux desktop application: it uses ALSA for audio output and
the XDG Desktop Portal for the native folder picker. GNOME is not a requirement.
Any desktop environment with a working portal backend can be used.

### Binary compatibility

The compiled binary is linked against the glibc of the machine that builds it (and
against the audio stack available there). A binary built on Debian 13 may not run
on an older release such as Debian 12 because of the glibc version. For the widest
compatibility, **build the app on the distribution where you will run it**, or
build inside a container for the oldest target you want to support.

The prebuilt release is built on Ubuntu 22.04 (glibc 2.35), so it should run on
Debian 12/13, Ubuntu 22.04 or newer, Linux Mint 21 or newer, and Pop!_OS 22.04 or
newer. It is **x86_64 only**.

## Requirements

### Build requirements

- Linux with X11 or Wayland.
- Rust **1.89 or newer** (the project uses edition 2024 and recent language
  features). Debian 13's default `rustc` is older; use
  [rustup](https://rustup.rs) or your distribution's updated toolchain.
- A C toolchain, `pkg-config`, and the ALSA development headers.

Install the build dependencies for your distribution:

```sh
# Debian / Ubuntu / Linux Mint / Pop!_OS
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install gcc make pkgconf-pkg-config alsa-lib-devel

# Arch Linux / Manjaro
sudo pacman -S --needed base-devel pkgconf alsa-lib

# openSUSE
sudo zypper install gcc make pkg-config alsa-devel
```

Install Rust with rustup if your distribution's version is older than 1.89:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version   # must be 1.89 or newer
```

### Runtime requirements

- An audio server supported through ALSA: PipeWire (default on modern GNOME and
  KDE), PulseAudio, or direct ALSA. On Debian/Ubuntu with PipeWire, install
  `pipewire-alsa` so ALSA applications are routed through PipeWire.
- `xdg-desktop-portal` plus the backend for your desktop, used by the
  "Carpeta…" (Folder…) button. GNOME uses `xdg-desktop-portal-gnome`; KDE uses
  `xdg-desktop-portal-kde`; GTK-based desktops can use `xdg-desktop-portal-gtk`.

On Debian 13 / Ubuntu / Linux Mint:

```sh
sudo apt install pipewire pipewire-alsa xdg-desktop-portal xdg-desktop-portal-gnome
```

On Fedora:

```sh
sudo dnf install pipewire pipewire-alsa xdg-desktop-portal xdg-desktop-portal-gnome
```

On Arch Linux:

```sh
sudo pacman -S --needed pipewire pipewire-alsa xdg-desktop-portal xdg-desktop-portal-gnome
```

If the portal is missing, playback and the default music folder still work; only
the native folder chooser will not respond.

## Prebuilt release

The [Releases](https://github.com/Abemilek/dimtune/releases) page contains a
prebuilt x86_64 Linux tarball produced by GitHub Actions. Set `VERSION` to the
release you downloaded:

```sh
VERSION=v0.1.0
PKG="dimtune-${VERSION}-x86_64-linux"

tar -xzf "${PKG}.tar.gz"

install -Dm755 "${PKG}/dimtune" "$HOME/.local/bin/dimtune"
install -Dm644 "${PKG}/dimtune.desktop" \
  "$HOME/.local/share/applications/dimtune.desktop"
install -Dm644 "${PKG}/dimtune.svg" \
  "$HOME/.local/share/icons/hicolor/scalable/apps/dimtune.svg"

update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
```

Release archives include the binary, desktop entry, icon, README, and license. A
`.sha256` file is published next to each archive.

## Build and install

```sh
git clone https://github.com/Abemilek/dimtune.git
cd dimtune
./instalar.sh
```

The install script builds the release binary and installs only for the current
user:

- `~/.local/bin/dimtune`
- `~/.local/share/applications/dimtune.desktop`
- `~/.local/share/icons/hicolor/scalable/apps/dimtune.svg`

Then launch it from the application menu or with:

```sh
dimtune
```

If `~/.local/bin` is not in your `PATH`:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

### Build without installing

```sh
cargo build --release
./target/release/dimtune
```

For a quick development run:

```sh
cargo run --release
```

### Tests

```sh
cargo test
```

The test suite covers duration formatting, file-name handling, shuffle
permutation correctness, config parsing, recursive scanning, and case-insensitive
folder covers.

## Usage

- On first launch, dimtune uses the XDG music folder (for example `~/Music`, or
  `~/Música` on a Spanish Debian system) and scans it recursively. On later
  launches it reopens the last folder you chose.
- **Carpeta…** (Folder…) selects another folder. Subfolders are included.
- Click a track in the list to play it. When a track ends, the next one starts
  automatically.
- **Aleatorio** (Shuffle) shuffles the playback order; turn it off for
  alphabetical order. At the end of the list, playback wraps around.
- **Anterior** (Previous) restarts the current track if it has played for more
  than 4 seconds; otherwise it goes to the previous track.
- **Vol** (Volume) is applied live and saved when you release the slider.

Keyboard shortcuts:

| Key | Action |
| --- | --- |
| `Space` | Play / pause |
| `→` | Next track |
| `←` | Previous track / restart current track |

### Formats and cover art

- Supported containers/codecs: MP3, FLAC, Ogg Vorbis, WAV, M4A with AAC audio.
- Ogg Opus and ALAC are not supported by the current decoder configuration.
- Cover art is read from the file's tags first. If none is present, dimtune looks
  for `cover`, `folder`, `front`, or `album` with a `.jpg`, `.jpeg`, or `.png`
  extension in the same folder.

## Configuration

dimtune only stores three values in a plain-text file:

```
$XDG_CONFIG_HOME/dimtune/config
# usually ~/.config/dimtune/config
```

```ini
carpeta=/home/user/Music
aleatorio=0
volumen=0.80
```

No cache, history, play counts, or telemetry are written anywhere else.

## Uninstall

```sh
./desinstalar.sh
```

This removes the binary, desktop entry, icon, and `~/.config/dimtune`.

## Troubleshooting

### The build fails on ALSA headers

Install `libasound2-dev` (Debian/Ubuntu), `alsa-lib-devel` (Fedora/openSUSE), or
`alsa-lib` (Arch), then run the build again.

### There is no sound

Check which audio server is running:

```sh
wpctl status     # PipeWire
pactl info       # PulseAudio or PipeWire's PulseAudio compatibility layer
```

Make sure the ALSA bridge is installed (`pipewire-alsa` or `pulseaudio-alsa`) and
that the output device is not muted. A quick ALSA test is:

```sh
speaker-test -c 2 -t sine -l 1   # package: alsa-utils
```

### The folder picker does nothing

The native chooser needs a running D-Bus session and an XDG Desktop Portal
backend. Install the portal package for your desktop, then log out and back in.
On minimal window-manager setups, `xdg-desktop-portal-gtk` is usually enough.

### The window does not open (Wayland / OpenGL)

dimtune uses OpenGL through `glow`. On virtual machines, remote desktops, or
systems without working GPU drivers, try software rendering:

```sh
LIBGL_ALWAYS_SOFTWARE=1 dimtune
```

If both Wayland and X11 are available, you can force an X11 session with:

```sh
WAYLAND_DISPLAY= dimtune
```

### "GLIBC_2.xx not found" on another distribution

The binary was built for a newer glibc than the target system. Build dimtune on
that system (or in a matching container) instead of copying the binary.

## How it works

`src/main.rs` is intentionally a small, single-file application:

- **UI:** `eframe`/`egui` with the `glow` OpenGL renderer, built for both Wayland
  and X11. The theme is a fixed dark palette with animations disabled.
- **Audio:** `rodio` with Symphonia decoders; output goes through cpal/ALSA to
  PipeWire, PulseAudio, or ALSA directly. Tracks are streamed from disk.
- **Metadata:** `lofty` reads tags and duration; `image` decodes embedded or
  sidecar covers.
- **Folder chooser:** `rfd` with the XDG Desktop Portal backend.
- **Concurrency:** folder scanning and metadata loading run on background threads
  and report back over `std::sync::mpsc` channels, keeping the UI responsive.
- **Shuffle:** a tiny xorshift64 generator, so no random-number crate is needed.
- **Config:** a hand-written key/value parser with atomic writes (temporary file
  plus rename).

Project layout:

```
Cargo.toml
src/main.rs
instalar.sh          # build + per-user install
desinstalar.sh       # per-user uninstall
dimtune.desktop      # desktop entry
dimtune.svg          # application icon
.github/workflows/   # CI and tagged release automation
```

## License

MIT. See [LICENSE](LICENSE).
