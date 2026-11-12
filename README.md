# Poni

A native Windows capsule HUD. Built with Rust, tiny-skia, and the Win32 API.

## What works

- One floating capsule that expands into a full dashboard.
- Idle HUD: clock and a proportional battery indicator with a green charging state.
- Expanded view: large clock, date, battery, Wi-Fi SSID, and Bluetooth device.
- Transient notices for Wi-Fi connection + SSID, Bluetooth device connection with
  battery level, and airplane mode changes.
- Hover expands the capsule; move away to collapse it.
- Left-click or `Ctrl+Alt+N` pins/unpins the expanded state.
- Right-click opens the capsule menu: pin, size (75–130%), Start with Windows, and Quit.
- `Ctrl+Alt+Q` quits.
- Transparent, always-on-top, no-activate window with click-through outside the capsule.
- Hides automatically for fullscreen apps and returns when fullscreen ends.
- DPI-aware and crisp on scaled displays.
- Wi-Fi, Bluetooth, and airplane polling runs on a background worker every three
  seconds, so network APIs never stall the animation loop.
- Battery uses native `GetSystemPowerStatus`; status refreshes once per second and
  only redraws when a value changes.

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

The capsule starts at the top center of the primary monitor. It uses a
transparent layered window and only the capsule's rounded path is interactive.
The size scale is persisted in:

```text
%APPDATA%\Poni\settings.json
```

## Current scope

This is the interaction shell, not the final product. Not built yet:

- A dedicated settings window (settings currently live in the right-click menu).
- Media controls (SMTC), system notifications, clipboard, shelf, timers.
- Per-monitor profiles and notch calibration.
- Signed installer/updater.

The shell uses a fixed maximum-size window and draws the capsule inside it, so
expanding does not resize the native window.
