# Changelog

[简体中文](CHANGELOG.md) | English

User-visible changes in each Argus release. Versions follow [Semantic Versioning](https://semver.org/). This is the English edition of [CHANGELOG.md](CHANGELOG.md): `pnpm release` adds each release to both files, and the English entry is translated from the Chinese one before the release is published. The release notes on GitHub and in the app come from these entries.

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
- **Startup entry**: Turning on "Start when signing in to Windows" in an Argus opened with Ctrl+Shift+Esc now points the startup entry at the installed Argus rather than the copy Task Manager replacement uses. An entry that already points at the copy is corrected after the new Argus is opened once from the Start menu or elsewhere.
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
