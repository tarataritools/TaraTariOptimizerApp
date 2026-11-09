# TaraTari Optimizer

A new, native desktop utility written in Rust. This project is an independent app, not a fork or port of Kudu. Its Windows-first interface is built with egui and follows TaraTariTools' dark surfaces and green accent.

## Available in this build

Version 0.0.1 provides a **native rounded-panel UI** and these working features:

- **Hardware overview:** the My Specs page shows hardware names and available sensor readings from LibreHardwareMonitor.
- **LibreHardwareMonitor:** the app builds a self-contained .NET 8 bridge against upstream `LibreHardwareMonitorLib` 0.9.7-pre782 (MPL-2.0) and presents detected hardware plus available sensor readings. The bridge executable is published beside the app in `hardware-bridge\TaraTariHardwareBridge.exe`; it includes its .NET runtime and does not require the user to install .NET. A .NET SDK is needed only to build the app. Detection depends on firmware, device, and driver support and cannot promise every component or value.
- **Who's fat? storage analyzer:** scans a selected local drive read-only, totals readable file sizes, and ranks its largest top-level folders and individual files in a proportional treemap with an analysis icon and progress indicator. The scan reports inaccessible entries and its safety limit; filesystem allocation, compression, sparse files, and metadata mean file totals may differ from used disk space.
- **Cache Cleaner:** scans the current Windows temporary folder and `%WINDIR%\Temp`, automatically selects regular files older than 24 hours, and shows the candidate size. Under 100 MB it recommends leaving the cache alone; otherwise one explicit **Clean cache** click removes the listed files and reports the bytes actually removed. Directories and saved network data are not touched.
- **Empty-folder review:** does not scan automatically. The user must click **Scan for empty folders** to scan accessible local drive roots; results have visible progress and a bounded candidate list. Users select folders and approve removal; each folder is checked again and removed only if it is still empty and inside a scanned drive.
- **Empty-folder safety:** when fewer than 10 candidates are found, the app recommends no cleanup. Access-denied folders are skipped; it never changes ownership or permissions.
- **Memory optimization:** one reviewed **Free RAM now** action uses the documented Windows `EmptyWorkingSet` API on eligible app processes and shows progress. Access-denied or exited processes are skipped while other processes continue. MemReduct also uses undocumented global system-memory calls; this app does not use those private APIs. Before/after memory-in-use values are system-wide measurements that may change for other reasons, not a promise of memory reclaimed.
- **DNS cache flush:** runs `ipconfig /flushdns` only after review and confirmation. Saved Wi-Fi profiles/history are not touched.

On Windows, the app requests administrator elevation on **every launch** using its embedded application manifest. Windows displays a UAC consent prompt; declining cancels the launch. Available hardware and sensor readings depend on firmware, device, and driver support.

Bulk app installation, driver lookup, registry tools, and app uninstalling are not included in this build. The nonfunctional App Installer, Drivers, Registry, and Installed Apps menu entries have been removed. Catalog EXE/MSI entries and silent arguments are never run.

Scans and actions start only after an explicit user click; no cleanup or storage scan runs automatically on launch or navigation. Long-running scans and actions are serialized: while one is active, other action buttons are disabled with a notice. RAM cleanup has a 20-second cooldown after each run.

## Download and install

The [0.0.1 GitHub release](https://github.com/tarataritools/TaraTariOptimizerApp/releases/tag/v0.0.1) provides:

- **TaraTariOptimizerApp-Setup-x64.msi** — installer for 64-bit Windows.
- **TaraTariOptimizerApp-Setup-x86.msi** — installer for 32-bit Windows.
- **TaraTariOptimizerApp-Portable-x64.zip** — x64 app and its hardware bridge; extract and run `TaraTariOptimizerApp.exe`.
- **TaraTariOptimizerApp-Portable-x86.zip** — x86 app and its hardware bridge; extract and run `TaraTariOptimizerApp.exe`.
- **TaraTariOptimizerApp-Source.zip** — source code for the tagged release.

The portable packages do not install the app, but include the `hardware-bridge` folder required for LibreHardwareMonitor readings. The installers add Start Menu shortcuts and support uninstalling from Windows Settings.

## Build

Install a current stable Rust toolchain, the `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc` Rust targets, the .NET 8 SDK, and Visual Studio Build Tools with the **Desktop development with C++** workload (including the Windows SDK). Run the package script from PowerShell:

```powershell
.\release\package-windows.ps1 -DotnetRoot "C:\Program Files\dotnet"
```

`cargo test --no-default-features` runs the tests without embedding the elevation manifest, so the test harness itself does not request administrator privileges.

The Rust interface uses Windows system APIs for storage-volume information and LibreHardwareMonitor for hardware and sensor readings. LibreHardwareMonitor is a .NET library, so a small self-contained .NET 8 helper communicates its results to the Rust app as JSON; the user does not need to install .NET.

Release packaging stages architecture-matched app/bridge pairs, creates x64 and x86 MSI installers with the open-source WiX Toolset, creates portable ZIPs, and exports the tagged source tree.

## Roadmap

Remaining milestones:

1. **Hardware inventory detail** — map each Windows volume to its physical disk and distinguish SSD/NVMe/HDD where Windows exposes that information.
2. **Cross-platform support** — add platform-specific inventory and maintenance providers without converting the native Rust UI into a web app.

The app's native Rust UI and platform-specific inventory provider are structured to support other desktop platforms in a later phase without converting the interface into a web app.

Every destructive or system-modifying feature is gated on preview, explicit user approval, and safeguards. No Windows Update integration is planned for driver lookup.
