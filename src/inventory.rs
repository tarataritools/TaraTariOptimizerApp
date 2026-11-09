use sysinfo::Disks;

#[derive(Clone, Debug)]
pub struct Volume {
    pub mount_point: String,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoredHardware {
    pub name: String,
    #[serde(rename = "type")]
    pub hardware_type: String,
    pub sensors: Vec<MonitorSensor>,
    pub sub_hardware: Vec<MonitoredHardware>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSensor {
    pub name: String,
    #[serde(rename = "type")]
    pub sensor_type: String,
    pub value: f32,
}

#[derive(Clone, Debug, Default)]
pub struct HardwareInventory {
    pub volumes: Vec<Volume>,
    pub monitored_hardware: Vec<MonitoredHardware>,
    pub hardware_error: Option<String>,
}

pub fn collect() -> HardwareInventory {
    let disks = Disks::new_with_refreshed_list();
    let volumes = disks
        .iter()
        .map(|disk| Volume {
            mount_point: disk.mount_point().to_string_lossy().into_owned(),
        })
        .collect();

    let (monitored_hardware, hardware_error) = {
        #[cfg(windows)]
        {
            match libre_hardware_monitor::collect() {
                Ok(hardware) => (hardware, None),
                Err(error) => (Vec::new(), Some(error)),
            }
        }
        #[cfg(not(windows))]
        {
            (
                Vec::new(),
                Some("LibreHardwareMonitor is currently supported only on Windows.".to_owned()),
            )
        }
    };

    HardwareInventory {
        volumes,
        monitored_hardware,
        hardware_error,
    }
}

#[cfg(windows)]
mod libre_hardware_monitor {
    use std::{env, process::Command};

    use serde::Deserialize;

    use super::MonitoredHardware;

    #[derive(Deserialize)]
    struct HardwareReport {
        hardware: Vec<MonitoredHardware>,
    }

    pub(super) fn collect() -> Result<Vec<MonitoredHardware>, String> {
        let current_exe = env::current_exe().map_err(|error| {
            format!("Could not locate the app for LibreHardwareMonitor: {error}")
        })?;
        let bridge_path = current_exe
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("hardware-bridge")
            .join("TaraTariHardwareBridge.exe");
        if !bridge_path.is_file() {
            return Err(
                "LibreHardwareMonitor bridge was not built. Rebuild the app with the .NET 8 SDK to include hardware detection."
                    .to_owned(),
            );
        }
        let output = Command::new(&bridge_path)
            .output()
            .map_err(|error| format!("Could not start the LibreHardwareMonitor bridge: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "LibreHardwareMonitor could not read sensors: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let report: HardwareReport = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("LibreHardwareMonitor returned invalid data: {error}"))?;
        Ok(report.hardware)
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;

    if bytes >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.0} MB", bytes as f64 / MIB as f64)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, MonitoredHardware};

    #[test]
    fn formats_memory_sizes_in_binary_units() {
        assert_eq!(format_bytes(512), "512 bytes");
        assert_eq!(format_bytes(1024 * 1024), "1 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn deserializes_libre_hardware_monitor_camel_case_report() {
        let report: MonitoredHardware = serde_json::from_str(
            r#"{"name":"Test CPU","type":"Cpu","sensors":[{"name":"Core 1","type":"Temperature","value":42.0}],"subHardware":[]}"#,
        )
        .unwrap();

        assert_eq!(report.name, "Test CPU");
        assert_eq!(report.hardware_type, "Cpu");
        assert_eq!(report.sensors[0].name, "Core 1");
        assert_eq!(report.sensors[0].value, 42.0);
    }
}
