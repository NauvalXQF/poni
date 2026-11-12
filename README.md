# Poni

A native Windows notch/pill island. Built with Rust, tiny-skia, and the Win32 API.

## What works

- Two shapes: **Notch** (attached to the top edge) and **Pill** (floating just below it).
- Hover expands the island; move away to collapse it.
- Left-click pins/unpins the expanded state.
- Right-click toggles Notch/Pill.
- `Ctrl+Alt+N` toggles Notch/Pill.
- `Ctrl+Alt+Q` quits.
- Transparent, always-on-top, no-activate window with click-through outside the island.
- DPI-aware and crisp on scaled displays.
- Clock and native-feeling shell UI.

## Build

```powershell
cargo build --release
.\target\release\poni.exe
```

Rust GNU toolchain + MinGW is used on the development machine.

## Run behavior

The island starts at the top center of the primary monitor. It uses a transparent layered window and only the island's rounded path is interactive. The shape is persisted in:

```text
%APPDATA%\Poni\settings.json
```

## Current scope

This is the interaction shell, not the final product. Not built yet:

- System tray icon and settings window.
- Fullscreen auto-hide.
- Media controls (SMTC), notifications, clipboard, shelf, timers.
- Per-monitor profiles and notch calibration.
- Auto-start and signed installer/updater.

The shell uses a fixed maximum-size window and draws the island inside it, so expanding does not resize the native window.
