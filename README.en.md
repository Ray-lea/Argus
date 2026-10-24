<div align="center">

<img src="ui/public/logo.svg" width="112" height="112" alt="Argus logo">

# Argus

**A fast, modern and informative task manager for Windows**

[简体中文](README.md) · **English**

[![Latest release](https://img.shields.io/github/v/release/Ray-lea/Argus?include_prereleases&sort=semver&label=release&color=2f80ed)](https://github.com/Ray-lea/Argus/releases)
[![Downloads](https://img.shields.io/github/downloads/Ray-lea/Argus/total?label=downloads&color=2ea44f)](https://github.com/Ray-lea/Argus/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%2F%2011%20x64-0078D4)](#-system-requirements)
[![License](https://img.shields.io/badge/license-AGPL--3.0--or--later-A42E2B?logo=gnu&logoColor=white)](LICENSE)

[![Rust](https://img.shields.io/badge/Rust-stable-B7410E?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)](https://tauri.app/)
[![SolidJS](https://img.shields.io/badge/SolidJS-1.9-2C4F7C?logo=solid&logoColor=white)](https://www.solidjs.com/)
[![TypeScript](https://img.shields.io/badge/TypeScript-7-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)

[Features](#-features) · [Download](#-download) · [Build from source](#-build-from-source) · [Keyboard shortcuts](#-keyboard-shortcuts) · [Documentation](#-documentation) · [Community](#-community)

</div>

<br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/screenshot-en-dark.png">
  <img src="docs/images/screenshot-en-light.png" alt="The Argus processes page: processes grouped by app, with CPU, memory, disk, network and GPU at a glance">
</picture>

<br>

Argus queries Windows native APIs directly in Rust, with an interface built on Tauri 2 and SolidJS. It brings processes, performance, services, startup apps, network connections, and 7-day history together in a single window, featuring charts that flow smoothly at the display's refresh rate while using only about 1% of a single CPU core in the system tray. The project is named after Argus Panoptes, the hundred-eyed giant of Greek mythology.

## ⭐ Star history

If Argus is useful in your daily workflow, please consider giving it a star on GitHub. It is the most direct way to support the project and helps more people discover it.

<a href="https://www.star-history.com/#Ray-lea/Argus&Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=Ray-lea/Argus&type=Date&theme=dark">
    <img src="https://api.star-history.com/svg?repos=Ray-lea/Argus&type=Date" alt="Number of stars on Argus over time">
  </picture>
</a>

## ✨ Features

| | Feature | Description |
|:---:|---|---|
| 🧮 | **Processes** | Browse by app, process tree, or flat list, with real-time CPU, memory, disk read/write rates, and GPU usage for each process (plus network traffic when the privileged service is enabled or running as administrator). Sort, search, and end a process or its entire tree; inspect the command line, user, integrity level, and more in the details pane. |
| ⚡ | **Priority and efficiency mode** | Adjust them on the fly from the context menu, or create rules that apply automatically whenever a program starts. |
| 📈 | **Performance** | Monitor CPU (per-core load, P-cores and E-cores, frequency, temperature, and power), memory, disks, network, GPU, and battery in detail. |
| 🧩 | **Services and startup apps** | Start and stop Windows services and change their startup type; enable or disable startup apps, staying in sync with Windows Task Manager. |
| 🌐 | **Connections and ports** | Inspect active TCP and UDP endpoints alongside their owning processes, with exact port filtering and the ability to close connections. |
| 🔍 | **File locks** | Drop a file or folder onto the window to identify which processes are holding it. Combines Restart Manager with handle enumeration (including processes' current working directories), with broader coverage across elevated processes when the privileged service is enabled. |
| 🕘 | **History** | Automatically records resource usage charts over the past 7 days; click any point in time to inspect the busiest processes at that moment. |
| 📌 | **System tray** | Show or hide the window anytime with the global shortcut Ctrl+Alt+Esc (which can be changed or turned off); uses minimal resources while hidden, with separate refresh rates for minimized and tray states. |
| 🔄 | **In-app updates** | New versions download quietly in the background and notify in the title bar; preview all changes since the current version before installing, and review what's new the first time the window opens after an update. Offers Stable and Beta channels with optional GitHub proxies, and installs only updates that pass signature verification. |
| 🎨 | **Native look** | Features Mica backdrop on Windows 11 and a clean solid background on Windows 10; the system accent color and light or dark theme follow Windows by default, with a theme override in Settings. |
| 🌍 | **Languages** | Available in English and Simplified Chinese, following the Windows display language by default; includes a welcome guide on first launch to configure essential settings. |
| 👤 | **No administrator rights needed** | View executable paths and resource usage across all processes as a standard user. |
| 🛡️ | **Optional privileged service** | After a one-time administrator approval, Argus running as a standard user can also manage system processes, apply rules automatically, control services, close connections, and measure per-process network traffic (as well as disk reads and writes on Windows 10 and Windows 11 prior to 24H2); can be uninstalled anytime in Settings. |
| 🪟 | **Tasks and desktop** | Run new tasks from any page (optionally as administrator), or restart the current desktop's Windows Explorer. |

## 📦 Download

Get the latest release from [GitHub Releases](https://github.com/Ray-lea/Argus/releases):

| File | Description |
|---|---|
| `Argus_<version>_x64-setup.exe` | **Installer (recommended).** Installs for all users into `Program Files`; installing and updating require administrator permission. Supports in-app updates. |
| `Argus_<version>_x64.exe` | **Portable version.** Runs directly as a standalone executable and shares settings and history with the installed version; does not support automatic updates. |
| `SHA256SUMS.txt` | SHA-256 checksums of the release files above. |

> [!NOTE]
> Argus is not code-signed yet, so Windows SmartScreen may warn about an "unknown publisher" the first time it runs.

## 💻 System requirements

- **Operating system**: Windows 10 version 22H2 (build 19045) or later, including Windows 11 (x64 only)
- **Runtime**: Microsoft Edge WebView2 Runtime (included in Windows 11)

See [compatibility](docs/guide/hardware-data.md#系统兼容范围) (in Chinese) for tested Windows versions, feature differences, and permission requirements.

## 🔧 Build from source

Prerequisites:

- Rust `stable-x86_64-pc-windows-msvc` toolchain (1.99.1 or later)
- Node.js 26.x from 26.12.1 (24.x LTS from 24.22.1 is also supported) and pnpm 12.14.0
- Visual Studio Build Tools ("Desktop development with C++" workload)

Run the following commands in the repository root:

```bash
start.bat           # development mode: instant frontend reload; Rust changes rebuild and restart automatically
start.bat build     # release build: outputs target\release\argus.exe
```

For browser preview, code checks, packaging the installer, and cleaning caches, see the [development guide](docs/development/README.md) (in Chinese).

## 🔑 Keyboard shortcuts

| Action | Keys |
|---|---|
| Switch pages | <kbd>Ctrl</kbd>+<kbd>1</kbd> … <kbd>Ctrl</kbd>+<kbd>7</kbd> |
| Search processes | <kbd>Ctrl</kbd>+<kbd>F</kbd> |
| Temporarily pause updates | Hold <kbd>Ctrl</kbd>; release to resume (Processes and Performance) |
| Open details | <kbd>Enter</kbd> |
| End task | <kbd>Alt</kbd>+<kbd>E</kbd> or <kbd>Delete</kbd> (<kbd>Shift</kbd>+<kbd>Delete</kbd> skips the confirmation) |
| Show or hide the window (global) | <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Esc</kbd> |

The full shortcut list is available in Settings → Keyboard shortcut → View shortcuts, and further usage details are covered in the [user guide](docs/guide/README.md) (in Chinese).

## 📚 Documentation

The documentation is in Chinese.

| Document | Overview |
|---|---|
| [User guide](docs/guide/README.md) | Interface and operations, background tray and shortcuts, settings, hardware data sources and limitations |
| [Development guide](docs/development/README.md) | Environment and commands, builds and caches, performance baselines and measurement |
| [Architecture](docs/architecture/README.md) | System layers and data flow, tech stack, module responsibilities |
| [Design](docs/design/README.md) | Design principles, chart animation, window appearance and materials |
| [Roadmap](docs/roadmap.md) | Completed and planned features |
| [Known issues](docs/known-issues.md) | Known limitations of the current version |
| [Changelog](CHANGELOG.en.md) | Detailed changes in each release |

## 💬 Community

[![QQ group 453803497](https://img.shields.io/badge/QQ%20group-453803497-12B7F5?logo=qq&logoColor=white)](https://qm.qq.com/q/97YqJAQRNK)

Join the QQ group **453803497** (Chinese-speaking) to report problems, suggest features, or talk with other users: click the badge above, [this link](https://qm.qq.com/q/97YqJAQRNK), or Settings → About → Join QQ group in Argus.

## 🤝 Contributing

Issues and pull requests are welcome; please read the [contributing guide](.github/CONTRIBUTING.md) first. Report security vulnerabilities privately as described in the [security policy](.github/SECURITY.md), rather than in public issues.

## 🧭 Roadmap

- [x] **M0–M3**: processes, performance, services, startup apps, network connections, and file locks
- [x] **Languages, welcome guide, and in-app updates**
- [x] **M4**: per-process GPU usage, disk reads and writes, and ETW network traffic; priority and efficiency mode rules; history
- [x] **Optional hardware sensors**: per-core CPU readings, effective clocks, voltage, and power limits via the official PawnIO driver, plus AMD and Intel GPU telemetry (see [hardware coverage](docs/guide/hardware-data.md), in Chinese)
- [ ] **M5**: optional privileged service, rules and network collection through it, handle-based file lock queries, and [Task Manager replacement](docs/architecture/task-manager.md) (in Chinese) are complete; consumer hardware coverage continues to expand
- [ ] **Extended keyboard shortcuts**: distinguishing left and right modifiers, and supporting modifier-only or two-key combinations

See the [roadmap](docs/roadmap.md) (in Chinese) for full details.

## 🙏 Acknowledgements

Argus is built upon the following open-source projects, with sincere thanks:

| Project | Used for |
|---|---|
| [Tauri](https://tauri.app/) and its plugins | App host, system tray, start at sign-in, and in-app updates |
| [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/), [webview2-com](https://github.com/wravery/webview2-rs) | Interface rendering and native integration |
| [windows-rs](https://github.com/microsoft/windows-rs) | Rust bindings for Windows native APIs |
| [SolidJS](https://www.solidjs.com/), [TanStack Virtual](https://tanstack.com/virtual) | UI framework and virtual scrolling for long lists |
| [Vite](https://vite.dev/), [Tailwind CSS](https://tailwindcss.com/), [TypeScript](https://www.typescriptlang.org/), [Biome](https://biomejs.dev/) | Frontend build, styling, and linting |
| [serde](https://serde.rs/), [reqwest](https://github.com/seanmonstar/reqwest) and other Rust crates | Serialization, networking, and other foundational functionality |
| [PawnIO](https://pawnio.eu/), [PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) | Signed driver and collection modules behind the optional kernel sensors |
| [ZenStates-Core](https://github.com/irusanov/ZenStates-Core), [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor), [GNR-SMU](https://github.com/Kyworn/gnr-smu) | CPU register layout references such as AMD SMU tables |
| [Intel IGCL](https://github.com/intel/drivers.gpu.control-library), [AMD ADL](https://github.com/GPUOpen-LibrariesAndSDKs/display-library) | GPU telemetry interfaces |

All third-party components and their license notices are listed in [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).

Thanks to the [LINUX DO](https://linux.do/) community for its support.

## 📄 License

Argus is free software, released under the [GNU Affero General Public License](LICENSE), version 3 or (at your option) any later version (`AGPL-3.0-or-later`).

<div align="center">
<br>
<sub>Argus · Task manager for Windows · © 2026 Argus contributors</sub>
</div>
