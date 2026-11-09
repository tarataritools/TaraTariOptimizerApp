use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use walkdir::WalkDir;

const MAX_SCAN_ENTRIES: usize = 10_000;
const MAX_VISITED_ENTRIES: usize = 50_000;
const MAX_SCAN_DEPTH: usize = 8;
const MINIMUM_FILE_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug)]
pub struct CleanupItem {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified: SystemTime,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub struct FolderItem {
    pub path: PathBuf,
    pub selected: bool,
}

#[derive(Debug)]
pub struct ScanReport<T> {
    pub items: Vec<T>,
    pub skipped_entries: usize,
    pub truncated: bool,
}

impl<T> Default for ScanReport<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            skipped_entries: 0,
            truncated: false,
        }
    }
}

#[derive(Debug)]
pub struct DeleteReport {
    pub deleted: usize,
    pub bytes_removed: u64,
    pub errors: Vec<String>,
    pub deleted_paths: Vec<PathBuf>,
}

pub fn scan_temporary_files_with_progress(
    mut progress: impl FnMut(usize),
) -> ScanReport<CleanupItem> {
    let mut report = ScanReport::default();
    let mut roots = vec![std::env::temp_dir()];
    if let Some(windows) = std::env::var_os("WINDIR") {
        roots.push(PathBuf::from(windows).join("Temp"));
    }
    let now = SystemTime::now();
    let mut visited = 0;

    for root in roots {
        if !root.is_dir() {
            continue;
        }

        for result in WalkDir::new(&root)
            .follow_links(false)
            .max_depth(MAX_SCAN_DEPTH)
            .into_iter()
        {
            let entry = match result {
                Ok(entry) => entry,
                Err(_) => {
                    report.skipped_entries += 1;
                    continue;
                }
            };
            visited += 1;
            if visited % 100 == 0 || visited == 1 {
                progress(visited);
            }
            if visited >= MAX_VISITED_ENTRIES {
                report.truncated = true;
                progress(visited);
                return report;
            }

            if entry.file_type().is_symlink() || entry.file_type().is_dir() {
                continue;
            }
            if report.items.len() >= MAX_SCAN_ENTRIES {
                report.truncated = true;
                return report;
            }

            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => {
                    report.skipped_entries += 1;
                    continue;
                }
            };
            if !metadata.is_file() {
                continue;
            }

            let modified = match metadata.modified() {
                Ok(modified) => modified,
                Err(_) => {
                    report.skipped_entries += 1;
                    continue;
                }
            };
            if now.duration_since(modified).unwrap_or_default() < MINIMUM_FILE_AGE {
                continue;
            }

            report.items.push(CleanupItem {
                path: entry.path().to_owned(),
                size_bytes: metadata.len(),
                modified,
                selected: false,
            });
        }
    }

    progress(visited);
    report
}

pub fn delete_selected_temp_files(
    items: &[CleanupItem],
    mut progress: impl FnMut(usize, usize),
) -> DeleteReport {
    let roots = allowed_temp_roots();
    let mut deleted = 0;
    let selected_count = items.iter().filter(|item| item.selected).count();
    let mut completed = 0;
    let mut bytes_removed = 0;
    let mut errors = Vec::new();
    let mut deleted_paths = Vec::new();

    for item in items.iter().filter(|item| item.selected) {
        match remove_temp_file(&item.path, &roots) {
            Ok(bytes) => {
                deleted += 1;
                bytes_removed += bytes;
                deleted_paths.push(item.path.clone());
            }
            Err(error) => errors.push(format!("{}: {error}", item.path.display())),
        }
        completed += 1;
        progress(completed, selected_count);
    }

    DeleteReport {
        deleted,
        bytes_removed,
        errors,
        deleted_paths,
    }
}

fn allowed_temp_roots() -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir()];
    if let Some(windows) = std::env::var_os("WINDIR") {
        roots.push(PathBuf::from(windows).join("Temp"));
    }
    roots
}

fn remove_temp_file(path: &Path, roots: &[PathBuf]) -> std::io::Result<u64> {
    let canonical_target = path.canonicalize()?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target is no longer a regular file",
        ));
    }
    let belongs_to_temp_root = roots.iter().any(|root| {
        root.canonicalize()
            .map(|canonical_root| canonical_target.starts_with(canonical_root))
            .unwrap_or(false)
    });
    if !belongs_to_temp_root {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "target is outside the allowed temporary folders",
        ));
    }
    let bytes = metadata.len();
    fs::remove_file(path)?;
    Ok(bytes)
}

pub fn scan_empty_folders_with_progress(
    root: &Path,
    mut progress: impl FnMut(usize),
) -> ScanReport<FolderItem> {
    let mut report = ScanReport::default();
    let canonical_root = match root.canonicalize() {
        Ok(path) if path.is_dir() => path,
        _ => {
            report.skipped_entries = 1;
            return report;
        }
    };

    let mut visited = 0;
    for result in WalkDir::new(&canonical_root)
        .follow_links(false)
        .min_depth(1)
        .contents_first(true)
        .into_iter()
    {
        let entry = match result {
            Ok(entry) => entry,
            Err(_) => {
                report.skipped_entries += 1;
                continue;
            }
        };
        visited += 1;
        if visited % 100 == 0 || visited == 1 {
            progress(visited);
        }
        if entry.file_type().is_symlink() || !entry.file_type().is_dir() {
            continue;
        }

        match fs::read_dir(entry.path()) {
            Ok(children) => {
                if children.count() == 0 {
                    if report.items.len() < MAX_SCAN_ENTRIES {
                        report.items.push(FolderItem {
                            path: entry.path().to_owned(),
                            selected: false,
                        });
                    } else {
                        report.truncated = true;
                    }
                }
            }
            Err(_) => report.skipped_entries += 1,
        }
    }

    progress(visited);
    report
}

pub fn remove_selected_empty_folders_from_roots_with_progress(
    roots: &[PathBuf],
    items: &[FolderItem],
    mut progress: impl FnMut(usize, usize),
) -> (usize, Vec<String>) {
    let canonical_roots = roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .filter(|root| root.is_dir())
        .collect::<Vec<_>>();
    let mut removed = 0;
    let mut errors = Vec::new();
    let total = items.iter().filter(|item| item.selected).count();
    let mut completed = 0;

    for item in items.iter().filter(|item| item.selected) {
        let result = (|| {
            let canonical_target = item.path.canonicalize()?;
            if !canonical_roots
                .iter()
                .any(|root| canonical_target != *root && canonical_target.starts_with(root))
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "folder is outside the scanned local volumes",
                ));
            }
            let metadata = fs::symlink_metadata(&item.path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "target is no longer a normal folder",
                ));
            }
            fs::remove_dir(&item.path)
        })();

        match result {
            Ok(()) => removed += 1,
            Err(error) => errors.push(format!("{}: {error}", item.path.display())),
        }
        completed += 1;
        progress(completed, total);
    }

    (removed, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_remove_a_folder_that_is_no_longer_empty() {
        let root =
            std::env::temp_dir().join(format!("tara-tari-empty-test-{}", std::process::id()));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("created-after-scan");
        fs::write(&file, "keep").unwrap();

        let items = [FolderItem {
            path: nested.clone(),
            selected: true,
        }];
        let (removed, errors) = remove_selected_empty_folders_from_roots_with_progress(
            &[root.clone()],
            &items,
            |_, _| {},
        );
        assert_eq!(removed, 0);
        assert_eq!(errors.len(), 1);
        assert!(file.exists());

        fs::remove_file(file).unwrap();
        fs::remove_dir(nested).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn empty_folder_scan_reaches_beyond_the_old_depth_limit() {
        let root = std::env::temp_dir().join(format!("tara-tari-deep-scan-{}", std::process::id()));
        let mut leaf = root.clone();
        for index in 0..10 {
            leaf.push(format!("level-{index}"));
        }
        fs::create_dir_all(&leaf).unwrap();

        let report = scan_empty_folders_with_progress(&root, |_| {});
        let canonical_leaf = leaf.canonicalize().unwrap();
        assert!(report.items.iter().any(|item| item.path == canonical_leaf));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn multi_root_removal_rejects_folders_outside_scanned_volumes() {
        let base = std::env::temp_dir().join(format!("tara-tari-roots-{}", std::process::id()));
        let scanned_root = base.join("scanned");
        let outside_root = base.join("outside");
        let scanned_empty = scanned_root.join("empty");
        let outside_empty = outside_root.join("empty");
        fs::create_dir_all(&scanned_empty).unwrap();
        fs::create_dir_all(&outside_empty).unwrap();
        let roots = [scanned_root.clone()];
        let items = [
            FolderItem {
                path: scanned_empty,
                selected: true,
            },
            FolderItem {
                path: outside_empty.clone(),
                selected: true,
            },
        ];

        let (removed, errors) =
            remove_selected_empty_folders_from_roots_with_progress(&roots, &items, |_, _| {});
        assert_eq!(removed, 1);
        assert_eq!(errors.len(), 1);
        assert!(outside_empty.is_dir());

        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn refuses_to_remove_a_temp_file_outside_the_allowlist() {
        let outside =
            std::env::temp_dir().join(format!("tara-tari-not-a-cache-{}.tmp", std::process::id()));
        fs::write(&outside, "keep").unwrap();

        let roots = [std::env::temp_dir().join("not-this-directory")];
        let error = remove_temp_file(&outside, &roots).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(outside.exists());
        fs::remove_file(outside).unwrap();
    }
}
