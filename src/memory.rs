#[cfg(not(windows))]
use std::io;

use sysinfo::{ProcessesToUpdate, System};

#[derive(Clone, Debug)]
pub struct MemoryProcess {
    pub pid: u32,
    pub name: String,
    pub working_set_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct MemoryProgress {
    pub completed: usize,
    pub total: usize,
    pub process_name: String,
}

#[derive(Clone, Debug)]
pub struct MemoryResult {
    pub processed: usize,
    pub trimmed: usize,
    pub used_before: u64,
    pub used_after: u64,
    pub errors: Vec<String>,
}

pub fn list_application_processes() -> Vec<MemoryProcess> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let current_pid = std::process::id();
    let mut processes = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let pid = pid.as_u32();
            let name = process.name().to_string_lossy().trim().to_owned();
            if pid <= 1000
                || pid == current_pid
                || name.is_empty()
                || is_core_windows_process(&name)
            {
                return None;
            }

            Some(MemoryProcess {
                pid,
                name,
                working_set_bytes: process.memory(),
            })
        })
        .collect::<Vec<_>>();
    processes.sort_by(|left, right| {
        right
            .working_set_bytes
            .cmp(&left.working_set_bytes)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    processes
}

fn is_core_windows_process(name: &str) -> bool {
    [
        "system",
        "registry",
        "smss.exe",
        "csrss.exe",
        "wininit.exe",
        "services.exe",
        "lsass.exe",
        "winlogon.exe",
    ]
    .iter()
    .any(|system_process| name.eq_ignore_ascii_case(system_process))
}

#[cfg(windows)]
pub fn trim_application_working_sets(mut progress: impl FnMut(MemoryProgress)) -> MemoryResult {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::{
            ProcessStatus::EmptyWorkingSet,
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA},
        },
    };

    let processes = list_application_processes();
    let mut memory = System::new();
    memory.refresh_memory();
    let used_before = memory.used_memory();
    let total = processes.len();
    let mut trimmed = 0;
    let mut errors = Vec::new();
    for (index, process) in processes.iter().enumerate() {
        progress(MemoryProgress {
            completed: index,
            total,
            process_name: process.name.clone(),
        });
        let result = unsafe {
            OpenProcess(
                PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION,
                false,
                process.pid,
            )
        }
        .and_then(|handle| {
            let result = unsafe { EmptyWorkingSet(handle) };
            let _ = unsafe { CloseHandle(handle) };
            result
        });

        match result {
            Ok(()) => trimmed += 1,
            Err(error) => errors.push(format!("{} (PID {}): {error}", process.name, process.pid)),
        }
    }

    progress(MemoryProgress {
        completed: total,
        total,
        process_name: "Measuring available memory".to_owned(),
    });
    memory.refresh_memory();
    MemoryResult {
        processed: total,
        trimmed,
        used_before,
        used_after: memory.used_memory(),
        errors,
    }
}

#[cfg(not(windows))]
pub fn trim_application_working_sets(_progress: impl FnMut(MemoryProgress)) -> MemoryResult {
    MemoryResult {
        processed: 0,
        trimmed: 0,
        used_before: 0,
        used_after: 0,
        errors: vec![io::Error::new(
            io::ErrorKind::Unsupported,
            "working-set trimming is available only on Windows",
        )
        .to_string()],
    }
}
