use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

const MAX_VISITED_ENTRIES: usize = 1_000_000;
const MAX_RESULTS: usize = 20;

#[derive(Clone, Debug)]
pub struct StorageEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Default)]
pub struct StorageReport {
    pub scanned_root: PathBuf,
    pub total_file_bytes: u64,
    pub visited_entries: usize,
    pub skipped_entries: usize,
    pub truncated: bool,
    pub largest_folders: Vec<StorageEntry>,
    pub largest_files: Vec<StorageEntry>,
}

pub fn analyze_with_progress(root: &Path, mut progress: impl FnMut(usize, u64)) -> StorageReport {
    let mut report = StorageReport {
        scanned_root: root.to_path_buf(),
        ..StorageReport::default()
    };
    let mut folder_sizes = HashMap::<PathBuf, u64>::new();
    let mut largest_files = Vec::new();
    let canonical_root = match root.canonicalize() {
        Ok(path) if path.is_dir() => path,
        _ => {
            report.skipped_entries = 1;
            return report;
        }
    };

    for result in WalkDir::new(&canonical_root)
        .follow_links(false)
        .into_iter()
    {
        let entry = match result {
            Ok(entry) => entry,
            Err(_) => {
                report.skipped_entries += 1;
                continue;
            }
        };
        report.visited_entries += 1;
        if report.visited_entries % 1000 == 0 || report.visited_entries == 1 {
            progress(report.visited_entries, report.total_file_bytes);
        }
        if report.visited_entries >= MAX_VISITED_ENTRIES {
            report.truncated = true;
            break;
        }
        if entry.file_type().is_symlink() || !entry.file_type().is_file() {
            continue;
        }

        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            Ok(_) => continue,
            Err(_) => {
                report.skipped_entries += 1;
                continue;
            }
        };
        let size_bytes = metadata.len();
        report.total_file_bytes = report.total_file_bytes.saturating_add(size_bytes);
        let relative = match entry.path().strip_prefix(&canonical_root) {
            Ok(path) => path,
            Err(_) => {
                report.skipped_entries += 1;
                continue;
            }
        };
        let folder_key = match relative.components().next() {
            Some(component) if relative.components().count() > 1 => {
                canonical_root.join(component.as_os_str())
            }
            _ => canonical_root.join("(files at drive root)"),
        };
        *folder_sizes.entry(folder_key).or_default() += size_bytes;
        largest_files.push(StorageEntry {
            path: entry.path().to_path_buf(),
            size_bytes,
        });
    }

    report.largest_folders = folder_sizes
        .into_iter()
        .map(|(path, size_bytes)| StorageEntry { path, size_bytes })
        .collect();
    report
        .largest_folders
        .sort_by_key(|entry| std::cmp::Reverse(entry.size_bytes));
    report.largest_folders.truncate(MAX_RESULTS);
    largest_files.sort_by_key(|entry| std::cmp::Reverse(entry.size_bytes));
    largest_files.truncate(MAX_RESULTS);
    report.largest_files = largest_files;
    progress(report.visited_entries, report.total_file_bytes);
    report
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::analyze_with_progress;

    #[test]
    fn groups_storage_by_top_level_folder_and_lists_large_files() {
        let root =
            std::env::temp_dir().join(format!("tara-tari-storage-test-{}", std::process::id()));
        let nested = root.join("Games").join("sample.bin");
        fs::create_dir_all(nested.parent().unwrap()).unwrap();
        fs::write(&nested, vec![0_u8; 64]).unwrap();
        fs::write(root.join("movie.mp4"), vec![0_u8; 32]).unwrap();

        let report = analyze_with_progress(&root, |_, _| {});
        assert_eq!(report.total_file_bytes, 96);
        assert_eq!(report.largest_files[0].path, nested.canonicalize().unwrap());
        assert!(report.largest_folders.iter().any(|entry| {
            entry.path == root.canonicalize().unwrap().join("Games") && entry.size_bytes == 64
        }));
        assert!(report.largest_folders.iter().any(|entry| {
            entry.path == PathBuf::from(root.canonicalize().unwrap()).join("(files at drive root)")
                && entry.size_bytes == 32
        }));

        fs::remove_dir_all(root).unwrap();
    }
}
