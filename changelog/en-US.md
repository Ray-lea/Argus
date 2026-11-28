# Changelog

[简体中文](zh-CN.md) · [繁體中文](zh-TW.md) · **English** · [Español](es-ES.md) · [Deutsch](de-DE.md) · [日本語](ja-JP.md) · [한국어](ko-KR.md) · [Français](fr-FR.md) · [Русский](ru-RU.md)

User-visible changes in each Argus release. Versions follow [Semantic Versioning](https://semver.org/). This file is translated from the [Simplified Chinese changelog](zh-CN.md), entry for entry.

## [0.7.0](https://github.com/Ray-lea/Argus/compare/v0.6.0...v0.7.0) - 2026-11-27

Argus v0.7.0 introduces Windows ARM64 installer and portable builds, with the app, the privileged service and Task Manager replacement running natively on ARM64. Processes adds 27 optional columns and two actions, “Create memory dump file” and “Debug”. A new byte units setting arrives, the taskbar monitor supports vertical taskbars, and Argus can send a Windows notification when a new version is available. This release also reduces redrawing while the interface is idle and fixes issues including Ryzen 9000 sensors, the taskbar monitor covering the Start button and a privileged service that could not be reinstalled after its registration was deleted.

This release completely rewrites the documentation site; visit the [documentation site](https://argustask.com/docs/) (in Chinese) for detailed usage of each feature.

### Breaking changes

- **Column menu**: The column header context menu on Processes no longer shows or hides columns by category. Click “Select columns…” in the menu and choose the columns to show in the dialog.

### Features

- **Windows ARM64**: ARM64 installer and portable builds are available, with the app, the privileged service, administrator mode and Task Manager replacement running natively and in-app updates staying on the ARM64 build. Performance shows the vendor, model, instruction sets and virtualization status of ARM64 processors, and CPU power comes from Windows energy meters; kernel sensors are not available on ARM64 yet. ([#172](https://github.com/Ray-lea/Argus/issues/172))
- **Process columns**: The “Select columns” dialog adds 27 optional columns, including running time, kernel and user CPU, context switches, shared working set, paged and non-paged pool, hard faults, and disk and I/O operation counts and totals. Sampling cost is unchanged while none of these columns is selected.
- **Details pane**: Lists the readings of every optional process column. The Resources and Totals sections add context switches, hard faults and requests per second, and readings such as working set and commit size show their peaks. “Copy details” includes these items as well.
- **Memory dumps and debugging**: The context menu of a single process adds “Create memory dump file” and “Debug”. Dump files are written to the temporary folder, and a notification shows the path when done; when a just-in-time debugger is registered, “Debug” starts it and attaches it to the process. ([#169](https://github.com/Ray-lea/Argus/issues/169), [#197](https://github.com/Ray-lea/Argus/issues/197))
- **Byte units**: “Settings → Data and history” adds “Byte units”, with Traditional (default), Binary IEC or Decimal SI. Memory, storage capacity, transfer rates, chart scales and monitor readings switch accordingly. ([#156](https://github.com/Ray-lea/Argus/issues/156))
- **Vertical taskbar**: When the taskbar is on the left or right of the screen, the taskbar monitor stacks its readings along the taskbar. All four styles have a vertical layout and show only the readings that fit. ([#177](https://github.com/Ray-lea/Argus/issues/177))
- **New version notifications**: When the window is hidden in the tray or not in the foreground, the installed version sends a Windows notification once a new version is downloaded and verified, and the portable version once a new version is found; clicking it opens the update dialog. With “Notify me when an update is available” turned off under “Settings → Updates”, the notice appears at the bottom right of the window when it is shown.
- **Switching pages**: Ctrl+Tab and Ctrl+Shift+Tab switch to the next or previous page, wrapping around at either end. ([#185](https://github.com/Ray-lea/Argus/issues/185))
- **Top navigation**: When the page bar runs out of space, the top navigation tabs collapse to icons first, and hovering shows the page name and shortcut; only if space is still short do the page controls move to the next line. ([#87](https://github.com/Ray-lea/Argus/issues/87))
- **Documentation site**: All documentation is rewritten. A new “Getting started” section covers Quick start, What is Argus, Installation and updates, and a comparison with Windows Task Manager; the user guide is split by app page into Processes, Performance, Services and startup apps, Connections, History and more, with descriptions checked and completed against the code.

### Performance

- **Redrawing**: Visible windows whose content is unchanged no longer redraw on every screen refresh. On a 160 Hz display, CPU cycles per second with Settings idle drop by about half; the presented frame rates of animation and scrolling are unchanged.

### Fixes

- **Tray icon**: The default style is now “App icon”, as “Trend” was hard to make out on dark taskbars; Components now selects the tray icon style from a drop-down list. ([#178](https://github.com/Ray-lea/Argus/issues/178))
- **Taskbar monitor**: With centered taskbar buttons, readings no longer cover the Start button and app buttons; only the readings that fit are shown. When Group Policy disables widgets, the monitor no longer leaves a gap for the Widgets button.
- **Desktop monitor**: After Windows starts or Argus restarts, an enabled desktop monitor appears right away without being turned off and on again.
- **Ryzen 9000**: Models with disabled cores in a CCD (such as the Ryzen 9 9900X3D) no longer show “Unrecognized sensor table” and read the current values and limits of PPT and TDC. Once the core mapping is verified, per-core temperatures, voltages and C0/C1/CC6 residency also appear. ([#203](https://github.com/Ray-lea/Argus/issues/203))
- **GPU and CPU readings**: While a discrete GPU sleeps, the GPU page keeps the last “Max core clock” it read. With kernel sensors on and a 30-second background refresh interval, CPU power and effective clock speed no longer come and go, and history recorded in the background no longer misses them.
- **Privileged service and kernel sensors**: When security software or a cleanup tool deletes the service's registration, the service can be reinstalled from Argus right away without restarting Windows first. After PawnIO is uninstalled or its device is removed, turning kernel sensors back on reinstalls the driver automatically if the installed driver is version 2.2.0 or earlier.
- **Administrator mode**: Administrator mode can be turned on even when Task Scheduler keeps a leftover registration with the same name; if registration is refused, the message points to possible blocking by security software such as 360. Error messages show each error code once, with HRESULTs in hexadecimal.
- **Task Manager replacement**: With replacement turned on, clicking “Open Windows Task Manager once” from an administrator account no longer does nothing. When Argus does not respond, Task Manager entry points fall back to Windows Task Manager without waiting through an extra timeout.
- **Right-to-left text**: Arabic, Hebrew and other right-to-left text in process names, descriptions and the search box no longer appears reversed. ([#155](https://github.com/Ray-lea/Argus/issues/155))
- **Menus and panes**: Long submenus opened near the bottom of the window no longer have their last item cut off by the window edge. The details pane sizes its label column to the longest label, so labels such as “Dedicated GPU memory” no longer wrap. Dialog cards no longer flash white when closing in Mica windows.
- **Icons after updating**: After installing or updating, desktop shortcuts and Argus pinned to the taskbar show the new icon immediately, without restarting the PC.

### Upgrading

- **Privileged service**: The communication protocol remains version 12. Ryzen 9000 readings, kernel sensor power in the background, the Task Manager replacement fixes and, on Windows versions before Windows 11 24H2 with standard permissions, disk operation counts take effect once the service is updated. Upgrading the installed version updates an installed service; with the portable version, click “Update service” under “Settings → Permissions → Privileged service”.
- **Installed version on ARM64 PCs**: On ARM64 PCs, version 0.6.0 still downloads the x64 installer, which asks for the ARM64 version and exits. Download `Argus_0.7.0_arm64-setup.exe` from the website and install it over the existing location; settings and history are kept, and later in-app updates use the ARM64 build.
- **Portable version on ARM64 PCs**: Exit Argus and replace the program with `Argus_0.7.0_arm64.exe`. If “Allow running as administrator” is on, turn it off first and turn it on again from the new version after replacing the file.
- **Tray icon**: A tray icon whose style was never changed shows the app icon after upgrading. To show live CPU usage, choose “Trend”, “Share” or “Number” under “Tray icon” on Components.

## [0.6.0](https://github.com/Ray-lea/Argus/compare/v0.5.0...v0.6.0) - 2026-11-23

Argus v0.6.0 introduces a new dot-matrix performance chart app icon and a redesigned main window, with a streamlined title bar, reorganized page and status bars, and an Always on top button. The new “Components” page shows live system readings in the tray, on the desktop and on the taskbar; the redesigned History page supports multi-lane charts, resource events and cross-run process observation. Processes adds resource leak alerts and power usage columns, Processes, Services, Startup and Connections support copying cells and CSV export, services and startup entries can be deleted, Performance adds NPU monitoring, and touch input and a hardware acceleration switch are now available. This release also reduces scrolling and redraw overhead, fixes a blue screen when opening disk details on some Intel RST VMD systems, and avoids waking powered-off discrete GPUs and spun-down hard disks.

### Breaking changes

- **History**: This release uses a new storage format, so History shows only data recorded from version 0.6.0 onward. The first write after upgrading replaces the old-format file for that day, if present; earlier versions of Argus cannot read the new records.
- **Privileged service protocol**: The communication protocol changes from version 11 to 12. Update an existing service before using it to end processes or change I/O or memory priorities.

### Features

- **App icon**: Switches to a new dot-matrix performance chart icon with dedicated layouts per size so it stays sharp on the taskbar, in the title bar and in updated shortcuts. About and the welcome guide form the logo from dots and transition into a live dot chart of the last 10 CPU samples; with “Interface animations” off, the dot chart appears immediately.
- **Title bar**: Streamlined to the app logo, a search box that adapts to window width and Windows standard-size window buttons, leaving all remaining empty space draggable. A new pin button toggles “Always on top” and persists across restarts. ([#121](https://github.com/Ray-lea/Argus/issues/121))
- **Page bar and status bar**: Page controls and “More actions” move beside the navigation tabs, and process view switches use icons. “More actions” gathers “Run new task…” (Ctrl+N), “Find processes using a file…”, keyboard shortcuts and, on Services, the Windows Services console. The status bar adds pause and refresh-rate controls, while the version number also shows update progress and opens the install prompt. ([#87](https://github.com/Ray-lea/Argus/issues/87))
- **Components page**: Adds the “Components” page to manage tray and background options and to turn on the desktop and taskbar monitors (both off by default). Opening settings from the tray or a monitor selects its section in the sidebar.
- **Live tray icon**: Shows live CPU usage with “Trend” by default, or switches to “Share”, “Number” or “App icon”; the three live styles turn amber at 80% usage. Hover for a live readings card, or right-click to open the quick panel listing the three busiest processes by CPU usage and common actions.
- **Tray card and quick panel**: The hover card adapts its layout to the language and font size, with a system tooltip available when disabled. Click readings in the quick panel to jump to the corresponding device in Performance; opening the panel from the hidden-icons area keeps that area expanded. ([@digital-bombyx](https://github.com/digital-bombyx) [#113](https://github.com/Ray-lea/Argus/pull/113) [#153](https://github.com/Ray-lea/Argus/pull/153))
- **Desktop monitor**: Displays live readings in a persistent desktop mini-window in “Strip” or “Panel” layout, with “List”, “Curves” and “Gauges” styles for panels. Dragging stays within the current display’s work area, with options for always on top, position lock, click-through and opacity.
- **Taskbar monitor**: Displays live readings on the taskbar in “Labels”, “Curves”, “Capsules” or “Large figures” style. Preview and switch styles in Components or the right-click panel; the panel also selects the display, toggles readings and reorders them by dragging, taking effect immediately. ([#28](https://github.com/Ray-lea/Argus/issues/28), [#123](https://github.com/Ray-lea/Argus/issues/123))
- **Multi-display and window switching**: If the selected display disconnects, the taskbar monitor moves to the primary display and returns automatically when reconnected. Clicking the taskbar monitor or tray icon hides an unobstructed main window, or brings a hidden, minimized or covered window to the front; double-clicking the desktop monitor opens the main window.
- **Monitor readings**: Both monitors offer 18 readings, covering temperature, clock speed, power and fan speed. Select readings individually for each GPU, disk and network adapter, and drag categories or individual readings to reorder them. The taskbar monitor uses device colors for labels and identifies follow-up readings for the same CPU or GPU by metric name.
- **Monitor refresh interval**: While the desktop or taskbar monitor is active, Argus samples at the main window’s refresh rate. When the minimized or background interval differs, Settings shows the actual interval in effect while preserving the interval option used after both monitors close.
- **History page**: Redesigned into a multi-lane step chart where every lane can overlay a second reading. Search readings by name or device and drag lanes to reorder or resize them; when lanes exceed the chart height, scroll inside the chart.
- **Navigating history charts**: Scroll the wheel to zoom around the time under the pointer; zooming, clicking a curve or dragging a selection pauses live tracking. Click “Back to latest”, right-click the chart or press End to resume. Hatching marks unrecorded gaps, and the last 10 minutes show each actual sampling interval. ([#79](https://github.com/Ray-lea/Argus/issues/79), [#129](https://github.com/Ray-lea/Argus/issues/129))
- **Live history charts**: During live tracking, step curves advance continuously while the cursor on the hovered lane updates in real time as curves pass beneath it. Time range and axis transitions animate smoothly with fading overlays, and previously viewed long ranges reopen faster.
- **Time ranges and events**: The dropdown offers 5 minutes, 3 hours, 1 day, 3 days and “Custom…”; in the “Custom time range” dialog, select dates on a calendar or enter exact start and end times. Enable “Show the 30-day overview and events” for an hourly heat map and events such as high CPU load, low memory and recording gaps.
- **History process table**: Search processes for the selected moment or range, and sort by name to group apps, background processes and Windows processes. Expand a name group to inspect the top four instances and a combined total for the rest; the context menu overlays the program’s curve on the chart or copies its name and PIDs.
- **History chart tooltips**: Hovering over CPU and other per-process lanes lists the five busiest processes, while the process-count lane lists the five programs with the most instances and a total for the rest. Temperature, battery level and other device-only readings display device data only.
- **Recorded history**: Adds CPU clock speed, power and temperature, memory details, battery readings, and per-device readings for each disk, GPU and network adapter. Under “Settings → Data and history”, select categories to record, choose about 20, about 40 or all active processes per frame, and view the estimated daily storage use.
- **History storage**: Set independent age and size limits, whichever is reached first; defaults are 30 days and 2 GB, with records older than 7 days merged by minute into averages, peaks and the busiest processes. Changing the save location offers to move existing history and watch records; if the target folder already contains other files, Argus stores records in an “Argus History” subfolder.
- **Process observation**: Choose “Watch process” from the process context menu or details pane to record 14 readings per run for programs at the same path (memory, CPU, GPU, disk, I/O, network, threads and handles), or the current instance only if the path is unavailable. Starting a watch opens “Process observation” under History to switch between runs and compare readings with multi-lane charts. ([@hayxe1](https://github.com/hayxe1) [#82](https://github.com/Ray-lea/Argus/pull/82))
- **Watch records**: The last 10 minutes display individual samples, while earlier periods read from disk. Records are retained for up to 7 days, removing the oldest data first if the capacity limit is reached; “Stop recording” keeps existing data, and watches must be restarted after Argus restarts.
- **Process groups**: Distinct colors identify “Apps”, “Background processes”, “Windows processes” and each user group. While scrolling, the active group header stays pinned at the top, and markers beside the scroll bar indicate each group’s position and the current group name. With “Hide system processes” enabled, the process count also reports how many processes are hidden. ([#88](https://github.com/Ray-lea/Argus/issues/88))
- **Process power usage**: Adds optional “Power usage” and “Power usage trend” columns (hidden by default) that estimate power consumption from CPU, GPU and disk activity. The former shows the current level, and the latter shows the average over roughly two minutes. ([#134](https://github.com/Ray-lea/Argus/issues/134))
- **Process priorities**: The context menu now sets I/O priority and memory priority, with values shown in the details pane and optional columns. “Base priority” displays class names, keeps special values numeric and continues to sort numerically. ([@digital-bombyx](https://github.com/digital-bombyx) [#80](https://github.com/Ray-lea/Argus/pull/80) [#116](https://github.com/Ray-lea/Argus/pull/116))
- **Pause sorting while hovering**: When hovering over the process list pauses sorting, the sort column header shows a pause icon and the status bar displays “Sorting paused”. “Settings → Language and appearance” adds a “Pause sorting while hovering” switch, enabled by default. ([#86](https://github.com/Ray-lea/Argus/issues/86))
- **Resource leak alerts**: Processes automatically detects handle leaks, accumulating child processes and leftover processes, with an amber header badge to jump straight to them. The details pane shows evidence, count trends and suggested actions, updating on the next sample after a process exits. Programs can be ignored individually and restored—or alerts disabled—under “Settings → Language and appearance → Resource leak alerts”.
- **Copying table data**: Processes, Services, Startup and Connections all support copying the current cell, name or details from the context menu. On Processes, Ctrl+C copies the name and Ctrl+Shift+C copies details; when text is selected, Ctrl+C copies the selection first. ([@digital-bombyx](https://github.com/digital-bombyx) [#161](https://github.com/Ray-lea/Argus/pull/161), [#91](https://github.com/Ray-lea/Argus/issues/91), [#150](https://github.com/Ray-lea/Argus/issues/150))
- **CSV export**: Choose “Export current page as CSV…” from “More actions” on Processes, Services, Startup or Connections to export the complete list (including off-screen rows) while preserving column order, filters, sorting, display units and expanded groups. ([#139](https://github.com/Ray-lea/Argus/issues/139))
- **Details pane**: The “Resources” section adds peak working set, virtual address space, I/O rates and power usage; a new “Totals” section lists user and kernel CPU time, bytes by I/O category and page faults. The copy button at the top right copies all populated items by section.
- **Connections page**: Supports collapsible grouping by app, expanding matching groups automatically while searching. Status filters for Services and Connections move into the “Status” column header; Connections also adds TCP, TCPv6, UDP and UDPv6 filters that combine with the status filter. ([@digital-bombyx](https://github.com/digital-bombyx) [#152](https://github.com/Ray-lea/Argus/pull/152))
- **Delete services and startup apps**: Adds “Delete” to the context menu to remove a service or startup entry after confirmation. Services in use are removed by Windows once stopped and released; startup folder entries move to the Recycle Bin, while packaged app startup tasks remain disable-only. ([#114](https://github.com/Ray-lea/Argus/issues/114), [#146](https://github.com/Ray-lea/Argus/issues/146))
- **Packaged app services**: When Windows refuses to change a packaged app service’s startup type, Argus can write the setting to the registry after confirmation and marks entries that take effect after a restart. Error messages name the service and explain permission or protection limits. ([@digital-bombyx](https://github.com/digital-bombyx) [#112](https://github.com/Ray-lea/Argus/pull/112), [#168](https://github.com/Ray-lea/Argus/issues/168))
- **Memory readings**: Hatching in the Performance memory composition bar marks compressed memory in use, with hover details showing estimated stored data and saved memory. Memory tooltips on Processes also clarify the scope of system usage, per-process private working sets and group totals. ([@digital-bombyx](https://github.com/digital-bombyx) [#81](https://github.com/Ray-lea/Argus/pull/81), [#122](https://github.com/Ray-lea/Argus/issues/122))
- **NPU**: Performance displays the name, utilization and a 60-second live chart for each NPU identified by Windows, moving recognized NPUs out of the GPU list. ([@SpookySandwich](https://github.com/SpookySandwich) [#140](https://github.com/Ray-lea/Argus/pull/140), [#135](https://github.com/Ray-lea/Argus/issues/135), [#148](https://github.com/Ray-lea/Argus/issues/148))
- **CPU charts and disk details**: Overall/logical processor view switching and core type filters move into the CPU chart’s context menu; logical-processor charts keep readable proportions and omit clock speeds then titles when space is tight. Disk details display SMART raw values in decimal by default, and switching to hexadecimal applies to all disks and persists across restarts.
- **Hardware acceleration**: “Settings → Animations” adds a “Hardware acceleration” switch, enabled by default. Turning it off switches UI rendering to the CPU immediately without using the GPU. ([#127](https://github.com/Ray-lea/Argus/issues/127))
- **Touch**: Supports tap, double-tap, swipe scrolling, press-and-hold menus and scroll-bar dragging on Windows touch screens. Forwarded touch taps activate main-window controls properly, and focused text fields bring up the touch keyboard as needed. ([#73](https://github.com/Ray-lea/Argus/issues/73))
- **About page**: Adds a “Copy diagnostic information” button to copy version, run state, hardware and display settings in one click for issue reports. New shortcut buttons beneath the website and GitHub cards link directly to downloads, the user guide, changelog, issues, discussions and releases.
- **Location of administrator programs**: Privileged service, Task Manager replacement and administrator-mode binaries are now stored together in `Program Files\Argus Privileged` with fixed file names, keeping path-based security software exclusions valid across updates. When installing outside Program Files, an information icon on the installer page explains where these files are stored. ([@digital-bombyx](https://github.com/digital-bombyx) [#154](https://github.com/Ray-lea/Argus/pull/154), [#136](https://github.com/Ray-lea/Argus/issues/136), [#159](https://github.com/Ray-lea/Argus/issues/159))

### Performance

- **Drawing**: Updates, hover effects and clicks redraw only changed regions, significantly lowering GPU work when moving across lists or playing icon animations. In release-build benchmarks, scrolling the process list uses about 40% fewer CPU cycles per frame; with hardware acceleration off, scrolling and page switches also reuse already drawn content.
- **First readings**: Shortens initial collection wait time after launch so the interface displays initial readings sooner. ([@hayxe1](https://github.com/hayxe1) [#119](https://github.com/Ray-lea/Argus/pull/119))

### Fixes

- **Blue screen from disk details**: Checks the controller driver before querying temperature and NVMe health, preventing a blue screen when opening disk details on affected Intel RST VMD systems; skipped items appear as “Unknown”. If an abnormal exit occurs during or within two minutes after reading, automatic reads pause on the next start while still offering “Read anyway”. ([#48](https://github.com/Ray-lea/Argus/issues/48), [#133](https://github.com/Ray-lea/Argus/issues/133), [#160](https://github.com/Ray-lea/Argus/issues/160))
- **Device sleep**: Skips sensor queries for powered-off discrete GPUs on hybrid-graphics laptops to avoid repeatedly waking them, showing “Sleeping” and the reason on Performance. Idle hard disks retain their last temperature and skip temperature queries once spun down so they can stay asleep. ([#147](https://github.com/Ray-lea/Argus/issues/147))
- **CPU readings**: Calculates CPU usage from idle cycles, fixing inflated readings that counted the idle loop as load. Fixes missing clock speeds for hybrid processors on Windows 10 and Intel processors that failed to distinguish P-cores from E-cores. ([#137](https://github.com/Ray-lea/Argus/issues/137))
- **GPU readings**: Processes on integrated GPUs no longer count shared GPU memory as dedicated GPU memory. Without a hardware GPU, software rendering no longer counts as process GPU usage and Processes hides GPU columns. When a hardware GPU was detected at startup, driver updates or newly attached GPUs recover or appear without restarting.
- **Disk readings**: Fixes USB external hard disks failing to read SMART data or reporting “Unknown” health, and drive letters of virtual drives such as Google Drive appearing under disk 0.
- **File locks**: Fixes file-lock queries failing when the system has an unusually high handle count. Duplicate failure messages from the privileged service and local search are shown only once, and Argus no longer prompts to enable the privileged service when it is already installed.
- **Protection when ending processes**: Refuses to end a target when Argus cannot verify whether it is a critical system process, covering single processes, batch actions and process trees. ([@hayxe1](https://github.com/hayxe1) [#77](https://github.com/Ray-lea/Argus/pull/77))
- **Processes page**: Keeps column widths steady when searching or expanding and collapsing groups, and preserves row order when expanding child processes while hover-pause sorting is active. Fixes user-group rows not opening their context menu, double-clicking a selected row unexpectedly collapsing the details pane, and stray characters in program names and version strings.
- **Historical process jump**: Verifies PID, name and start time when jumping from History to a process, avoiding selecting another process that reused the same PID. ([@hayxe1](https://github.com/hayxe1) [#76](https://github.com/Ray-lea/Argus/pull/76))
- **Service and startup lists**: Automatically sizes columns to their contents, restores automatic width on border double-click and remembers manual widths across restarts. Unreadable startup states now display concisely as “Unavailable” in the Status column.
- **Privileged service and administrator mode**: Fixes system folder misdetection that blocked enabling administrator mode or the privileged service. Missing service files show “Damaged” with repair or uninstall actions; power loss during updates or removal preserves usable service files, and uninstalling Argus cleans up files left after deregistration. ([#111](https://github.com/Ray-lea/Argus/issues/111))
- **Task Manager replacement**: Clearly displays “Replacement removed” in Settings and the welcome guide when security software or another program reverts the replacement. When the maintenance helper is terminated abnormally, the error includes its exit code.
- **Install and load security**: The installer checks and protects eligible install directories so only administrators and the system can write to them; during a new interactive install, it explains why an unprotected location cannot be secured and lets you continue or pick another folder. Argus loads imported system DLLs only from the system directory and ignores same-named files in its run directory. ([#136](https://github.com/Ray-lea/Argus/issues/136))
- **Window display**: Fixes missing system transitions when maximizing or restoring, failing to stay maximized when reopened from the tray, and opening behind other windows on first launch or via Task Manager entry points. ([#126](https://github.com/Ray-lea/Argus/issues/126), [#142](https://github.com/Ray-lea/Argus/issues/142))
- **Scaling and scrolling**: Fixes pages getting stuck at the bottom and unscrollable dialogs at system or interface scale levels such as 125% or 150%. Scale inputs now fit three digits cleanly, and column options and hints no longer clip descenders. ([#164](https://github.com/Ray-lea/Argus/issues/164))
- **Menus and dialogs**: Context menus and submenus dismiss immediately, while drop-down lists expand from and retract toward their triggers. Dialogs zoom and fade in from the window center, dim window button backgrounds while open, and shrink out on close. Fixes tooltips covering context menus and disabled items showing warning or hover highlights. ([#141](https://github.com/Ray-lea/Argus/issues/141))
- **Layout**: Fixes the Performance overview overflowing at the default window size; Settings expands appropriately in wide windows, and dialogs hide unneeded scroll bars when content fits.
- **Screen readers**: Supports reading page headings, descriptions, performance readings and table cells, keeping controls accessible after page switches or refreshes. Settings switches and number inputs now take their accessible names from their setting titles. ([#115](https://github.com/Ray-lea/Argus/issues/115))
- **Interface text**: Trims redundant UI descriptions and standardizes CPU, GPU and I/O terms and hardware units across all languages; the Russian interface also revises error messages, grammar and unit spacing. ([@Dimmitrius](https://github.com/Dimmitrius) [#93](https://github.com/Ray-lea/Argus/pull/93) [#95](https://github.com/Ray-lea/Argus/pull/95) [#96](https://github.com/Ray-lea/Argus/pull/96) [#98](https://github.com/Ray-lea/Argus/pull/98) [#100](https://github.com/Ray-lea/Argus/pull/100) [#101](https://github.com/Ray-lea/Argus/pull/101) [#102](https://github.com/Ray-lea/Argus/pull/102) [#103](https://github.com/Ray-lea/Argus/pull/103) [#104](https://github.com/Ray-lea/Argus/pull/104) [#105](https://github.com/Ray-lea/Argus/pull/105) [#106](https://github.com/Ray-lea/Argus/pull/106) [#108](https://github.com/Ray-lea/Argus/pull/108) [#109](https://github.com/Ray-lea/Argus/pull/109))

### Upgrading

- **Privileged service**: Upgrading through the installer or in-app updater also updates the installed privileged service and Task Manager replacement copy. After replacing a portable executable, click “Update service” under “Settings → Permissions → Privileged service” and approve administrator authorization.
- **Location of administrator programs**: After upgrading the installed version or updating the portable version’s service, existing privileged service, Task Manager replacement and administrator-mode copies migrate automatically to `Program Files\Argus Privileged`. Any path-based trust added in security software for the old path must be added once for the new path.
- **Page shortcuts**: With the new “Components” page, Ctrl+7 opens Components, “Settings” moves to Ctrl+8 and “About” moves to Ctrl+9.
- **Settings and history**: After upgrading, system history opens to the last 5 minutes by default and remembers new selections thereafter. A saved “UDP” status filter on Connections resets to “All” and can be reapplied in the “Protocol” column. Settings files now save only nondefault values, so unchanged options update automatically with future defaults.

## [0.5.0](https://github.com/Ray-lea/Argus/compare/v0.4.0...v0.5.0) - 2026-11-12

v0.5.0 adds process suspend and resume, process groups by user, four process information columns and column widths that fit the content. The device list on the Performance page can hide and reorder devices. The interface is now available in Traditional Chinese, Spanish, German, Japanese, French and Russian. This release also avoids requests that can freeze some Intel RST systems after Argus starts, corrects a problem that blocked the safe removal of USB disks, and decreases memory and CPU usage while the window is hidden.

### Features

- **Suspend and resume processes**: Process and app group menus now offer "Suspend process" and "Resume process". Argus blocks suspension of critical system processes, the Desktop Window Manager and itself, and asks for confirmation before it suspends other processes. Suspended processes show a "Suspended" badge after the name. ([@digital-bombyx](https://github.com/digital-bombyx) [#65](https://github.com/Ray-lea/Argus/pull/65))
- **Process information columns**: Four new optional columns are available: "File version", "Architecture", "Elevated" and "Command line". They are hidden by default. Enable them in the column header menu or in "Select columns". ([@digital-bombyx](https://github.com/digital-bombyx) [#72](https://github.com/Ray-lea/Argus/pull/72))
- **Process column widths**: Automatic widths fit the content and leave unused space on the right. Drag a column border to set its width, or double-click it to restore automatic width. The new "Full width" option gives the remaining width to the Name column.
- **Group processes by user**: In Settings → Language and appearance → Group processes by user, select Automatic, Always group or Never group. With Automatic, Argus groups processes only when more than one user is signed in. The context menu of a group row has "Manage user accounts" and "Sign out".
- **Hide system processes**: The Processes and Connections pages have a new "Hide system processes" switch. It is off by default, and each page keeps its own setting. Edit the lists of programs to always show or to also hide on the Settings page. ([#32](https://github.com/Ray-lea/Argus/issues/32))
- **Process tree and overview band**: The process tree starts collapsed, and the header menu has "Expand all" and "Collapse all". Set "Process overview band" to Automatic, Always show or Hidden. Automatic mode hides the band when the window is too short.
- **Device list on the Performance page**: Right-click a device to hide it, or select the devices to show in the "Shown devices" submenu. Drag a device to change the order. Disks can be sorted by disk number, by drive letter or in a custom order.
- **CPU multi-core view**: Each logical processor graph shows kernel time, which can be hidden from the CPU graph menu. Argus identifies Intel low-power efficiency cores (LP E-cores) with a separate color and filter. The graphs fill complete rows when possible.
- **Hardware information**: The GPU page shows the driver version, driver date and DirectX version, including the feature level. Disk details support remaining life and total data read and written for more SATA SSDs. HDD statistics now include power-on hours, start/stop counts, reallocated sectors and pending sectors.
- **Interface languages**: The interface is now available in Traditional Chinese, Spanish, German, Japanese, French and Russian. The tray, system messages and the installer are also translated. Numbers and dates use the Windows regional format. ([@abc0922001](https://github.com/abc0922001) [#45](https://github.com/Ray-lea/Argus/pull/45), [@Rimuwu](https://github.com/Rimuwu) [#51](https://github.com/Ray-lea/Argus/pull/51), [@s0ftw4r8](https://github.com/s0ftw4r8) [#52](https://github.com/Ray-lea/Argus/pull/52), [@khativari](https://github.com/khativari) [#55](https://github.com/Ray-lea/Argus/pull/55), [@hayxe1](https://github.com/hayxe1) [#62](https://github.com/Ray-lea/Argus/pull/62))
- **Release notes**: The About page and the update dialog show release notes in the interface language. In Settings → Updates, "See what's new" opens the dialog, which has "Expand all" and "Collapse all" controls. Opening, expansion and hover transitions follow "Interface animations".
- **Icons and motion**: Interface icons now come from the Lucide icon library. When the pointer moves onto a button or a navigation item, its icon plays one short animation. The animation does not play when "Interface animations" is off.
- **Window and tray**: A separate "Window transitions" switch is available. "Reopen from tray" can open the default page. The default window size is now 1000×680, and the minimum size is now 640×480. ([#27](https://github.com/Ray-lea/Argus/issues/27), [#40](https://github.com/Ray-lea/Argus/issues/40))
- **Task Manager replacement**: "Replace Task Manager" has a new "Open Windows Task Manager once" button. It opens the Windows Task Manager and does not change the replacement settings.
- **Notices and reminders**: The portable version explains its differences from the installed version and provides download links in the welcome guide and at the bottom right of the window. The new "Star reminder" appears at most once a day and can be turned off in Settings → Updates. The tips above "This PC" can be collapsed.
- **Telegram channel**: About → Project and support has a new "Telegram channel" item. "Join channel" opens [t.me/argustask](https://t.me/argustask), and "Copy link" copies the channel link.

### Performance

- **Background memory usage**: The new "Free memory when hidden" setting releases chart images and returns unused memory to Windows after the window stays minimized or hidden in the tray for the set time (10 minutes by default). CPU usage for the interface decreases when all windows are hidden or minimized. Sampling, history recording and rules continue to run. ([@SpookySandwich](https://github.com/SpookySandwich) [#54](https://github.com/Ray-lea/Argus/pull/54))

### Fixes

- **System freeze**: Argus no longer reads the temperature of disks behind RAID controllers such as Intel RST. Some driver versions do not respond to this request, and the system can freeze a few seconds after Argus starts. The temperature of these disks now shows as unknown. ([#48](https://github.com/Ray-lea/Argus/issues/48))
- **Safe removal of USB disks**: Disk monitoring no longer keeps USB disks in use, so Windows can safely remove them. ([#57](https://github.com/Ray-lea/Argus/issues/57))
- **Disk list and performance data**: Some NVMe disks showed device information but no performance data. Disks also disappeared from the list when performance counters were not available. Both problems are corrected.
- **Disk details**: The SMART table keeps values close to their attribute names and avoids large gaps between groups. Raw values fit in narrow windows. Health bars use green for normal values and threshold-based warning colors for temperature and spare capacity.
- **Process data**: When system queries are slow, Argus continues to update the process list and keeps the last valid resource readings. Exited processes no longer stay in the list. Process actions update the displayed state even when refresh is paused. ([#66](https://github.com/Ray-lea/Argus/issues/66))
- **Process table**: Efficiency mode and pin icons appear next to the process name. Column headers align with the data columns. Process usage values include the percent sign.
- **Connections page**: Connections without an owning process now show as "No owning process".
- **CPU readings**: Hybrid CPUs now show the correct speed. Some 6-core Ryzen processors now show the correct per-core voltage and temperature. Intel internal sensor readings no longer stop at intervals, and the temperature graph no longer breaks. ([#58](https://github.com/Ray-lea/Argus/issues/58))
- **Authorization and uninstallation**: Argus can revoke authorization and uninstall after the permissions of Program Files or the runtime folder change. Kernel sensors can use a PawnIO driver that is already running. Uninstallation no longer leaves the "Argus Sensor Setup" folder and an empty "Argus Runtime" folder. ([#46](https://github.com/Ray-lea/Argus/issues/46))
- **Startup permissions**: After installation or an in-app update, Argus first tries to start with the desktop account's permissions, as it does from a shortcut. When Argus runs as administrator, it can restart File Explorer that already runs as administrator and keep its existing permissions.
- **Window display**: After the window size is restored, the interface fills the window again. After a cold start from the Task Manager replacement entry, a maximized window shows completely. The window background is no longer transparent on Windows 11 21H2. After a switch to another app, the Argus window no longer stays in front of other windows. ([#44](https://github.com/Ray-lea/Argus/issues/44), [#37](https://github.com/Ray-lea/Argus/issues/37), [#35](https://github.com/Ray-lea/Argus/issues/35))
- **Tray**: CPU usage no longer stays high after the window closes to the tray. After an in-app update, the tray icon keeps its position and does not move to the overflow area. ([#31](https://github.com/Ray-lea/Argus/issues/31))
- **Interface display**: The update dialog stays centered while it loads, and the file lock dialog stays in place when results appear. All content on the About page can be reached by scrolling in short windows. Text box context menus use the interface language. ([#39](https://github.com/Ray-lea/Argus/issues/39))
- **Numbers and rendering**: Live numbers stay in place when they update. Light text on dark backgrounds has corrected edges. Small colored artifacts no longer appear below the overview band.
- **Stability**: Argus no longer exits when the display timing is not valid or when network adapter address queries fail many times. The interface no longer stops responding when graphics device recovery and window closure occur at the same time.
- **Large fonts, process rules and startup items**: Performance readings no longer overlap the specifications at large font sizes. When a program name contains non-ASCII letters, the process menu no longer adds a duplicate rule. Argus no longer crashes when it reads the startup tasks of packaged apps. ([@hayxe1](https://github.com/hayxe1) [#59](https://github.com/Ray-lea/Argus/pull/59))
- **Build type**: Settings, About and the status bar show "Development build", "Beta", "Stable" from the running program. The update channel separately selects the source for future update checks.

### Upgrading

- **Privileged service**: This release updates the service communication protocol to version 11. An upgrade with the installer or an in-app update also updates the installed service and the Task Manager replacement copy. For the portable version, replace the executable, then select "Update service" in Settings → Permissions → Privileged service and approve the administrator prompt.
- **Settings and history**: The upgrade keeps your settings and history. At the first start, "Window transitions" uses the window transition setting of the previous "Interface animations" option.

## [0.4.0](https://github.com/Ray-lea/Argus/compare/v0.3.4...v0.4.0) - 2026-11-06

v0.4.0 migrates the application interface from Tauri, SolidJS and WebView2 to a native Rust + GPUI Kit stack, eliminating browser-engine overhead, rebuilding DirectX rendering and interaction, and optimizing high-refresh-rate animations, live charts, sampling updates and background resource usage. It also adds customizable process columns, disk health and SMART details, expanded Intel integrated graphics readings and new layout options. The [Argus website](https://argustask.com/en/) is now live, along with an official update mirror.

### Features

- **Customizable process columns**: Choose from 40 columns across six categories (General, CPU, Memory, Disk and I/O, Network, GPU), including dedicated and shared GPU memory, GPU engine, image name, virtual address space, peak working set, page faults and total I/O counters. Virtual address space and peak working set are shown per process. Right-click column headers to toggle columns continuously across category submenus, or use the two-column "Select columns" dialog to filter by category, search and reorder; dragging headers or selected columns reorders them live with smooth transitions; automatic widths reserve space by number format to prevent jitter, and manual widths are also saved. The Name column in the flat list and process tree views now matches the Apps view by showing the application name or file description first, with the executable name shown beside it or in the Image name column.
- **Disk health and details**: The disk Performance view adds an Overview / Details switch that reads NVMe, ATA/SATA, USB-SAT and SCSI/SAS device information on demand, showing health status, remaining life, temperatures and thresholds, power-on time and cycles, total bytes read and written, media and read/write error counters, PCIe link speed and SMART attributes, with manual refresh support. Serial numbers prefer the vendor identity reported by the protocol and are hidden by default.
- **System overview and performance charts**: Redesigned overview cards on the Processes page draw edge-to-edge curves that fade beneath the numbers on the left, mark the scale limit at the top and highlight the full card on hover, while adding CPU temperature, CPU power and GPU power readings. The CPU Overview page combines package and core power into a single "Power" reading (preferring package power, with the source shown on hover and both curves kept on the Details page); disk and network charts adjust their scales in fixed steps; compact sidebar preview charts draw secondary series as translucent solid lines for better legibility at small sizes.
- **Intel integrated graphics**: Expanded driver-reported temperature, clock, voltage and power readings, and added 10th Gen Core (Comet Lake S/H) integrated graphics temperature, clocks, shared memory clock, core VID, GT/media activity and power through kernel sensors (the IntelMCHBAR module) and RAPL PP1. Graphics slice clock gating is distinguished from active clocks, sub-watt power below 1 W is shown, and integrated graphics power is placed on the corresponding GPU page; valid RAPL power readings take precedence over zero values reported by Windows power counters. Values not provided by the hardware or driver remain unknown.
- **Window and layout**: Choose the default page opened on launch under Settings → Startup and tray; drag the right edge of the expanded side navigation to resize it (double-click to restore the default width); use the new "Show when a process is selected" details-pane mode to slide the pane out on selection and hide it when cleared; switching time ranges on the History page always shows the latest window up to the present; the minimum window size is now 1240×800 to keep the full layout intact, shrinking automatically when the primary display's work area is smaller.
- **About page, release notes and official mirror**: About is now a dedicated navigation page (Ctrl+8) with link cards for the Argus website and GitHub repository; a version badge on the left of the status bar opens the current version's release notes on click and copies the version on right-click; release notes are grouped by category with new features arranged in a two-column card grid; automatic updates try the official Argus mirror when GitHub cannot be reached before falling back to GitHub proxies, verifying installer and release-note signatures as before.

### Performance

- **Interface responsiveness and high-refresh animations**: Reduced main-thread overhead from sampling updates, text measurement and repeated drawing, reused static chart backgrounds and composition layers, parallelized large chart texture rasterization across threads, and substantially reduced chart GPU usage when Hardware-accelerated GPU scheduling (HAGS) is off; page transitions, the details pane, list scrolling and column reordering animate at the display refresh rate, while multi-core grids and GPU engine charts resize in the same frame as the window.
- **Page switching and background operation**: Services, Startup and Connections lists are preloaded in the background after launch so they appear immediately on first visit; interface preferences are coalesced and written asynchronously in the background to eliminate page-switch pauses; pages without live charts skip repeated rebuilds on each sample to lower CPU usage; drawing stops while the window is minimized or hidden in the tray, reclaiming chart layer caches, expired icons and texture atlases as needed to reduce background memory and GPU memory overhead.
- **Heavy load and low memory**: Tray updates are coalesced on a dedicated thread to improve responsiveness when the system or shell is busy; when memory is tight, charts automatically shrink buffers and lower texture resolution to keep live curves running, restoring full quality once memory recovers; activating the window handles large obstructing topmost windows more effectively.

### Fixes

- **Per-process network traffic**: Fixed readings staying at zero on some machines, and alternating zero readings or delayed updates at the 0.5-second refresh interval.
- **Privileged service**: Fixed unexpected service exits when clients disconnect before pipe attachment completes, and timeouts or false no-data results during frequent reads.
- **Application names and icons**: Fixed packaged (MSIX) apps showing executable file descriptions instead of the application names shown in Task Manager, and missing icons or file descriptions when device-path mappings are unavailable or processes deny access.
- **Per-process GPU memory**: Switched to resident GPU memory counters deduplicated per process and physical adapter to fix inflated or double-counted readings on some systems and drivers, and distinguished unavailable readings from zero usage.
- **Intel integrated graphics power display**: Fixed idle integrated graphics power repeatedly appearing and disappearing on the CPU Overview page.
- **CPU virtualization status**: Fixed Hyper-V hosts with hardware virtualization enabled still showing "Disabled".
- **Administrator startup authorization**: Fixed SID resolution failures when the user name matches the computer name, and improved detection of rebuilt copies of the same version.
- **Memory use over long sessions**: Deduplicated program icons by content, periodically evicted metadata cached for exited program paths, and bounded queued requests while metadata resolvers are busy, fixing steady memory growth when many short-lived programs run from new paths.
- **External links**: Opening external web links (website, repository, release page) now hands URLs directly to the system shell instead of spawning an intermediate Explorer process, removing browser launch delays.

### Upgrading

- **System requirements**: Windows 10 22H2 (build 19045) or later, including Windows 11, x64 only, with a DirectX 11 capable graphics device and driver. The application interface no longer requires the WebView2 Runtime.
- **Settings and history**: Application settings and history files are retained. Upgrades from v0.3.3 or v0.3.4 retain separately saved interface preferences. When upgrading directly from v0.3.2 or earlier, interface preferences stored only in the old WebView2 cache, such as layout, row notes and pins, return to their defaults and need to be set again. Argus automatically cleans up its old WebView2 data in the background.
- **Privileged service**: This version updates the service protocol to version 10. Upgrading with the installer or in-app updater also updates the installed service and Task Manager replacement copies. After replacing the portable executable, select "Update service" under Settings → Permissions → Privileged service and complete the administrator prompt.

## [0.3.4](https://github.com/Ray-lea/Argus/compare/v0.3.3...v0.3.4) - 2026-10-26

### Fixes

- **Windows Explorer restart**: Fixed the desktop and taskbar failing to recover when the old Explorer process gets stuck during exit cleanup or error reporting. If normal shutdown times out, Argus ends only the verified old instance and waits for it to exit before restoring the desktop. If Windows has already restored the desktop, Argus uses the existing Explorer instance instead of starting another. ([#20](https://github.com/Ray-lea/Argus/issues/20))

### Upgrading

- Upgrading from v0.3.x preserves existing settings and history.

## [0.3.3](https://github.com/Ray-lea/Argus/compare/v0.3.2...v0.3.3) - 2026-10-26

This release adds interface scaling and font size controls, preserves window and list preferences, and updates GPU usage reporting.

### Features

- **Interface scale**: Set a custom scale from 80% to 150% in Settings → Language and appearance. The default is 100%, and any whole-number percentage, such as 113%, is supported.
- **Font size**: Adjust the base font size from 11 to 20 px (default: 13 px). It combines with interface scaling, applies immediately and saves automatically. Each setting can be reset independently.

### Fixes

- **GPU statistics**: Attempt to fix low overall usage under some compute workloads, spikes when sampling resumes, and inflated totals across multiple GPUs or engines. Process engine counters and available NVIDIA GPU-core utilization supplement overall usage, with the source shown in the interface. ([#19](https://github.com/Ray-lea/Argus/issues/19))
- **Window restoration**: Restore window size, position and maximized state after quitting and reopening. Quitting while minimized no longer makes the next launch remain minimized, and off-screen windows return to an available display area.
- **List preferences**: Processes, Services, Startup and Connections remember their sort column and direction. Services and Connections also restore the selected filter.
- **Preference storage**: Interface preferences are saved separately, with automatic migration from the previous cache. Saved preferences remain available after clearing the interface cache.
- **Layout and window controls**: Toolbars and navigation adapt to narrow windows with larger text or higher scaling, and the maximize button's hit area follows the scale correctly.

### Upgrading

- Upgrading from v0.3.x preserves existing settings and history. Previous interface preferences migrate automatically on first launch, without manual reconfiguration.

## [0.3.2](https://github.com/Ray-lea/Argus/compare/v0.3.1...v0.3.2) - 2026-10-23

This release replaces the withdrawn v0.3.1. It fixes installed copies being incorrectly detected as portable when launched through administrator startup or Task Manager entry points, which prevented in-app updates.

### Upgrading

- If an installed copy says that the portable version cannot update automatically, download and run the installer from this release page. There is no need to uninstall Argus first. After upgrading, administrator startup and Task Manager entry points retain in-app update support.

### Fixes

- **Installed updates**: Correctly identifies administrator and Task Manager replacement copies, restoring update checking, downloading and installation.
- **Portable detection**: A nearby uninstaller no longer causes a portable copy to be detected as installed. Portable copies still receive updates through manual downloads from the release page.

### Included changes

- Includes v0.3.1's administrator startup after a one-time authorization, plus fixes for Windows Explorer restart, CPU usage statistics, charts after restoring the window, the process list caret and update installation.

## [0.3.1](https://github.com/Ray-lea/Argus/compare/v0.3.0...v0.3.1) - 2026-10-23

**This release was withdrawn. Its features and fixes are included in v0.3.2.**

This release adds administrator startup after a one-time authorization across sign-in, shortcuts and Task Manager entry points, and fixes Windows Explorer restart, CPU usage and update installation issues.

### Upgrading

- **Run as administrator**: Off by default. Enable it in Settings → Permissions or during first-run setup. This requires an account with administrator privileges. Argus restarts after enabling it, keeping settings and history. The privileged service, kernel sensors and Task Manager replacement still require separate authorization.
- **Installed version**: Upgrading through the installer or in-app updater also updates the installed privileged service and Task Manager replacement copies.
- **Portable version**: Quit the old version before replacing the executable. If the privileged service is installed, select Update service in Settings → Permissions in the new version. For subsequent manual updates with administrator startup enabled, turn it off and quit first, replace the file, then open the new executable and enable it again.

### Features

- **Administrator startup**: Authorize once to run Argus as administrator from sign-in, shortcuts and pinned icons, as well as Ctrl+Shift+Esc and the taskbar context menu when Task Manager replacement is enabled. Settings and first-run setup show the authorization and current process permissions, with options to disable or repair authorization. ([#13](https://github.com/Ray-lea/Argus/issues/13))

### Fixes

- **Windows Explorer**: Fixed restarts failing with standard or administrator permissions, desktop problems after a restart, and access errors on subsequent restarts. The file locks dialog now offers Restart for the current desktop shell, avoiding the loss of the desktop and taskbar caused by ending it directly. ([#14](https://github.com/Ray-lea/Argus/issues/14), [#15](https://github.com/Ray-lea/Argus/issues/15))
- **CPU statistics**: Fixed CPU usage staying at 100% when idle counters stop updating on some systems. ([#12](https://github.com/Ray-lea/Argus/issues/12))
- **Charts**: Fixed queued samples bunching into dense spikes when restoring the window from the taskbar or system tray.
- **Process list**: Fixed a blinking text caret appearing after clicking the list, while preserving type-to-search and Chinese input method support. ([#16](https://github.com/Ray-lea/Argus/issues/16))
- **Update installation**: Shows processes holding the program file open and supports retrying, records failure stages and error codes, and fixes unreadable update diagnostic logs after authorization with a different administrator account.

### Known issues

- **Code signing**: Release executables do not yet have Windows code signatures, so Windows SmartScreen may show an unknown publisher warning on first launch. In-app updates are still verified with update signatures.

## [0.3.0](https://github.com/Ray-lea/Argus/compare/v0.2.0...v0.3.0) - 2026-10-22

This release adds Run new task, Windows Explorer restart and copying PC specifications, improves process groups and performance views, and fixes Windows 10 compatibility issues.

### Breaking changes

- **History**: After upgrading, the History page starts with new recordings. The old file for the current day is replaced. To keep old recordings, [back up the history folder](https://github.com/Ray-lea/Argus/blob/main/docs/guide/README.md#历史) first.

### Upgrading

- **Installed version**: Upgrading through the installer or in-app updates also updates the privileged service and Task Manager replacement files.
- **Portable version**: Exit the old version before replacing the executable. If the privileged service is installed, click Update service in Settings → Permissions and approve the request.

### Features

- **Run new task**: A new toolbar button opens programs, folders, documents or URLs, with an option to run as administrator.
- **Restart Windows Explorer**: Right-click Windows Explorer on the Processes page and choose Restart.
- **Keyboard shortcuts**: Hold Ctrl to pause refresh and release it to resume. On the Processes page, Alt+E ends the selected process. Settings now includes a shortcut reference.
- **Process groups**: The Apps view has Apps, Background processes and Windows processes sections, with collapsible headings and resource totals.
- **Quick search**: Start typing on the Processes page to search, including with a Chinese input method. Active filters are highlighted in the accent color.
- **PC specifications**: The details pane stays open and can be resized. With no process selected, it shows hardware specifications that can be hidden or copied as text.
- **CPU page**: Overview and Details views organize the information. Processor charts rearrange smoothly as the window changes size.
- **GPU page**: Active engines appear automatically. Engines, chart layout and chart height can also be selected manually.
- **Memory composition**: Adds modified memory. Hover over each legend icon for an explanation.
- **Battery history**: The History page adds battery charge and charge or discharge power alongside resource usage.
- **Pins and notes**: Right-click entries on the Services, Startup and Connections pages to pin them or add notes.
- **Window animations**: Minimizing, restoring and showing or hiding from the tray use an in-place fade.
- **Release notes**: Settings and the update dialog share the same layout, support English notes and use the available window height.
- **Diagnostic logs**: Include the failed step, error codes, relevant paths and CPU and GPU models to help with bug reports.
- **About**: Adds a link to the project homepage.

### Fixes

- **CPU usage**: Fixes low readings for overall, per-processor and per-process CPU usage during brief workloads.
- **Windows 10**: Fixes desktop wallpaper showing through Argus, blank Windows Settings windows and missing per-process disk read and write data.
- **Task Manager replacement**: Fixes setup failures and incorrect messages about Argus starting or exiting in some environments. Adds guidance for security software blocks.
- **Kernel sensors**: Fixes detection and upgrades of older PawnIO versions, and enabled status being shown when the driver is unavailable or faulty.
- **Startup apps**: Adds packaged apps and their enable and disable controls, and corrects names and status.
- **Startup**: Fixes Argus failing to open when another administrator account authorizes elevation or Windows Explorer has exited.
- **Process identification**: Fixes WebView2 app attribution, unrelated programs with the same name being grouped together and incorrect groups while sorting is paused.
- **Chinese input**: Fixes the first character being lost after returning to the Processes page.
- **Charts**: Fixes the left edge being progressively cut off, hover dot stutter and charts jumping while paused. Smooths changes to the chart scale.
- **Page layout**: Fixes pages and dialogs shifting when scrollbars appear.
- **History storage**: Fixes existing data being lost when reading the current day's recording fails.
- **Security**: Updates underlying components and tightens restrictions on loading external content in the embedded page.

### Performance

- Improves process list scrolling and reduces repeated background refreshes and their resource usage.

### Known issues

- **Code signing**: A production certificate is being arranged. Windows SmartScreen may show an “unknown publisher” warning on first launch.

Thanks to @kostyanp95 for contributing battery history support.

## [0.2.0](https://github.com/Ray-lea/Argus/compare/v0.1.0...v0.2.0) - 2026-10-18

This release adds disk read and write rates for each process and separate refresh rates for when the window is minimized and when it is in the tray, improves in-app updates and shortcut settings, and fixes several problems, including a crash after running for a while on some PCs. If the privileged service is installed, read "Breaking changes" first.

### Breaking changes

- **The privileged service needs an update**: This release changes how Argus talks to the privileged service, and the 0.1.0 service does not work with the new version. Nothing changes if the privileged service is not installed.
- **Upgrading with the installer**: This covers both in-app updates and running the installer yourself. The installer first closes an old Argus that Task Manager replacement started and that is still running, and updates the privileged service with the same administrator approval, so nothing else needs to be done. If Replace Task Manager is on, it stays on, and after the update Ctrl+Shift+Esc opens the new version.
- **Upgrading the portable version**: If the privileged service is installed, update the service by hand after replacing the program: click Update service in the new Argus's notice or under Settings → Permissions, and approve. Do the same if the installer fails to update the service. Until the service is updated, network, disk read/write and CPU sensor data that rely on it are unavailable, and management actions that need administrator rights may fail. With Replace Task Manager on, Ctrl+Shift+Esc may still open the old version; updating the service updates the replacement too and keeps it on.

### Features

- **Disk read and write for each process**: The Processes page has two new columns, Disk read and Disk write, showing how much data each process reads from and writes to disk per second. Both can be sorted, show the group total when processes are grouped by app, and also appear in the details panel. The I/O column also counts network and device traffic and is kept separate from them. Windows 11 24H2 and later need no extra permissions; earlier versions need the privileged service or Argus running as administrator, and the privileged service's approval now lists this. While there is no reading yet the columns show "—", and hovering explains why. In a narrow window these two columns always stay, the table scrolls sideways and the name column stays on the left; the Threads, Handles, PID and I/O columns are therefore hidden earlier than before.
- **In-app updates**: From this release, once a new version has been downloaded and has passed the signature check, an update button appears on the right of the title bar, the tray icon shows a dot, and a notice appears once in the lower-right corner, once per version. The portable version does not download updates and tells you as soon as it finds one. The update dialog lists the changes of every version between the current one and the new one, by category, with the number of changes in each; the notes are signature-checked just like the installer. Right-click the update button to choose "Don't remind me about this version". A downloaded update does not need to be downloaded again after restarting Argus.
- **What's new after updating**: The first time the window opens after an update, it shows what changed in the new version. To turn this off, use "Show what's new after updating" under Settings → Updates; you then only get a notice in the lower-right corner. "See this version's changes" on the same page opens the notes at any time.
- **Background refresh rates**: The refresh rate while minimized and in the tray can be set separately, from 0.5 to 30 seconds (5 seconds by default), under Settings → Data and history and in the first-run guide. Below 5 seconds, Argus first explains that CPU use and power draw go up; above 5 seconds, history is recorded more sparsely too. Argus refreshes right away when the window comes back to the front. If the background rate is slower than the foreground one, a notice appears once in the lower-right corner, with a shortcut to the setting and an option not to show it again.
- **Shortcut settings**: A shortcut can be turned off while keeping its keys, and turning it back on restores it. While recording, the keys being held are shown live, combinations that cannot be used say why, and "Restore default" is available. Combinations that easily clash with other programs, such as Ctrl+letter or Alt+letter, are saved only after you confirm. If another program holds the combination, you can retry right away; once that program exits, Argus also retries the next time the window is shown.
- **CPU up time**: Shown to the second, for example "5d 14:03:07", and updated with every refresh.

### Fixes

- **Crash**: On some PCs without a usable Intel graphics adapter, Argus could crash after running for a while. This is fixed.
- **Lost settings**: A power loss or forced restart shortly after saving settings could lose them, so that the first-run guide appeared again and the language went back to the system default. This is fixed.
- **Starting with Windows**: Argus no longer takes the foreground when it starts with Windows. Before, keystrokes went to the invisible Argus window until you clicked somewhere else. While hidden in the tray, the interface no longer keeps drawing in the background: its CPU and GPU use drop sharply and its memory use falls by more than half.
- **Startup entry**: Turning on "Start when signing in to Windows" in an Argus opened with Ctrl+Shift+Esc now points the startup entry at the installed Argus. An entry that already points at the copy used by Task Manager replacement is corrected after the new Argus is opened once from the Start menu or elsewhere.
- **Background refresh**: The refresh rate while minimized or in the tray now matches the setting. Before, a 5-second rate was often 6 seconds and sometimes 10. When running as a standard user with the privileged service, the network column on the Processes page no longer briefly disappears after restoring the window from the tray or from minimized.
- **Window**: When maximized, two overlapping sets of buttons no longer appear in the top-right corner, and the window can be dragged with the pointer at the very top of the screen.
- **Installer and uninstaller**: The Close button on the finish page, the Cancel button on the uninstall confirmation page and the row with the check box are now centered; they used to sit slightly to the left.
- **Upgrading with the installer**: Running the installer to upgrade also closes an old Argus that is still running after being opened by Ctrl+Shift+Esc or an old startup entry, so that starts after the upgrade are no longer handed to the old version. There is no need to exit from the tray first.
- **Uninstalling**: Uninstalling also closes an Argus that is still running after being opened by Ctrl+Shift+Esc. Before, it stayed in the background: after uninstalling with "Also delete settings, history and logs" and reinstalling, the first start skipped the first-run guide and the old settings were written back. If not all data could be deleted, the uninstaller lists the folders that are left.
- **First-run guide**: Holding Ctrl+Shift+Esc to open Argus no longer closes the guide with the held Esc and marks it as done.
- **Recording shortcuts**: Pressing the current combination or holding a key a little longer while recording no longer hides the window by accident. With keyboard layouts other than US English, the recorded keys match the ones that trigger the shortcut. Shift+Backspace and Win+Backspace no longer clear the shortcut by mistake.
- **Shortcut combinations**: New combinations must include Ctrl, Alt or Win, except for a few keys such as F13–F24. Combinations that get in the way of typing, such as Shift+letter, and common editing and system actions such as Ctrl+C and Alt+F4, are no longer accepted. Saved combinations are not affected.
- **Shortcuts**: Some numeric keypad keys are no longer registered as other keys. If saving settings fails while changing a shortcut, no extra shortcut that cannot be removed is left behind. Reasons for a failed registration are shown in the interface language. In the first-run guide, a shortcut that is taken or cannot be used only shows the reason and notes that it can be changed in Settings.
- **Replace Task Manager**: Updating the privileged service keeps the replacement on and updates the Argus it uses to the new version, instead of restoring Windows Task Manager first and needing it turned back on after the update. If turning it on fails, Argus says which step was refused and why that might be, for example security software blocking a registry change. When the status reads "Configured by something else", it lists the settings involved. After the replacement is turned off, other tools such as Process Explorer taking over Task Manager no longer get in the way of updating or uninstalling the privileged service. A failed attempt no longer leaves an extra copy of Argus in the service folder; copies left by earlier versions are cleaned up when upgrading, turning the replacement on or off, or updating or uninstalling the privileged service, unless they are running.
- **Processes**: Clicking "Show details" with nothing selected now asks you to select an item in the list first; before, nothing happened. While a context menu or confirmation is open, the status bar no longer shows "Sorting paused while the pointer is over the list", and the list no longer re-sorts as the confirmation appears. Switching pages with a shortcut while the pointer is over the list no longer leaves that message in the status bar of other pages.
- **Charts**: The hover dot no longer stutters with the pointer near the right edge of a chart.
- **Switching the update channel**: Switching between Stable and Beta while an update is being checked or downloaded now checks again on the new channel instead of offering the old channel's version.
- **Diagnostic log**: Failures are logged with the system error code instead of a generic "operation failed", which used to be nearly all of them on non-English systems. At startup, the log also records why the privileged service could not be reached, for example an incompatible version or a timeout.

### Performance

- **Processes page**: The small per-processor usage bars in the system overview cost less to draw. While charts move, the interface sends about a third less data to the graphics card each frame; most of WebView2's reads and writes in the I/O column come from this data, so they drop too.

### Known issues

- **Disk read and write**: The method used on Windows 11 24H2 and later has only been verified on build 26300; earlier versions read through the privileged service or administrator rights, which has not been verified on them yet. If both columns always show "—", please report it with the diagnostic log attached; the log records why reading failed.
- **Code signing**: The program is not code-signed yet, so Windows SmartScreen may warn about an "unknown publisher" the first time it runs.
- **Other**: For other known issues, see the [known issues](https://github.com/Ray-lea/Argus/blob/main/docs/known-issues.md) page on GitHub (in Chinese).

## 0.1.0 - 2026-10-16

The first public release of Argus.

### Features

- **Processes**: View by app, as a process tree or as a list; when sorted by name, the list is split into Apps and Background processes. Each process shows CPU, memory, I/O and GPU use, plus network traffic with the privileged service or when running as administrator. Search, end a process or a whole process tree, and see the command line, user, integrity level and more in the details panel, with totals for a whole app when grouped by app.
- **Priority and efficiency mode**: Set from the context menu, or as rules that apply every time the program starts.
- **Performance**: CPU (per core, performance and efficiency cores, frequency, temperature, power and idle states), memory, disks, network, GPUs (with power draw in watts for NVIDIA, AMD and Intel graphics) and batteries. Charts flow with the display's refresh rate and scale with the window, and less-used details sit in expandable cards.
- **Kernel sensors (optional)**: Once turned on in Settings or the first-run guide, Argus reads the CPU's internal sensors through the privileged service and the official PawnIO driver, including per-core temperature and voltage, power and limit status, AMD SMU data, and fan speeds, voltages and temperatures on supported motherboards.
- **Services and startup apps**: Start and stop services and change their startup type; enable or disable startup apps with the same switch as Windows Task Manager.
- **Connections and ports**: TCP and UDP endpoints with the processes that own them, an exact search by port, and closing IPv4 TCP connections.
- **File locks**: Drop a file or folder on the window to find the processes using it, through the Restart Manager and handle enumeration, including processes whose current folder it is.
- **History**: The last 7 days of resource use are recorded; click any moment to see the processes that used the most at the time.
- **Tray**: Show or hide the window with the global shortcut Ctrl+Alt+Esc; while hidden, Argus uses very little.
- **Optional privileged service**: After a one-time administrator approval, the interface can run as a standard user and still manage system processes, apply rules automatically, track network traffic per process, control services and close connections.
- **Task Manager replacement**: Ctrl+Shift+Esc and the other ways to open Task Manager can open Argus, with a standalone recovery tool that relies on neither the interface nor the service.
- **In-app updates**: Stable and Beta channels and an optional GitHub download mirror; only versions that pass the signature check are installed.
- **Appearance**: The toolbar doubles as the title bar, and the navigation sits under the toolbar or on the left of the window; Mica background and the system accent color, with light and dark themes that follow Windows by default or can be chosen in Settings.
- **Install and uninstall**: A single-page installer that follows Windows' light or dark mode; an installed older version is upgraded in place, and uninstalling can also delete settings, history and logs.
- **Languages and first-run guide**: The interface, tray and installer come in Simplified Chinese and English and follow the Windows display language by default. On first start, a guide walks through language, display and refresh, the tray, updates and other common settings, and can install the optional privileged service, turn on kernel sensors or replace Task Manager right away.

### Known issues

- The program is not code-signed yet, so Windows SmartScreen may warn about an "unknown publisher" the first time it runs.
- The CPU's internal sensors need kernel sensors (the privileged service and the PawnIO driver); for supported processors and motherboards, see [hardware data](https://github.com/Ray-lea/Argus/blob/main/docs/guide/hardware-data.md#消费级硬件兼容范围) (in Chinese).
- For other known issues, see [known issues](https://github.com/Ray-lea/Argus/blob/main/docs/known-issues.md) (in Chinese).
