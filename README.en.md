<div align="center">

<img src="ui/public/logo.svg" width="112" height="112" alt="Argus logo">

# Argus

**A fast, modern and informative task manager for Windows**

[简体中文](README.md) · **English**

[![Latest release](https://img.shields.io/github/v/release/Ray-lea/Argus?include_prereleases&sort=semver&label=release&color=2f80ed)](https://github.com/Ray-lea/Argus/releases)
[![Downloads](https://img.shields.io/github/downloads/Ray-lea/Argus/total?label=downloads&color=2ea44f)](https://github.com/Ray-lea/Argus/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%2011%20x64-0078D4)](#-system-requirements)
[![License](https://img.shields.io/badge/license-AGPL--3.0--or--later-A42E2B?logo=gnu&logoColor=white)](LICENSE)

[![Rust](https://img.shields.io/badge/Rust-stable-B7410E?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)](https://tauri.app/)
[![SolidJS](https://img.shields.io/badge/SolidJS-1.9-2C4F7C?logo=solid&logoColor=white)](https://www.solidjs.com/)
[![TypeScript](https://img.shields.io/badge/TypeScript-7-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)

[Features](#-features) · [Download](#-download) · [Build from source](#-build-from-source) · [Keyboard shortcuts](#-keyboard-shortcuts) · [Documentation](#-documentation)

</div>

<br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/screenshot-en-dark.png">
  <img src="docs/images/screenshot-en-light.png" alt="The Argus processes page: processes grouped by app, with CPU, memory, disk, network and GPU at a glance">
</picture>

<br>

Argus reads Windows' native interfaces directly from Rust, with an interface built on Tauri 2 and SolidJS. Processes, performance, services, startup apps, network connections and the last 7 days of history share one window, and the charts move smoothly at the display's refresh rate; resident in the system tray, it uses about 1% of one CPU core. It is named after Argus Panoptes, the hundred-eyed giant of Greek myth.

## ✨ Features

| | Feature | Description |
|:---:|---|---|
| 🧮 | **Processes** | View by app, as a process tree or as a list, including each process's GPU usage, plus network traffic when the privileged service is enabled or Argus runs as administrator. Sort, search, end a process or its whole tree; the details pane shows the command line, user, integrity level and more. |
| ⚡ | **Priority and efficiency mode** | Set them from the context menu, or create rules that apply them every time a program starts. |
| 📈 | **Performance** | CPU (per core, P-cores and E-cores, frequency, temperature, power), memory, disks, network, GPU and battery. |
| 🧩 | **Services and startup apps** | Start and stop services and change their startup type; enable or disable startup apps with the same switch as Windows Task Manager. |
| 🌐 | **Connections and ports** | TCP and UDP endpoints with the processes they belong to; search by exact port and close connections. |
| 🔍 | **File locks** | Drop a file or folder onto the window to find the processes using it, combining Restart Manager with handle enumeration, including processes' current directories; the privileged service can check more processes. |
| 🕘 | **History** | Resource charts for the last 7 days; click any moment to see the busiest processes at that time. |
| 📌 | **System tray** | Show or hide the window with the global shortcut Ctrl+Alt+Esc; it uses very little while hidden. |
| 🔄 | **In-app updates** | Stable and beta channels, with optional GitHub proxies; only updates that pass signature verification are installed. |
| 🎨 | **Native look** | Mica background and the system accent color; dark and light themes follow Windows by default or can be chosen in Settings. |
| 🌍 | **Languages** | English and Simplified Chinese, following the Windows display language by default; a welcome guide sets up the essentials on first start. |
| 👤 | **No administrator rights needed** | See the path and resource usage of every process as a standard user. |
| 🛡️ | **Optional privileged service** | After a single administrator approval, Argus running as a standard user can also manage system processes, apply rules automatically, measure per-process network traffic, control services and close connections; it can be uninstalled in Settings. |

## 📦 Download

Get the latest version from [GitHub Releases](https://github.com/Ray-lea/Argus/releases):

| File | Description |
|---|---|
| `Argus_<version>_x64-setup.exe` | **Installer (recommended).** Installs for all users into Program Files; installing and updating require administrator permission. Supports in-app updates. |
| `Argus_<version>_x64.exe` | **Portable version.** Runs directly and shares settings and data with the installed version; no automatic updates. |
| `SHA256SUMS.txt` | SHA-256 checksums of the files above. |

> [!NOTE]
> Argus is not code-signed yet, so Windows SmartScreen may warn about an "unknown publisher" the first time it runs.

## 💻 System requirements

- Windows 11 x64
- WebView2 Runtime (included in Windows 11)

## 🔧 Build from source

Requires Rust (stable, MSVC toolchain), Node.js 22 or later, pnpm, and Visual Studio Build Tools ("Desktop development with C++"). In the repository root:

```bash
start.bat           # development mode: instant frontend reload; Rust changes rebuild and restart automatically
start.bat build     # release build: target\release\argus.exe
```

For the browser preview, checks, packaging the installer and cleaning caches, see the [development guide](docs/development/README.md) (in Chinese).

## 🔑 Keyboard shortcuts

| Action | Keys |
|---|---|
| Switch pages | <kbd>Ctrl</kbd>+<kbd>1</kbd> … <kbd>Ctrl</kbd>+<kbd>7</kbd> |
| Search processes | <kbd>Ctrl</kbd>+<kbd>F</kbd> |
| Open details | <kbd>Enter</kbd> |
| End task | <kbd>Delete</kbd> (<kbd>Shift</kbd>+<kbd>Delete</kbd> skips the confirmation) |
| Show or hide the window (global) | <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Esc</kbd> |

The [user guide](docs/guide/README.md) covers everything else.

## 📚 Documentation

The documentation is in Chinese.

| Document | Contents |
|---|---|
| [User guide](docs/guide/README.md) | Interface and operations, tray and shortcuts, settings, where hardware data comes from |
| [Development guide](docs/development/README.md) | Environment and commands, builds and caches, performance baselines and measurement |
| [Architecture](docs/architecture/README.md) | Layers and data flow, module responsibilities |
| [Design](docs/design/README.md) | Design principles, chart animation, window appearance |
| [Roadmap](docs/roadmap.md) | Done and planned features |
| [Known issues](docs/known-issues.md) | Limitations of the current version |
| [Changelog](CHANGELOG.md) | Changes in each version |

## 🤝 Contributing

Issues and pull requests are welcome; please read the [contributing guide](.github/CONTRIBUTING.md) first. Report security vulnerabilities privately as described in the [security policy](.github/SECURITY.md), not in public issues.

## 🧭 Roadmap

- [x] **M0–M3**: processes, performance, services, startup apps, network connections and file locks
- [x] **M4**: per-process GPU usage, priority and efficiency mode rules, history
- [x] **M5**: optional privileged service, rules and network collection through it, file locks by handle, [Task Manager replacement](docs/architecture/task-manager.md)
- [ ] Acceptance of the Task Manager replacement in a real administrator environment
- [ ] Features that need a kernel driver

See the [roadmap](docs/roadmap.md) for details.

## 📄 License

Argus is free software, released under the [GNU Affero General Public License](LICENSE), version 3 or (at your option) any later version (AGPL-3.0-or-later).

<div align="center">
<br>
<sub>Argus · Task manager for Windows · © 2026 Argus contributors</sub>
</div>
