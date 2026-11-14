# ZenithBar

A lightweight, modern hardware diagnostics and telemetry sidebar for Windows 10 and Windows 11 written in Rust.

ZenithBar is inspired by SidebarDiagnostics, redesigned from the ground up for minimal resource utilization, native Windows Shell integration, and a modern Fluent / Acrylic interface.

---

## Key Highlights

- Minimal Memory Footprint: Consumes approximately 15 MB to 25 MB of RAM under active monitoring (compared to 100 MB - 200 MB+ typical for managed runtime implementations).
- Negligible CPU Utilization: Less than 0.1% CPU usage when idle, powered by retained-mode rendering and decoupled polling timers.
- Native Windows Desktop Integration: Full Windows Shell AppBar (`SHAppBarMessage`) support that docks to the left or right screen edge, reserving the desktop work area so maximized windows do not overlap the sidebar.
- Fluent Dark Glass Aesthetics: Desktop Window Manager (DWM) Acrylic blur and dark mode system styling.
- Multi-Monitor Support: Dynamic display selection and resolution adaptation via `EnumDisplayMonitors` and `GetMonitorInfoW`.
- Hardware Telemetry: Multi-core CPU utilization, package clock, system memory distribution, GPU monitoring, per-disk read/write throughput, network interface throughput, and external IP lookup.
- Dedicated Settings Interface: Dedicated five-tab settings window providing 1:1 functional parity with original configuration models (General, Advanced, Customize, Hardware, Hotkeys).
- System Tray & Global Shortcuts: Full system tray integration with right-click context menu and global hotkeys (`RegisterHotKey`) for immediate visibility toggle (`Ctrl+Shift+S`).
- Zero Runtime Dependencies: Compiled to a single native executable with no dependency on the .NET Runtime or Visual C++ Redistributable packages.

---

## Architecture Overview

ZenithBar uses a dual-window, decoupled multi-threaded architecture:

1. Main Process (STA):
   The primary thread operates as a Single-Threaded Apartment (STA), hosting the Slint UI event loop, the native Windows message dispatch pump, the AppBar message receiver, and the system tray notification icon.

2. Telemetry Engine (MTA):
   Hardware sampling runs asynchronously in an isolated background thread. Metrics are gathered at configurable intervals (default: 1000 ms) and dispatched to the UI thread via thread-safe channels (`std::sync::mpsc`).

3. Window Hierarchy & Shell Registration:
   The sidebar window is configured with `WS_EX_TOOLWINDOW` and explicitly stripped of `WS_EX_APPWINDOW`, ensuring it does not occupy space in the Windows Taskbar or Alt+Tab switcher during normal desktop monitoring. The Settings window is managed as a standard modal dialog that displays in the taskbar only while open.

---

## Prerequisites

- Windows 10 (version 1809 or higher) or Windows 11 (x86_64).
- Rust 1.80 or newer with the `x86_64-pc-windows-gnu` or `x86_64-pc-windows-msvc` target toolchain.

---

## Building from Source

### Native Windows Build (MSVC)

```bash
cargo build --release
```

The optimized binary is generated at:
`target/release/ZenithBar.exe`

### Cross-Compilation from Linux (MinGW GNU)

```bash
cargo build --release --target x86_64-pc-windows-gnu
```

The binary will be located at:
`target/x86_64-pc-windows-gnu/release/ZenithBar.exe`

### Binary Size Optimizations

ZenithBar is pre-configured with whole-program Link Time Optimization (`lto = "fat"`), single codegen unit (`codegen-units = 1`), compact instruction density (`opt-level = "z"`), and automatic symbol stripping (`strip = true`), resulting in a release binary under 9 MB.

---

## Configuration

ZenithBar loads and persists preferences in standard JSON format:

- Portable Mode: If a `Settings.json` file is present in the same directory as `ZenithBar.exe`, configuration is read and saved locally.
- Standard Mode: If no local file is detected, configuration is automatically stored in `%LOCALAPPDATA%\ZenithBar\Settings.json`.

### Example `Settings.json`

```json
{
  "DockEdge": "Right",
  "ScreenIndex": 0,
  "Culture": "en-US",
  "UseAppBar": true,
  "AlwaysTop": true,
  "AutoUpdate": true,
  "RunAtStartup": false,
  "UiScale": 1.0,
  "PollingInterval": 1000,
  "ToolbarMode": true,
  "ClickThrough": false,
  "ShowTrayIcon": true,
  "CollapseMenuBar": false,
  "InitiallyHidden": false,
  "SidebarWidth": 300,
  "BgOpacity": 0.95,
  "TextAlign": "Left",
  "FontSize": 14,
  "FontColor": "#ffffff",
  "AlertFontColor": "#ef4444",
  "AlertBlink": true,
  "ShowMachineName": true,
  "ShowClock": true,
  "Clock24hr": true,
  "EnableCpu": true,
  "EnableRam": true,
  "EnableGpu": true,
  "EnableDisks": true,
  "EnableNetwork": true,
  "ShowCpuCores": true,
  "ShowAllDisks": false,
  "ShowSparklines": true,
  "HotkeyToggle": "Ctrl+Shift+S",
  "HotkeyCycleEdge": "Ctrl+Shift+E",
  "HotkeyCycleScreen": "Ctrl+Shift+M",
  "HotkeyReserveSpace": "Ctrl+Shift+R"
}
```

---

## Automated Releases

The repository includes a GitHub Actions workflow (`.github/workflows/release.yml`) that automates version management and release distribution:

- Trigger: Pushes to the `main` branch.
- Version Determination: Automatically parses commit history following Conventional Commits (`feat:`, `fix:`, `perf:`, `refactor:`, `chore:`, breaking changes). When no prior tag exists, the initial release tag defaults to `v0.1.0`.
- Release Assets:
  - `ZenithBar-windows-x64.zip` (containing `ZenithBar.exe`, `Settings.json`, and `README.md`)
  - Standalone `ZenithBar.exe`
  - `SHA256SUMS.txt` cryptographic verification file

---

## License

Distributed under the terms of the MIT License and the Apache License (Version 2.0).
