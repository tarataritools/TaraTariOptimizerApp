# TaraTari Optimizer App 0.0.1

First public Windows release of the native Rust desktop app.

## Downloads

- `TaraTariOptimizerApp-Setup-x64.msi` — guided installation on 64-bit Windows.
- `TaraTariOptimizerApp-Setup-x86.msi` — guided installation on 32-bit Windows.
- `TaraTariOptimizerApp-Portable-x64.zip` — extract and run the x64 app.
- `TaraTariOptimizerApp-Portable-x86.zip` — extract and run the x86 app.
- `TaraTariOptimizerApp-Source.zip` — source at this release tag.

Both portable packages include the matching self-contained LibreHardwareMonitor bridge in `hardware-bridge`. The bridge does not require .NET to be installed on the user's PC.

## Features

- My Specs hardware and sensor readings via LibreHardwareMonitor.
- Cache Cleaner with preview before deleting temporary files.
- Manual empty-folder scan with selection and confirmation before removal.
- Read-only storage analysis.
- Temporary memory working-set reclamation.
- DNS cache flush with confirmation.
- Administrator elevation requested by Windows at launch.

Hardware readings depend on the machine's firmware, drivers, and sensor support. Cleanup and system-changing actions require a user action and confirmation.
