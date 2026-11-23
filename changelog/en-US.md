# Changelog

[简体中文](zh-CN.md) · [繁體中文](zh-TW.md) · **English** · [Español](es-ES.md) · [Deutsch](de-DE.md) · [日本語](ja-JP.md) · [Français](fr-FR.md) · [Русский](ru-RU.md)

User-visible changes in each Argus release. Versions follow [Semantic Versioning](https://semver.org/). This file is translated from the [Simplified Chinese changelog](zh-CN.md), entry for entry.

## [0.6.0](https://github.com/Ray-lea/Argus/compare/v0.5.0...v0.6.0) - 2026-11-22

Argus v0.6.0 shows live readings in the tray, on the desktop and on the taskbar. The redesigned History page charts multiple readings, highlights resource events and tracks a program across runs. The process list flags resource leaks, and four lists support CSV export. This release also reduces the cost of scrolling and fixes a blue screen when opening disk details on some Intel RST VMD systems.

### Breaking changes

- **History**: History shows only data recorded with version 0.6.0 or later. The first write after upgrading replaces the old-format file for that day, if present. Earlier versions cannot read the new records.
- **Privileged service protocol**: The privileged service protocol changes from 11 to 12. Version 0.6.0 requires the updated service for operations it delegates, such as ending a process or changing I/O or memory priority.

### Features

- **Components page**: “Components” contains the tray and background settings and the switches for both monitors. The desktop and taskbar monitors start turned off. Opening settings from the tray or a monitor selects its section in the sidebar.
- **Live tray icon**: The tray shows CPU usage with “Trend” by default and also offers “Share”, “Number” and “App icon”. The three live styles use amber at 80% usage. Hover for live readings or right-click for the three busiest processes by CPU usage and common actions.
- **Tray card and quick panel**: The hover card fits labels to the language and font size, with a system tooltip available in its place. The quick panel links directly to each device. Opening it from the hidden-icons area keeps that area expanded. ([@digital-bombyx](https://github.com/digital-bombyx) [#113](https://github.com/Ray-lea/Argus/pull/113) [#153](https://github.com/Ray-lea/Argus/pull/153))
- **Desktop monitor**: The desktop monitor shows readings in a “Strip” or a panel with “List”, “Curves” or “Gauges”. Dragging keeps the window within the current display’s work area. Its settings control pinning, position locking, click-through and opacity.
- **Taskbar monitor**: The taskbar monitor offers “Labels”, “Curves”, “Capsules” and “Large figures”. Preview and switch styles in Components or in the right-click panel. The panel also selects the display and updates readings as they are checked or reordered. ([#28](https://github.com/Ray-lea/Argus/issues/28), [#123](https://github.com/Ray-lea/Argus/issues/123))
- **Displays and windows**: If the selected display disconnects, the taskbar monitor moves to the primary display until it reconnects. Clicking the taskbar monitor or tray icon hides an unobstructed window or brings a hidden, minimized or covered window to the front. Double-clicking the desktop monitor opens the main window.
- **Monitor readings**: Both monitors offer 18 types of readings, including temperature, clock speed, power and fan speed. Select readings for each GPU, disk or network adapter and drag groups or individual readings to reorder them. Taskbar labels use device colors and identify successive readings for the same CPU or GPU by metric.
- **Monitor refresh interval**: When either monitor is on, Argus samples at the same rate as when the main window is visible. If the minimized or background interval differs, settings show the actual interval and let you select the interval to use after both monitors close.
- **Recorded history**: History now records CPU clock speed, power and temperature, memory details, readings for each disk, network adapter and GPU, and battery readings. Under “Data and history” in Settings, select reading groups and record about 20, about 40 or all active processes per frame. An estimate shows how much space a day will use.
- **History storage**: Age and size limits apply independently, whichever comes first. The defaults are 30 days and 2 GB, with records older than 7 days merged into minute averages, peaks and the busiest processes. Changing folders offers the option to move existing history and watch records. If the selected folder already holds other files, Argus uses an “Argus History” subfolder in it.
- **History page**: Each lane shows a step chart and can overlay a second reading. Search for readings by name or device and drag lanes to reorder or resize them. Scroll within the chart when the lanes exceed its height.
- **Navigating history charts**: Use the wheel to zoom around the time under the pointer. Zooming, clicking a curve or dragging a selection pauses live tracking, which resumes with “Back to latest”, a right-click or End. Hatching marks gaps, and readings from the last 10 minutes show their actual sampling interval. ([#79](https://github.com/Ray-lea/Argus/issues/79), [#129](https://github.com/Ray-lea/Argus/issues/129))
- **Live history charts**: During live tracking the chart moves beneath the pointer, and the active lane shows the reading at that position. Time and axis ranges change smoothly, and overlays fade in or out. Previously viewed long ranges reopen faster.
- **Time ranges and events**: The time range list has 5 minutes, 3 hours, 1 day, 3 days and "Custom…". In the "Custom time range" dialog, select dates in a calendar or type an exact start and end time. Select "Show the 30-day overview and events" to show an hourly heat map and events such as high CPU load, low memory and gaps in the records.
- **History process table**: Search the processes for a moment or time range and sort by name to group apps, background processes and Windows processes. Expand a name to see the top four instances and a total for the rest. The context menu can overlay the program’s curve or copy its name and PIDs.
- **History chart tooltips**: For CPU and other resources measured per process, the tooltip lists the five busiest processes. The process-count lane shows the five programs with the most instances and totals for the rest. Temperature and battery-level tooltips show device readings only.
- **Process observation**: “Watch process” in the context menu or details pane records 14 readings per run of the same executable, covering memory, CPU, GPU, disk, I/O, network, threads and handles. If its path is unavailable, Argus tracks only the current instance. Starting a watch opens “Process observation” in History, where you can select a run and compare its readings with the same chart controls as in system history. ([@hayxe1](https://github.com/hayxe1) [#82](https://github.com/Ray-lea/Argus/pull/82))
- **Watch records**: The last 10 minutes show individual samples, and older periods use disk records. Watch data lasts up to 7 days, with the oldest entries removed sooner if the storage limit is reached. Stopping a watch keeps its data, and watches must be started again after Argus restarts.
- **Title bar**: The title bar contains the logo, a search box that adjusts to the window width and standard-size window buttons. Drag empty space to move the window. The pin toggles “Always on top” and remembers the choice after restart. ([#121](https://github.com/Ray-lea/Argus/issues/121))
- **Page bar and status bar**: Page controls and “More actions” now sit beside the navigation tabs, with icons for switching process views. The menu includes “Run new task…” (Ctrl+N), “Find processes using a file…”, keyboard shortcuts and, on the Services page, the Windows Services console. The status bar has buttons to pause updates and to select the refresh rate, and the version number also shows update progress and the install prompt. ([#87](https://github.com/Ray-lea/Argus/issues/87))
- **Process groups**: Apps, background processes, Windows processes and each user group have their own color. While you scroll, the title of the current group stays at the top, and marks next to the scroll bar show the position of each group and the name of the current group. With "Hide system processes" on, the process count also shows how many processes are hidden. ([#88](https://github.com/Ray-lea/Argus/issues/88))
- **Process power usage**: “Power usage” and “Power usage trend” estimate consumption from CPU, GPU and disk activity. They show the current level and the average over roughly two minutes. Both columns are hidden by default. ([#134](https://github.com/Ray-lea/Argus/issues/134))
- **Process priorities**: Set a process’s I/O and memory priorities from its context menu and view them in the details pane or optional columns. “Base priority” shows class names, keeps special values numeric and sorts by value. ([@digital-bombyx](https://github.com/digital-bombyx) [#80](https://github.com/Ray-lea/Argus/pull/80) [#116](https://github.com/Ray-lea/Argus/pull/116))
- **Pause sorting while hovering**: When the pointer on the process list pauses sorting, the header of the sort column shows a pause icon and the status bar shows "Sorting paused". The new "Pause sorting while hovering" option in "Settings → Language and appearance" is on by default. ([#86](https://github.com/Ray-lea/Argus/issues/86))
- **Resource leak alerts**: The process list flags handle leaks, accumulating child processes and leftover processes. Click the amber header mark to locate an affected process and see the evidence, trend and suggested action in its details pane. Flags refresh after the process exits, and “Settings → Language and appearance → Resource leak alerts” can restore individually ignored programs or turn off alerts.
- **Copying table data**: The context menus of the Processes, Services, Startup and Connections pages can copy the value in a column, the name and the details. On the Processes page, Ctrl+C copies the name and Ctrl+Shift+C copies the details. If text is selected, Ctrl+C copies the selection. ([@digital-bombyx](https://github.com/digital-bombyx) [#161](https://github.com/Ray-lea/Argus/pull/161), [#91](https://github.com/Ray-lea/Argus/issues/91), [#150](https://github.com/Ray-lea/Argus/issues/150))
- **CSV export**: Choose “Export current page as CSV…” from “More actions” on Processes, Services, Startup or Connections. The file includes rows outside the viewport and preserves column order, filters, sorting, units and expanded groups. ([#139](https://github.com/Ray-lea/Argus/issues/139))
- **Details pane**: The resources section adds peak working set, virtual address space, I/O rates and power usage. “Totals” lists user and kernel CPU time, bytes by I/O type and page faults. The copy button at the top right copies available values by section.
- **Connections page**: Connections can be grouped by app, and a search automatically expands matching groups. The status filters for services and connections are in the “Status” column header. Connections can also be filtered by TCP, TCPv6, UDP or UDPv6, independently of status. ([@digital-bombyx](https://github.com/digital-bombyx) [#152](https://github.com/Ray-lea/Argus/pull/152))
- **Services and startup apps**: Choose “Delete” in the context menu and confirm to remove a service or startup entry. Windows removes a service in use once it stops and its handles close. Startup folder files go to the Recycle Bin, and packaged app startup tasks can only be disabled. ([#114](https://github.com/Ray-lea/Argus/issues/114), [#146](https://github.com/Ray-lea/Argus/issues/146))
- **Packaged app services**: If Windows denies a startup-type change for a packaged app service, a confirmation lets Argus write the setting to the registry. The list marks changes that take effect after a restart. Errors identify the service and explain permission or protection restrictions. ([@digital-bombyx](https://github.com/digital-bombyx) [#112](https://github.com/Ray-lea/Argus/pull/112), [#168](https://github.com/Ray-lea/Argus/issues/168))
- **Memory readings**: Hatching in the memory composition bar marks compressed memory in use. Hover to see how much data it is estimated to hold and how much memory it saves. Memory tooltips on the Processes page explain system usage, private working sets and group totals. ([@digital-bombyx](https://github.com/digital-bombyx) [#81](https://github.com/Ray-lea/Argus/pull/81), [#122](https://github.com/Ray-lea/Argus/issues/122))
- **NPU**: Performance shows the name, utilization and a 60-second chart for each NPU that Windows identifies. These devices have their own entries outside the GPU list. ([@SpookySandwich](https://github.com/SpookySandwich) [#140](https://github.com/Ray-lea/Argus/pull/140), [#135](https://github.com/Ray-lea/Argus/issues/135), [#148](https://github.com/Ray-lea/Argus/issues/148))
- **CPU charts and disk details**: The overall and logical processor views and the core type filter are now in the context menu of the CPU chart. The small charts of the logical processor view keep a readable shape and, when space is low, drop first the clock speed and then the title. Disk details show SMART raw values in decimal by default and remember the choice between decimal and hexadecimal for all disks across restarts.
- **Hardware acceleration**: The new "Hardware acceleration" option in "Settings → Animations" is on by default. Turning it off switches interface rendering to the CPU immediately. ([#127](https://github.com/Ray-lea/Argus/issues/127))
- **Touch**: Argus supports tap, double-tap, swipe to scroll, press and hold for menus, and dragging the scroll bar on Windows touch screens. Forwarded touch taps work on main-window controls, and focused text boxes request the touch keyboard when needed. ([#73](https://github.com/Ray-lea/Argus/issues/73))
- **App icon**: The taskbar, title bar and updated shortcuts use the new dot-matrix chart icon at the appropriate size. In About and the welcome guide, the dots form the logo, then show the last ten CPU samples. With interface animations off, the readings appear immediately.
- **About page**: The new "Copy diagnostic information" command copies the version, run state, hardware and display settings for an issue report. New buttons on the website and GitHub cards open the download, user guide, changelog, issue, discussion and release pages.
- **Location of administrator programs**: The privileged service, the Task Manager replacement and the administrator copies are now in `Program Files\Argus Privileged` with fixed file names, so trust that security software gives by path stays valid after an update. When the install location is not in Program Files, an information icon on the install page tells where these files are. ([@digital-bombyx](https://github.com/digital-bombyx) [#154](https://github.com/Ray-lea/Argus/pull/154), [#136](https://github.com/Ray-lea/Argus/issues/136), [#159](https://github.com/Ray-lea/Argus/issues/159))

### Performance

- **Drawing**: Updates, hover effects and clicks redraw only the affected areas, reducing GPU work during hover and icon animations. In release-build measurements, scrolling the process list used about 40% fewer CPU cycles per frame. With hardware acceleration off, scrolling and page changes reuse content already drawn.
- **First readings**: Argus shows the first readings sooner after it starts. ([@hayxe1](https://github.com/hayxe1) [#119](https://github.com/Ray-lea/Argus/pull/119))

### Fixes

- **Blue screen from disk details**: Argus checks the controller driver before querying temperature or NVMe health, avoiding a blue screen on affected Intel RST VMD systems. Skipped readings appear as unknown. If the session ends abnormally during the read or within two minutes afterward, automatic reads pause on the next start and disk details offers “Read anyway”. ([#48](https://github.com/Ray-lea/Argus/issues/48), [#133](https://github.com/Ray-lea/Argus/issues/133), [#160](https://github.com/Ray-lea/Argus/issues/160))
- **Device sleep**: On hybrid-graphics laptops, Argus skips sensor queries to a powered-off discrete GPU and shows “Sleeping” with an explanation on the Performance page. Idle hard disks keep their last temperature reading, and Argus skips the query once they spin down. ([#147](https://github.com/Ray-lea/Argus/issues/147))
- **CPU readings**: CPU usage now accounts for idle cycles, correcting inflated readings that counted the idle loop as work. Corrected missing clock speeds of hybrid processors on Windows 10, and some Intel processors on Windows 10 that showed no difference between performance and efficiency cores. ([#137](https://github.com/Ray-lea/Argus/issues/137))
- **GPU readings**: Processes on an integrated GPU no longer count shared GPU memory as dedicated GPU memory. On computers without a GPU, software rendering no longer counts as GPU usage of a process, and the Processes page hides the GPU columns. If Argus detected a hardware GPU at startup, it can refresh readings after a driver update or detect an additional GPU without restarting.
- **Disk readings**: Corrected USB external hard disks that gave no SMART information or showed "Unknown" health, and drive letters of virtual drives such as Google Drive that showed under disk 0.
- **File locks**: The search for processes that use a file no longer fails when the system has very many handles. When the privileged service and the local search fail for the same reason, Argus shows the message only once. When the privileged service is installed, Argus no longer recommends that you turn it on.
- **Protection when ending processes**: When Argus cannot confirm whether a process is critical to the system, it does not end the process. This applies to single processes, multiple processes and process trees. ([@hayxe1](https://github.com/hayxe1) [#77](https://github.com/Ray-lea/Argus/pull/77))
- **Processes page**: Searching or collapsing groups keeps column widths stable, and expanding child processes preserves order while hover has paused sorting. User groups now open their context menu, and double-clicking a selected row keeps its details open. Stray characters no longer appear in program names and version information.
- **Historical processes**: When opening a process from History, Argus checks its PID, name and start time to avoid selecting a different process that reused the PID. ([@hayxe1](https://github.com/hayxe1) [#76](https://github.com/Ray-lea/Argus/pull/76))
- **Service and startup lists**: Service and startup columns size to their contents and retain manual widths after restart. Double-click a column border to restore automatic sizing. An unreadable startup state appears as “Unavailable”.
- **Privileged service and administrator mode**: Argus now recognizes protected system folders correctly when enabling administrator mode or the privileged service. Missing service files show “Damaged” with repair or uninstall options. A power loss during maintenance preserves the files needed by a registered service, and uninstalling Argus clears files left after deregistration. ([#111](https://github.com/Ray-lea/Argus/issues/111))
- **Task Manager replacement**: When security software or another program reverts the replacement, the Settings page and the welcome guide show that it was reverted. When the maintenance helper is ended, the message gives its exit code.
- **Install and load security**: Outside Program Files, the installer restricts writes to administrators and the system in folders it can protect. During a new interactive install, it explains an unsuitable location and offers to continue or choose another. The program loads the system DLLs that it imports only from the system folder, and ignores files with the same name in its own folder. ([#136](https://github.com/Ray-lea/Argus/issues/136))
- **Window**: Corrected a missing system animation when the window is maximized or restored, a window that did not stay maximized when it opened again from the tray, and a window that other windows covered at the first start or when it opened from a Task Manager shortcut. ([#126](https://github.com/Ray-lea/Argus/issues/126), [#142](https://github.com/Ray-lea/Argus/issues/142))
- **Scrolling**: Corrected pages that did not scroll up again after they reached the bottom, and dialogs that did not scroll, at system or interface scale levels such as 125% or 150%. Scale fields now fit three digits, and column options and hints no longer clip the bottom of letters. ([#164](https://github.com/Ray-lea/Argus/issues/164))
- **Menus and dialogs**: Context menus and submenus close immediately, and drop-down lists expand from and retract toward their controls. Dialogs open at the center with a zoom and fade, dim the window button backgrounds and shrink as they close. Tooltips stay clear of context menus, and disabled entries no longer show warning or hover colors. ([#141](https://github.com/Ray-lea/Argus/issues/141))
- **Layout**: The overview on the Performance page now fits the default window size. The Settings page uses more width in wide windows. Dialogs no longer show a scroll bar when the content fits.
- **Screen readers**: Screen readers can read page headings, descriptions, performance readings and table cells. Controls remain accessible after a page change or refresh. Switches and number fields use their setting titles as accessible names. ([#115](https://github.com/Ray-lea/Argus/issues/115))
- **Interface text**: Shorter descriptions remove repeated text, and all languages use consistent CPU, GPU and I/O terms and hardware units. Russian messages also have revised error text, grammar and unit spacing. ([@Dimmitrius](https://github.com/Dimmitrius) [#93](https://github.com/Ray-lea/Argus/pull/93) [#95](https://github.com/Ray-lea/Argus/pull/95) [#96](https://github.com/Ray-lea/Argus/pull/96) [#98](https://github.com/Ray-lea/Argus/pull/98) [#100](https://github.com/Ray-lea/Argus/pull/100) [#101](https://github.com/Ray-lea/Argus/pull/101) [#102](https://github.com/Ray-lea/Argus/pull/102) [#103](https://github.com/Ray-lea/Argus/pull/103) [#104](https://github.com/Ray-lea/Argus/pull/104) [#105](https://github.com/Ray-lea/Argus/pull/105) [#106](https://github.com/Ray-lea/Argus/pull/106) [#108](https://github.com/Ray-lea/Argus/pull/108) [#109](https://github.com/Ray-lea/Argus/pull/109))

### Upgrading

- **Privileged service**: The installer and in-app update also update the installed service and the Task Manager replacement copy. After replacing a portable executable, choose “Update service” under “Settings → Permissions → Privileged service” and approve the administrator prompt.
- **Location of administrator programs**: After you upgrade the installed version, or update the service of the portable version, the existing privileged service, Task Manager replacement and administrator copies move to `Program Files\Argus Privileged`. Trust that security software gives by the old path must be added again once for the new path.
- **Page shortcuts**: With the new Components page, Ctrl+7 opens Components, Settings moves to Ctrl+8 and About moves to Ctrl+9.
- **Settings and history**: System history opens to the last 5 minutes after upgrading, then remembers subsequent choices. A saved UDP status filter resets to all connections and can be set again in the Protocol column. Settings now store only nondefault values, so untouched options follow future defaults.

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
