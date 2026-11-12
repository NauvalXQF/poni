# Poni

A native Windows notch/pill island. Built with Rust, tiny-skia, and the Win32 API.

## What works

- Two shapes: **Notch** (attached to the top edge) and **Pill** (floating just below it).
- Idle HUD: clock, CPU usage, and a white battery indicator with green charging bolt.
- Transient notices for Wi-Fi connection + SSID, Bluetooth device connection, and airplane mode changes.
- Hover expands the island; move away to collapse it.
- Left-click pins/unpins the expanded state.
- Right-click toggles Notch/Pill.
- Tray menu: shape, size (75–130%), Start with Windows, and Quit.
- `Ctrl+Alt+N` toggles Notch/Pill.
- `Ctrl+Alt+Q` quits.
- Transparent, always-on-top, no-activate window with click-through outside the island.
- Hides automatically for fullscreen apps and returns when fullscreen ends.
- DPI-aware and crisp on scaled displays.
- Adaptive frame pacing: detects the monitor refresh rate and animates at
  120Hz or 60Hz without changing animation duration.
- Idle HUD uses native `GetSystemTimes` and `GetSystemPowerStatus`; status
  refreshes once per second and only redraws when a value changes.

Animations are time-based, so the same spring feels identical on a 60Hz and
120Hz panel. Idle behavior drops to a low-frequency cursor poll instead of
running the render loop continuously. For testing, override the detected rate:

```powershell
$env:PONI_REFRESH_HZ = "60"
.\target\release\poni.exe
```

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

- A dedicated settings window (settings currently live in the tray menu).
- Fullscreen auto-hide.
- Media controls (SMTC), notifications, clipboard, shelf, timers.
- Per-monitor profiles and notch calibration.
- Signed installer/updater.

The shell uses a fixed maximum-size window and draws the island inside it, so expanding does not resize the native window.
