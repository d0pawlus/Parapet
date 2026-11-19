<p align="center">
  <img src="assets/logo-256.png" width="160" alt="Parapet logo: a pixel-art window with a sill, showing sun, rain, snow and a storm">
</p>

# Parapet

A cozy, transparent, click-through overlay for the whole desktop on Windows, macOS and Linux: autumn leaves, rain,
snow and storms that react to your windows, your mouse cursor and the real weather outside.

*Parapet* is Polish for windowsill.

<p align="center">
  <a href="https://github.com/d0pawlus/Parapet/releases/latest/download/Parapet.exe">
    <img src="https://img.shields.io/badge/Download-Parapet.exe-2ea44f?style=for-the-badge&logo=windows&logoColor=white" alt="Download Parapet.exe">
  </a>
  <a href="https://github.com/d0pawlus/Parapet/releases/latest/download/Parapet-macOS-arm64.zip">
    <img src="https://img.shields.io/badge/Download-macOS%20(Apple%20silicon)-2ea44f?style=for-the-badge&logo=apple&logoColor=white" alt="Download Parapet for macOS">
  </a>
  <a href="https://github.com/d0pawlus/Parapet/releases/latest/download/Parapet-x86_64.AppImage">
    <img src="https://img.shields.io/badge/Download-Linux%20AppImage-2ea44f?style=for-the-badge&logo=linux&logoColor=white" alt="Download Parapet for Linux">
  </a>
  <br>
  <a href="https://github.com/d0pawlus/Parapet/releases/latest">
    <img src="https://img.shields.io/github/v/release/d0pawlus/Parapet?label=latest%20release" alt="Latest release">
  </a>
  <br>
  <sub>Windows 10/11 x64 · macOS 11+ on Apple silicon · Linux x86_64 (X11) · no installer</sub>
</p>

## Features

- **Leaves**: 28 species (maples, oaks, beech, birch, linden, ginkgo, chestnut, hawthorn…) in warm, flat
  colours. Every leaf falls in its own way: it swings like a pendulum, tumbles or spirals down, switches
  style now and then and catches its own gusts.
- **Windows are roofs**: leaves and snow settle on the top edges of windows and ride along when a window
  moves. Jerk a window to shake them off. Rain runs to the corners, where a drop gathers and falls like
  from a gutter, and some water trickles down the window's sides.
- **The taskbar is the ground** (on macOS: the Dock, or the bottom of the screen): leaves, snowdrifts and, after rain, mud and puddles with ripples and
  rings from the drops. It works with maximized windows too.
- **The mouse is the wind**: moving the cursor sweeps up falling leaves and flakes, lifts leaves off the
  ground, blows snow off the drifts and makes waves and splashes of water or mud in puddles.
- **Rare visitors** show up on their own every few minutes, depending on the weather (tray menu → Rare
  events, where you can also summon them by hand): a hedgehog spiking leaves on its quills (it curls into a
  ball near the cursor), a caterpillar eating fallen leaves, an arctic fox diving into snowdrifts,
  snowdrops when it gets warmer, a snail leaving a slime trail after rain, a frog hopping between puddles,
  a robin perching on windows, a squirrel with an acorn and mushrooms sprouting from the mud.

## Weather

**Automatic** mode fetches the weather from Open-Meteo every 15 minutes (no API key needed): snow when
it precipitates at ≤ 0.5 °C, rain for rain or drizzle (with leaves in autumn), lightning during storms and
leaves when it is dry. Wind direction and strength carry the particles. The location comes from your IP
address, or from `City=` or `Latitude=`/`Longitude=` in `settings.ini` next to the exe (tray menu →
Settings). Changes to the file are picked up automatically.

## Quality and performance

Tray menu → Performance: 20/30/60 fps and **low** (½ resolution), **medium** (¾), **high** (full) or
**ultra** (full + leaf shadows) quality.

- Written in Rust: one ~6 MB executable with no runtime to install. The same simulation runs everywhere;
  only window tracking, power and the overlay windows are platform code.
- Drawn on the GPU with wgpu: DirectX 12 through DirectComposition on Windows, Metal on macOS, one transparent
  window per monitor. Leaves, drops, flakes and the snow, mud and water layers are sprites from a single
  atlas, the visitors are antialiased shapes, all in one draw call. Leaves are painted at 4 sizes (mipmaps) so
  they don't shimmer.
- The animation runs on its own thread, so opening the tray menu doesn't stop it.
- On Windows, windows are tracked through system events rather than constant polling; on macOS the window
  list is read from the window server each frame. The cursor is read once per frame, with no global mouse hook.
- Pauses during full-screen games and videos, on the lock screen and while the monitor is off. On battery
  it caps at 20 fps (and uses EcoQoS on Windows).

Measured on Windows 11 with a 4K display plus a 1080p monitor (RTX 5070):

| Scenario | CPU (one core) | RAM (working set) |
|---|---|---|
| Storm, extreme intensity, ultra, 30 fps, every visitor out | 6.9% | 112 MB |

Most of the memory is the DirectX 12 driver; the old Direct2D build used about 20 MB.

## Download

Ready-to-run builds are on the [Releases](https://github.com/d0pawlus/Parapet/releases) page.

- **Windows 10/11 (x64)**: `Parapet.exe`. Just run it; settings are saved to `settings.ini` next to the exe.
  The exe isn't code-signed yet, so Windows SmartScreen may warn you: click "More info" → "Run anyway".
- **macOS 11+ (Apple silicon)**: `Parapet-macOS-arm64.zip`. Unzip it and move `Parapet.app` to Applications.
  It lives in the menu bar, with no Dock icon. Settings are in
  `~/Library/Application Support/Parapet/settings.ini`. The app isn't notarized yet, so the first time
  right-click it → Open (on macOS 15: System Settings → Privacy & Security → "Open Anyway"), or run
  `xattr -dr com.apple.quarantine /Applications/Parapet.app`.

- **Linux (x86_64)**: `Parapet-x86_64.AppImage`. Make it executable (`chmod +x`) and run it; settings are in
  `~/.config/parapet/settings.ini`. It needs what desktop distributions usually have: a compositing window
  manager, GTK 3 with an AppIndicator library, `libxkbcommon-x11`, a Vulkan driver and OpenSSL 3. On X11
  everything works as on Windows. Under Wayland it runs through XWayland and only falls: Wayland doesn't let
  apps see other windows or the pointer. GNOME shows the tray icon only with the AppIndicator extension.

## Building

With [Rust](https://rustup.rs) installed:

- `cargo build --release` produces `target/release/parapet(.exe)` (on Windows with the MSVC toolchain).
- `sh scripts/bundle-macos.sh` (on a Mac) builds `dist/Parapet.app` and `dist/Parapet-macOS-arm64.zip`.
- `sh scripts/bundle-linux.sh` (on Linux, with `libgtk-3-dev`, `libayatana-appindicator3-dev` and `libssl-dev`)
  builds `dist/Parapet-x86_64.AppImage`.
- `cargo build --release --features debug-overlay` builds a diagnostic version that writes a log next to
  `settings.ini`, dumps the leaf atlas and summons every visitor at startup.
- `cargo test` runs the simulation, drawing and settings tests.

GitHub Actions builds both platforms on every push and attaches them to a release for `v*` tags.

The logo is generated by `assets/make_logo.py` (Python + Pillow).

## License

Copyright (C) 2026 Dominik Pawluś

Parapet is free software, licensed under the [GNU GPL v3](LICENSE).
