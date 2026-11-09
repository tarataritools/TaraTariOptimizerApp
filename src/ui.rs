use std::{
    any::Any,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, SystemTime},
};

use eframe::egui::{self, Color32, RichText, Stroke};

use crate::{
    cleanup::{self, CleanupItem, FolderItem, ScanReport},
    inventory::{self, HardwareInventory},
    memory::{self, MemoryProgress, MemoryResult},
    storage::{self, StorageReport},
};

const BG: Color32 = Color32::from_rgb(9, 9, 11);
const SURFACE: Color32 = Color32::from_rgb(19, 19, 22);
const SURFACE_ALT: Color32 = Color32::from_rgb(28, 28, 32);
const BORDER: Color32 = Color32::from_rgb(48, 48, 54);
const TEXT: Color32 = Color32::from_rgb(228, 228, 231);
const MUTED: Color32 = Color32::from_rgb(161, 161, 170);
const GREEN: Color32 = Color32::from_rgb(74, 222, 128);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Cleanup,
    EmptyFolders,
    Storage,
    Memory,
    Network,
    About,
}
#[derive(Clone)]
enum PendingAction {
    RemoveEmptyFolders(Vec<PathBuf>, Vec<FolderItem>),
    FlushDns,
}

enum MemoryEvent {
    Progress(MemoryProgress),
    Finished(MemoryResult),
    Failed(String),
}

enum CleanupEvent {
    ScanProgress(usize),
    ScanFinished(ScanReport<CleanupItem>),
    DeleteProgress { completed: usize, total: usize },
    DeleteFinished(cleanup::DeleteReport),
}

enum EmptyFolderEvent {
    ScanProgress {
        root_index: usize,
        total_roots: usize,
        root: PathBuf,
        visited: usize,
    },
    ScanFinished(ScanReport<FolderItem>),
    RemoveProgress {
        completed: usize,
        total: usize,
    },
    RemoveFinished {
        removed: usize,
        errors: Vec<String>,
    },
}

enum StorageEvent {
    Progress { visited: usize, bytes: u64 },
    Finished(StorageReport),
}

pub struct OptimizerApp {
    inventory: HardwareInventory,
    page: Page,
    cleanup_items: Vec<CleanupItem>,
    cleanup_scanned: bool,
    cleanup_notice: Option<String>,
    cleanup_receiver: Option<Receiver<CleanupEvent>>,
    cleanup_progress: Option<(usize, Option<usize>)>,
    empty_items: Vec<FolderItem>,
    empty_scanned: bool,
    empty_scan_truncated: bool,
    empty_notice: Option<String>,
    empty_receiver: Option<Receiver<EmptyFolderEvent>>,
    empty_progress: Option<(usize, usize, PathBuf, usize)>,
    cleanup_result: Option<cleanup::DeleteReport>,
    cleanup_animation_id: u64,
    storage_receiver: Option<Receiver<StorageEvent>>,
    storage_report: Option<StorageReport>,
    storage_progress: Option<(usize, u64)>,
    selected_volume: usize,
    memory_receiver: Option<Receiver<MemoryEvent>>,
    memory_progress: Option<MemoryProgress>,
    memory_result: Option<MemoryResult>,
    pending_action: Option<PendingAction>,
    action_notice: Option<String>,
    memory_cooldown_until: Option<std::time::Instant>,
}

impl OptimizerApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = SURFACE;
        visuals.extreme_bg_color = BG;
        visuals.faint_bg_color = SURFACE;
        visuals.widgets.noninteractive.bg_fill = SURFACE;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(9);
        visuals.widgets.inactive.bg_fill = SURFACE_ALT;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(9);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 39, 42);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(9);
        visuals.widgets.active.bg_fill = GREEN;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, BG);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(9);
        visuals.selection.bg_fill = GREEN;
        visuals.selection.stroke = Stroke::new(1.0_f32, BG);
        visuals.window_corner_radius = egui::CornerRadius::same(13);
        let mut style = (*creation_context.egui_ctx.style()).clone();
        style.visuals = visuals;
        style.spacing.interact_size.y = 44.0;
        style.spacing.button_padding = egui::vec2(16.0, 10.0);
        style.spacing.item_spacing = egui::vec2(12.0, 12.0);
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        creation_context.egui_ctx.set_style(style);

        Self {
            inventory: inventory::collect(),
            page: Page::Overview,
            cleanup_items: Vec::new(),
            cleanup_scanned: false,
            cleanup_notice: None,
            cleanup_receiver: None,
            cleanup_progress: None,
            empty_items: Vec::new(),
            empty_scanned: false,
            empty_scan_truncated: false,
            empty_notice: None,
            empty_receiver: None,
            empty_progress: None,
            cleanup_result: None,
            cleanup_animation_id: 0,
            storage_receiver: None,
            storage_report: None,
            storage_progress: None,
            selected_volume: 0,
            memory_receiver: None,
            memory_progress: None,
            memory_result: None,
            pending_action: None,
            action_notice: None,
            memory_cooldown_until: None,
        }
    }

    fn scan_cleanup(&mut self) {
        if self.operation_running() {
            self.action_notice = Some(busy_action_message().to_owned());
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.cleanup_scanned = false;
        self.cleanup_result = None;
        self.cleanup_progress = Some((0, None));
        self.cleanup_receiver = Some(receiver);
        std::thread::spawn(move || {
            let progress_sender = sender.clone();
            let report = cleanup::scan_temporary_files_with_progress(move |visited| {
                let _ = progress_sender.send(CleanupEvent::ScanProgress(visited));
            });
            let _ = sender.send(CleanupEvent::ScanFinished(report));
        });
    }

    fn start_cleanup(&mut self, items: Vec<CleanupItem>) {
        if self.operation_running() {
            self.action_notice = Some(busy_action_message().to_owned());
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let total = items.iter().filter(|item| item.selected).count();
        self.cleanup_result = None;
        self.cleanup_progress = Some((0, Some(total)));
        self.cleanup_receiver = Some(receiver);
        std::thread::spawn(move || {
            let progress_sender = sender.clone();
            let report = cleanup::delete_selected_temp_files(&items, move |completed, total| {
                let _ = progress_sender.send(CleanupEvent::DeleteProgress { completed, total });
            });
            let _ = sender.send(CleanupEvent::DeleteFinished(report));
        });
    }

    fn poll_cleanup(&mut self) {
        let Some(receiver) = &self.cleanup_receiver else {
            return;
        };
        let mut finished = false;
        loop {
            match receiver.try_recv() {
                Ok(CleanupEvent::ScanProgress(visited)) => {
                    self.cleanup_progress = Some((visited, None))
                }
                Ok(CleanupEvent::ScanFinished(report)) => {
                    self.cleanup_notice = Some(scan_summary(&report));
                    let mut items = report.items;
                    for item in &mut items {
                        item.selected = true;
                    }
                    self.cleanup_items = items;
                    self.cleanup_scanned = true;
                    self.cleanup_progress = None;
                    finished = true;
                    break;
                }
                Ok(CleanupEvent::DeleteProgress { completed, total }) => {
                    self.cleanup_progress = Some((completed, Some(total)))
                }
                Ok(CleanupEvent::DeleteFinished(report)) => {
                    self.cleanup_items
                        .retain(|item| !report.deleted_paths.contains(&item.path));
                    self.cleanup_result = Some(report);
                    self.cleanup_animation_id = self.cleanup_animation_id.wrapping_add(1);
                    self.cleanup_progress = None;
                    finished = true;
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    self.cleanup_notice = Some(
                        "The cache task ended unexpectedly before returning a result.".to_owned(),
                    );
                    self.cleanup_progress = None;
                    finished = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if finished {
            self.cleanup_receiver = None;
        }
    }

    fn scan_empty_folders(&mut self) {
        if self.operation_running() {
            self.action_notice = Some(busy_action_message().to_owned());
            return;
        }
        let roots = self.local_volume_roots();
        let (sender, receiver) = mpsc::channel();
        self.empty_items.clear();
        self.empty_scanned = false;
        self.empty_scan_truncated = false;
        self.empty_progress = Some((0, roots.len(), PathBuf::new(), 0));
        self.empty_receiver = Some(receiver);
        std::thread::spawn(move || {
            let mut combined = ScanReport::default();
            let total_roots = roots.len();
            for (index, root) in roots.iter().enumerate() {
                let progress_sender = sender.clone();
                let root_for_progress = root.clone();
                let report = cleanup::scan_empty_folders_with_progress(root, move |visited| {
                    let _ = progress_sender.send(EmptyFolderEvent::ScanProgress {
                        root_index: index,
                        total_roots,
                        root: root_for_progress.clone(),
                        visited,
                    });
                });
                combined.items.extend(report.items);
                combined.skipped_entries += report.skipped_entries;
                combined.truncated |= report.truncated;
            }
            let _ = sender.send(EmptyFolderEvent::ScanFinished(combined));
        });
    }

    fn remove_empty_folders(&mut self, roots: Vec<PathBuf>, items: Vec<FolderItem>) {
        if self.operation_running() {
            self.action_notice = Some(busy_action_message().to_owned());
            return;
        }
        let total = items.iter().filter(|item| item.selected).count();
        let (sender, receiver) = mpsc::channel();
        self.empty_progress = Some((0, 0, PathBuf::new(), total));
        self.empty_receiver = Some(receiver);
        std::thread::spawn(move || {
            let progress_sender = sender.clone();
            let (removed, errors) = cleanup::remove_selected_empty_folders_from_roots_with_progress(
                &roots,
                &items,
                move |completed, total| {
                    let _ =
                        progress_sender.send(EmptyFolderEvent::RemoveProgress { completed, total });
                },
            );
            let _ = sender.send(EmptyFolderEvent::RemoveFinished { removed, errors });
        });
    }

    fn poll_empty_folders(&mut self) {
        let Some(receiver) = &self.empty_receiver else {
            return;
        };
        let mut finished = false;
        loop {
            match receiver.try_recv() {
                Ok(EmptyFolderEvent::ScanProgress {
                    root_index,
                    total_roots,
                    root,
                    visited,
                }) => self.empty_progress = Some((root_index, total_roots, root, visited)),
                Ok(EmptyFolderEvent::ScanFinished(report)) => {
                    self.empty_notice = Some(scan_summary(&report));
                    self.empty_scan_truncated = report.truncated;
                    self.empty_items = report.items;
                    self.empty_scanned = true;
                    self.empty_progress = None;
                    finished = true;
                    break;
                }
                Ok(EmptyFolderEvent::RemoveProgress { completed, total }) => {
                    self.empty_progress = Some((completed, total, PathBuf::new(), total))
                }
                Ok(EmptyFolderEvent::RemoveFinished { removed, errors }) => {
                    self.empty_notice = Some(format_action_result(
                        "empty folders removed",
                        removed,
                        errors,
                    ));
                    self.empty_progress = None;
                    finished = true;
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    self.empty_notice =
                        Some("The folder operation could not return a result.".to_owned());
                    self.empty_progress = None;
                    finished = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if finished {
            self.empty_receiver = None;
        }
    }

    fn local_volume_roots(&self) -> Vec<PathBuf> {
        self.inventory
            .volumes
            .iter()
            .map(|volume| PathBuf::from(&volume.mount_point))
            .filter(|root| is_local_volume_root(root) && root.is_dir())
            .collect()
    }

    fn operation_running(&self) -> bool {
        self.memory_receiver.is_some()
            || self.cleanup_receiver.is_some()
            || self.empty_receiver.is_some()
            || self.storage_receiver.is_some()
    }

    fn start_storage_analysis(&mut self, root: PathBuf) {
        if self.operation_running() {
            self.action_notice = Some(busy_action_message().to_owned());
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.storage_report = None;
        self.storage_progress = Some((0, 0));
        self.storage_receiver = Some(receiver);
        std::thread::spawn(move || {
            let progress_sender = sender.clone();
            let report = storage::analyze_with_progress(&root, move |visited, bytes| {
                let _ = progress_sender.send(StorageEvent::Progress { visited, bytes });
            });
            let _ = sender.send(StorageEvent::Finished(report));
        });
    }

    fn poll_storage(&mut self) {
        let Some(receiver) = &self.storage_receiver else {
            return;
        };
        let mut finished = false;
        loop {
            match receiver.try_recv() {
                Ok(StorageEvent::Progress { visited, bytes }) => {
                    self.storage_progress = Some((visited, bytes));
                }
                Ok(StorageEvent::Finished(report)) => {
                    self.storage_report = Some(report);
                    self.storage_progress = None;
                    finished = true;
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    self.action_notice = Some(
                        "Storage analysis ended unexpectedly before returning a result.".to_owned(),
                    );
                    self.storage_progress = None;
                    finished = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if finished {
            self.storage_receiver = None;
        }
    }

    fn show_nav_item(&mut self, ui: &mut egui::Ui, page: Page, title: &str, detail: &str) {
        let selected = self.page == page;
        let label_color = if selected { BG } else { TEXT };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
        let hover_progress = ui.ctx().animate_value_with_time(
            egui::Id::new(("nav-hover", title)),
            if response.hovered() { 1.0 } else { 0.0 },
            0.28,
        );
        let mix = if selected { 1.0 } else { hover_progress };
        let fill = Color32::from_rgb(
            (28.0 + (74.0 - 28.0) * mix) as u8,
            (28.0 + (222.0 - 28.0) * mix) as u8,
            (32.0 + (128.0 - 32.0) * mix) as u8,
        );
        let painter = ui.painter();
        if selected || response.hovered() {
            painter.rect_filled(rect, egui::CornerRadius::same(8), fill);
        }
        if selected {
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(8),
                Stroke::new(1.0_f32, GREEN),
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(rect.left() + 2.0, rect.top() + 10.0),
                    egui::vec2(3.0, 24.0),
                ),
                egui::CornerRadius::same(2),
                GREEN,
            );
        }
        if hover_progress > 0.0 {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(rect.left() + 10.0, rect.bottom() - 3.0),
                    egui::vec2((rect.width() - 20.0) * hover_progress, 2.0),
                ),
                egui::CornerRadius::same(1),
                GREEN,
            );
        }
        draw_nav_icon(
            painter,
            page,
            egui::pos2(rect.left() + 25.0, rect.center().y),
            if selected || response.hovered() {
                BG
            } else {
                MUTED
            },
        );
        painter.text(
            egui::pos2(rect.left() + 52.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::proportional(15.0),
            if response.hovered() { BG } else { label_color },
        );
        let response = response.on_hover_text(detail);
        if response.clicked() {
            self.page = page;
            if page == Page::Storage && self.storage_report.is_none() {
                self.selected_volume = 0;
            }
        }
    }

    fn render_memory(&mut self, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "MemReduct",
            "Ask Windows to reclaim app working-set pages. This is temporary and can increase page reloads.",
        );
        self.section_card(ui, "What this does", |ui| {
            ui.label(
                RichText::new(
                    "This uses Windows' documented EmptyWorkingSet API on eligible application processes. MemReduct also uses undocumented global system-memory calls; this app deliberately does not. Windows may deny protected or service processes, which are skipped while other apps continue.",
                )
                .color(MUTED),
            );
        });
        ui.add_space(12.0);
        let cooldown_remaining = self
            .memory_cooldown_until
            .map(|until| until.saturating_duration_since(std::time::Instant::now()));
        let cooling_down = cooldown_remaining.is_some_and(|remaining| !remaining.is_zero());
        if cooling_down {
            let seconds = cooldown_remaining.unwrap().as_secs_f32().ceil() as u64;
            ui.label(
                RichText::new(format!("RAM cleanup cooldown: {seconds} seconds")).color(MUTED),
            );
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        if ui
            .add_enabled(
                !self.operation_running() && !cooling_down,
                egui::Button::new(RichText::new("Free RAM now").size(17.0).strong().color(BG))
                    .fill(GREEN)
                    .min_size(egui::vec2(280.0, 60.0)),
            )
            .clicked()
        {
            self.start_memory_trim();
        }

        if let Some(progress) = &self.memory_progress {
            let fraction = if progress.total == 0 {
                0.0
            } else {
                progress.completed as f32 / progress.total as f32
            };
            ui.add_space(12.0);
            ui.label(RichText::new(&progress.process_name).color(MUTED));
            ui.add(egui::ProgressBar::new(fraction).fill(GREEN).text(format!(
                "{}/{} processes",
                progress.completed, progress.total
            )));
        }
        if let Some(result) = &self.memory_result {
            let used_change = result.used_before.abs_diff(result.used_after);
            let used_direction = if result.used_after < result.used_before {
                "less"
            } else if result.used_after > result.used_before {
                "more"
            } else {
                "the same amount of"
            };
            ui.add_space(12.0);
            self.section_card(ui, "Last result", |ui| {
                ui.horizontal(|ui| {
                    Self::memory_stat(
                        ui,
                        "Memory in use before",
                        inventory::format_bytes(result.used_before),
                    );
                    Self::memory_stat(
                        ui,
                        "Memory in use after",
                        inventory::format_bytes(result.used_after),
                    );
                    Self::memory_stat(
                        ui,
                        "Working sets trimmed",
                        format!("{} / {}", result.trimmed, result.processed),
                    );
                });
                ui.add_space(12.0);
                ui.label(
                    RichText::new(format!(
                        "Windows reported {} {} in use after the operation. This system-wide reading can also change because of other apps.",
                        inventory::format_bytes(used_change),
                        used_direction
                    ))
                    .color(MUTED),
                );
                if !result.errors.is_empty() {
                    ui.label(
                        RichText::new(format!(
                            "{} processes could not be trimmed (for example, access was denied or a process exited); the operation continued through the remaining eligible processes.",
                            result.errors.len()
                        ))
                        .color(MUTED),
                    );
                    egui::CollapsingHeader::new(format!(
                        "Skipped process details ({})",
                        result.errors.len()
                    ))
                    .show(ui, |ui| {
                        for error in result.errors.iter().take(12) {
                            ui.label(RichText::new(error).small().color(MUTED));
                        }
                    });
                }
            });
        }
    }

    fn render_storage(&mut self, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Who's fat?",
            "Analyze a drive to see which folders and files are using the most space. This scan is read-only.",
        );
        let roots = self.local_volume_roots();
        if roots.is_empty() {
            self.section_card(ui, "No local drives found", |ui| {
                ui.label(
                    RichText::new("Windows did not report an accessible local drive root.")
                        .color(MUTED),
                );
            });
            return;
        }

        if self.selected_volume >= roots.len() {
            self.selected_volume = 0;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Drive to analyze").color(MUTED));
            egui::ComboBox::from_id_salt("storage-drive-picker")
                .selected_text(roots[self.selected_volume].display().to_string())
                .show_ui(ui, |ui| {
                    for (index, root) in roots.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.selected_volume,
                            index,
                            root.display().to_string(),
                        );
                    }
                });
            if ui
                .add_enabled(
                    !self.operation_running(),
                    egui::Button::new(RichText::new("Analyze drive").strong().color(BG))
                        .fill(GREEN)
                        .min_size(egui::vec2(180.0, 48.0)),
                )
                .clicked()
            {
                self.start_storage_analysis(roots[self.selected_volume].clone());
            }
        });

        if let Some((visited, bytes)) = self.storage_progress {
            ui.add_space(12.0);
            ui.label(
                RichText::new(format!(
                    "Scanning {visited} entries · {} found so far",
                    inventory::format_bytes(bytes)
                ))
                .color(MUTED),
            );
            ui.add(
                egui::ProgressBar::new(0.0)
                    .animate(true)
                    .text("Reading folder and file sizes"),
            );
        }

        if let Some(report) = &self.storage_report {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                self.section_card(ui, "Total file data found", |ui| {
                    ui.label(
                        RichText::new(inventory::format_bytes(report.total_file_bytes))
                            .size(26.0)
                            .strong()
                            .color(GREEN),
                    );
                    ui.label(
                        RichText::new(report.scanned_root.display().to_string()).color(MUTED),
                    );
                });
                self.section_card(ui, "Scan coverage", |ui| {
                    ui.label(
                        RichText::new(format!("{} entries visited", report.visited_entries))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} inaccessible entries skipped",
                            report.skipped_entries
                        ))
                        .color(MUTED),
                    );
                    if report.truncated {
                        ui.label(
                            RichText::new(
                                "Stopped at the 1,000,000-entry safety limit; results are incomplete.",
                            )
                            .color(Color32::YELLOW),
                        );
                    }
                });
            });
            ui.add_space(12.0);
            self.section_card(ui, "Space map", |ui| {
                render_storage_treemap(ui, report);
            });
            ui.add_space(12.0);
            ui.columns(2, |columns| {
                columns[0].vertical(|ui| {
                    self.section_card(ui, "Largest folders", |ui| {
                        if report.largest_folders.is_empty() {
                            ui.label(RichText::new("No readable files found.").color(MUTED));
                        }
                        for item in &report.largest_folders {
                            storage_row(ui, &item.path, item.size_bytes, report.total_file_bytes);
                        }
                    });
                });
                columns[1].vertical(|ui| {
                    self.section_card(ui, "Largest files", |ui| {
                        if report.largest_files.is_empty() {
                            ui.label(RichText::new("No readable files found.").color(MUTED));
                        }
                        for item in &report.largest_files {
                            storage_row(ui, &item.path, item.size_bytes, report.total_file_bytes);
                        }
                    });
                });
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new("Folder totals group files by their first folder under the selected drive. This reports file sizes, which can differ from actual disk allocation due to compression, sparse files, and filesystem metadata.")
                    .small()
                    .color(MUTED),
            );
        }
    }

    fn memory_stat(ui: &mut egui::Ui, label: &str, value: String) {
        ui.vertical(|ui| {
            ui.label(RichText::new(label).color(MUTED));
            ui.label(RichText::new(value).size(22.0).color(TEXT));
        });
    }

    fn start_memory_trim(&mut self) {
        if self.operation_running() {
            self.action_notice = Some(busy_action_message().to_owned());
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.memory_progress = Some(MemoryProgress {
            completed: 0,
            total: 0,
            process_name: "Starting...".to_owned(),
        });
        self.memory_result = None;
        self.action_notice = None;
        self.memory_receiver = Some(receiver);
        std::thread::spawn(move || {
            let progress_sender = sender.clone();
            let operation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                memory::trim_application_working_sets(move |progress| {
                    let _ = progress_sender.send(MemoryEvent::Progress(progress));
                })
            }));
            match operation {
                Ok(result) => {
                    let _ = sender.send(MemoryEvent::Finished(result));
                }
                Err(payload) => {
                    let message = panic_message(payload.as_ref());
                    let _ = sender.send(MemoryEvent::Failed(message));
                }
            }
        });
    }

    fn poll_memory(&mut self) {
        let Some(receiver) = &self.memory_receiver else {
            return;
        };
        let mut finished = false;
        loop {
            match receiver.try_recv() {
                Ok(MemoryEvent::Progress(progress)) => self.memory_progress = Some(progress),
                Ok(MemoryEvent::Finished(result)) => {
                    self.memory_result = Some(result);
                    self.memory_cooldown_until =
                        Some(std::time::Instant::now() + Duration::from_secs(20));
                    finished = true;
                    break;
                }
                Ok(MemoryEvent::Failed(error)) => {
                    self.action_notice = Some(format!("Memory optimization failed: {error}"));
                    finished = true;
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    self.action_notice =
                        Some("Memory optimization stopped without returning a result.".to_owned());
                    finished = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if finished {
            self.memory_receiver = None;
            self.memory_progress = None;
        }
    }

    fn section_card(&self, ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::group(ui.style())
            .fill(SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(egui::CornerRadius::same(13))
            .inner_margin(20.0)
            .show(ui, |ui| {
                ui.label(RichText::new(title).size(16.0).strong().color(TEXT));
                ui.add_space(12.0);
                body(ui);
            });
    }

    fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.heading(RichText::new(title).size(28.0).color(TEXT));
        ui.label(RichText::new(subtitle).color(MUTED));
        ui.add_space(18.0);
    }

    fn render_overview(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                Self::page_header(
                    ui,
                    "My Specs",
                    "Your PC at a glance. Hardware details are read-only.",
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.operation_running(),
                        egui::Button::new(RichText::new("Refresh").color(GREEN)),
                    )
                    .clicked()
                {
                    self.inventory = inventory::collect();
                }
            });
        });

        self.section_card(ui, "LibreHardwareMonitor sensors", |ui| {
            if self.inventory.monitored_hardware.is_empty() {
                let message = self
                    .inventory
                    .hardware_error
                    .as_deref()
                    .unwrap_or("LibreHardwareMonitor did not report any hardware readings.");
                ui.label(RichText::new(message).color(MUTED));
            } else {
                for hardware in &self.inventory.monitored_hardware {
                    render_monitored_hardware(ui, hardware);
                }
            }
        });
    }

    fn render_cleanup(&mut self, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Cache Cleaner",
            "Scan safe temporary folders, review the size, then clean all listed files with one click.",
        );
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.operation_running(),
                    egui::Button::new(RichText::new("Scan for cache").color(GREEN)),
                )
                .clicked()
            {
                self.scan_cleanup();
            }
        });
        if let Some(notice) = &self.cleanup_notice {
            ui.label(RichText::new(notice).color(MUTED));
        }
        if self.cleanup_scanned {
            let cleanable_bytes: u64 = self.cleanup_items.iter().map(|item| item.size_bytes).sum();
            let cleanable_mb = cleanable_bytes as f64 / 1_000_000.0;
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Potentially cleanable").color(MUTED));
                    ui.label(
                        RichText::new(format!("{cleanable_mb:.1} MB"))
                            .size(34.0)
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(format!("{} temporary files", self.cleanup_items.len()))
                            .color(MUTED),
                    );
                });
                ui.add_space(24.0);
                if cleanable_bytes < 100_000_000 {
                    draw_success_check(
                        ui,
                        egui::Id::new(("cache-clean", self.cleanup_animation_id)),
                    );
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("No need to clean")
                                .size(17.0)
                                .strong()
                                .color(GREEN),
                        );
                        ui.label(RichText::new("Cache is already clean enough").color(MUTED));
                    });
                } else if ui
                    .add_enabled(
                        !self.operation_running(),
                        egui::Button::new(
                            RichText::new("Clean cache").size(17.0).strong().color(BG),
                        )
                        .fill(GREEN)
                        .min_size(egui::vec2(220.0, 60.0)),
                    )
                    .clicked()
                {
                    self.start_cleanup(self.cleanup_items.clone());
                }
            });
        }
        if let Some((completed, total)) = self.cleanup_progress {
            let (fraction, label) = match total {
                Some(total) => (
                    if total == 0 {
                        1.0
                    } else {
                        completed as f32 / total as f32
                    },
                    format!("{completed}/{total} files processed"),
                ),
                None => (0.0, format!("{completed} entries scanned")),
            };
            ui.add(
                egui::ProgressBar::new(fraction)
                    .animate(total.is_none())
                    .fill(GREEN)
                    .text(label),
            );
        }
        if let Some(result) = &self.cleanup_result {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if result.deleted > 0 {
                    draw_success_check(
                        ui,
                        egui::Id::new(("cache-result", self.cleanup_animation_id)),
                    );
                }
                ui.vertical(|ui| {
                    let result_title = if result.deleted > 0 {
                        format!(
                            "Cleaned {:.1} MB",
                            result.bytes_removed as f64 / 1_000_000.0
                        )
                    } else {
                        "No files were cleaned".to_owned()
                    };
                    ui.label(RichText::new(result_title).size(20.0).strong().color(
                        if result.deleted > 0 {
                            GREEN
                        } else {
                            Color32::YELLOW
                        },
                    ));
                    ui.label(
                        RichText::new(format!("{} files removed", result.deleted)).color(MUTED),
                    );
                    if !result.errors.is_empty() {
                        ui.label(
                            RichText::new(format!(
                                "{} files could not be removed",
                                result.errors.len()
                            ))
                            .color(MUTED),
                        );
                    }
                });
            });
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .max_height(430.0)
            .show(ui, |ui| {
                for item in &mut self.cleanup_items {
                    let modified = age_label(item.modified);
                    ui.horizontal(|ui| {
                        draw_nav_icon(ui.painter(), Page::Cleanup, ui.cursor().center(), MUTED);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(item.path.display().to_string())
                                    .color(TEXT)
                                    .size(13.0),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{}  ·  modified {modified}",
                                    inventory::format_bytes(item.size_bytes)
                                ))
                                .small()
                                .color(MUTED),
                            );
                        });
                    });
                    ui.separator();
                }
            });
    }

    fn render_empty_folders(&mut self, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Empty folder review",
            "Nothing is scanned automatically. Start a scan when you're ready, then review folders before removal.",
        );
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.operation_running(),
                    egui::Button::new("Scan for empty folders"),
                )
                .clicked()
            {
                self.scan_empty_folders();
            }
            if !self.empty_items.is_empty() {
                if ui
                    .add_enabled(!self.operation_running(), egui::Button::new("Select all"))
                    .clicked()
                {
                    for item in &mut self.empty_items {
                        item.selected = true;
                    }
                }
                if ui
                    .add_enabled(!self.operation_running(), egui::Button::new("Select none"))
                    .clicked()
                {
                    for item in &mut self.empty_items {
                        item.selected = false;
                    }
                }
                let selected_count = self.empty_items.iter().filter(|item| item.selected).count();
                if ui
                    .add_enabled(
                        selected_count > 0 && !self.operation_running(),
                        egui::Button::new(
                            RichText::new(format!("Review removal ({selected_count})"))
                                .color(Color32::from_rgb(252, 211, 77)),
                        ),
                    )
                    .clicked()
                {
                    self.pending_action = Some(PendingAction::RemoveEmptyFolders(
                        self.local_volume_roots(),
                        self.empty_items.clone(),
                    ));
                }
            }
        });
        if let Some(notice) = &self.empty_notice {
            ui.label(RichText::new(notice).color(MUTED));
        }
        if self.empty_scanned && !self.empty_scan_truncated && self.empty_items.len() < 10 {
            ui.label(
                RichText::new(format!(
                    "No need to clean — only {} empty folders found.",
                    self.empty_items.len()
                ))
                .strong()
                .color(GREEN),
            );
        }
        if let Some((root_index, total_roots, root, visited)) = &self.empty_progress {
            let fraction = if root.as_os_str().is_empty() {
                if *total_roots == 0 {
                    1.0
                } else {
                    *root_index as f32 / *total_roots as f32
                }
            } else {
                (*root_index as f32 + (*visited as f32 / 10_000.0).clamp(0.0, 0.99))
                    / (*total_roots).max(1) as f32
            };
            let label = if root.as_os_str().is_empty() {
                format!("{root_index}/{total_roots} folders processed")
            } else {
                format!(
                    "Drive {}/{} · {} entries · {}",
                    root_index + 1,
                    total_roots,
                    visited,
                    root.display()
                )
            };
            ui.add(
                egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                    .animate(!root.as_os_str().is_empty())
                    .fill(GREEN)
                    .text(label),
            );
        }
        if self.empty_scanned && self.empty_items.is_empty() {
            ui.label(RichText::new("No empty folders were found.").color(GREEN));
        }
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .show(ui, |ui| {
                for item in &mut self.empty_items {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut item.selected, "");
                        ui.label(RichText::new(item.path.display().to_string()).color(TEXT));
                    });
                    ui.separator();
                }
            });
    }

    fn render_network(&mut self, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Network tools",
            "DNS cache is separate from saved network profiles; Wi-Fi history is never removed.",
        );
        let mut confirm_flush = false;
        self.section_card(ui, "DNS cache", |ui| {
            ui.label(
                RichText::new("Flush the Windows DNS cache with the system ipconfig utility.")
                    .color(MUTED),
            );
            ui.add_space(8.0);
            if ui
                .add_enabled(
                    !self.operation_running(),
                    egui::Button::new(RichText::new("Review DNS cache flush").color(GREEN)),
                )
                .clicked()
            {
                confirm_flush = true;
            }
        });
        if confirm_flush {
            self.pending_action = Some(PendingAction::FlushDns);
        }
    }

    fn render_about(&self, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "About TaraTari Optimizer App",
            "A native Windows utility from TaraTariTools.",
        );
        self.section_card(ui, "Project", |ui| {
            ui.label(
                RichText::new("Version 0.0.1 · Windows x64 and x86")
                    .strong()
                    .color(GREEN),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Hardware readings are provided by LibreHardwareMonitor. Cleanup and system actions require your review.",
                )
                .color(MUTED),
            );
            ui.add_space(14.0);
            ui.hyperlink_to(
                "TaraTari Optimizer App on GitHub",
                "https://github.com/tarataritools/TaraTariOptimizerApp",
            );
            ui.hyperlink_to("TaraTariTools website", "https://tarataritools.github.io");
            ui.hyperlink_to("TaraTariTools on GitHub", "https://github.com/tarataritools");
        });
    }

    fn render_confirmation(&mut self, context: &egui::Context) {
        let Some(action) = self.pending_action.clone() else {
            return;
        };
        let (title, description, details) = match &action {
            PendingAction::RemoveEmptyFolders(_, items) => (
                "Remove selected empty folders?",
                "Each folder is checked again. Non-empty folders, links, and items outside the scanned local volumes are refused.",
                format!("{} folders selected", items.iter().filter(|item| item.selected).count()),
            ),
            PendingAction::FlushDns => (
                "Flush the DNS resolver cache?",
                "Windows will discard cached DNS lookups; it will not delete saved Wi-Fi profiles or connection history.",
                "Action: ipconfig /flushdns".to_owned(),
            ),
        };

        egui::Window::new(title)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(500.0)
            .frame(
                egui::Frame::window(&context.style()).corner_radius(egui::CornerRadius::same(14)),
            )
            .show(context, |ui| {
                ui.label(RichText::new(description).color(MUTED));
                ui.add_space(10.0);
                ui.label(RichText::new(details).color(TEXT));
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.pending_action = None;
                    }
                    if ui
                        .add_enabled(
                            !self.operation_running(),
                            egui::Button::new(
                                RichText::new("Confirm action")
                                    .color(Color32::from_rgb(252, 211, 77)),
                            ),
                        )
                        .clicked()
                    {
                        self.execute_action(action.clone());
                        self.pending_action = None;
                    }
                });
            });
    }

    fn execute_action(&mut self, action: PendingAction) {
        if self.operation_running() {
            self.action_notice = Some(busy_action_message().to_owned());
            return;
        }
        match action {
            PendingAction::RemoveEmptyFolders(roots, items) => {
                self.remove_empty_folders(roots, items);
            }
            PendingAction::FlushDns => match Command::new("ipconfig").arg("/flushdns").output() {
                Ok(output) if output.status.success() => {
                    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                    self.action_notice = Some(if text.is_empty() {
                        "Windows DNS cache flush completed.".to_owned()
                    } else {
                        text
                    });
                }
                Ok(output) => {
                    self.action_notice = Some(format!(
                        "DNS cache flush failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
                Err(error) => self.action_notice = Some(format!("Could not run ipconfig: {error}")),
            },
        }
    }
}

impl eframe::App for OptimizerApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        if self.memory_receiver.is_some()
            || self.cleanup_receiver.is_some()
            || self.empty_receiver.is_some()
            || self.storage_receiver.is_some()
        {
            context.request_repaint_after(Duration::from_millis(100));
        }
        self.poll_memory();
        self.poll_cleanup();
        self.poll_empty_folders();
        self.poll_storage();
        egui::SidePanel::left("navigation")
            .resizable(false)
            .exact_width(256.0)
            .frame(
                egui::Frame::side_top_panel(&context.style())
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(13, 16)),
            )
            .show(context, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("TaraTari")
                            .font(egui::FontId::new(19.0, egui::FontFamily::Monospace))
                            .strong()
                            .color(GREEN),
                    );
                    ui.label(
                        RichText::new("Optimizer App")
                            .size(14.0)
                            .strong()
                            .color(TEXT),
                    );
                });
                ui.add_space(18.0);
                self.show_nav_item(
                    ui,
                    Page::Overview,
                    "My Specs",
                    "Hardware inventory and storage",
                );
                self.show_nav_item(
                    ui,
                    Page::Memory,
                    "MemReduct",
                    "Temporarily reclaim app memory",
                );
                self.show_nav_item(
                    ui,
                    Page::Cleanup,
                    "Cache Cleaner",
                    "Review stale temporary files before deletion",
                );
                self.show_nav_item(
                    ui,
                    Page::EmptyFolders,
                    "Empty folders",
                    "Choose, scan and review empty folders",
                );
                self.show_nav_item(
                    ui,
                    Page::Storage,
                    "Who's fat?",
                    "Analyze what is using drive space",
                );
                self.show_nav_item(ui, Page::Network, "DNS cache", "Flush cached DNS lookups");
                self.show_nav_item(
                    ui,
                    Page::About,
                    "About",
                    "About this app and its project links",
                );
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(
                        RichText::new("No changes without your review.")
                            .small()
                            .color(MUTED),
                    );
                    ui.label(RichText::new("Windows · 0.0.1").small().color(MUTED));
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&context.style())
                    .fill(BG)
                    .inner_margin(28.0),
            )
            .show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.operation_running() {
                        ui.label(
                            RichText::new(busy_action_message())
                                .strong()
                                .color(Color32::YELLOW),
                        );
                        ui.add_space(10.0);
                    }
                    match self.page {
                        Page::Overview => self.render_overview(ui),
                        Page::Cleanup => self.render_cleanup(ui),
                        Page::EmptyFolders => self.render_empty_folders(ui),
                        Page::Storage => self.render_storage(ui),
                        Page::Memory => self.render_memory(ui),
                        Page::Network => self.render_network(ui),
                        Page::About => self.render_about(ui),
                    }
                });
            });

        if let Some(notice) = self.action_notice.clone() {
            egui::Window::new("Action result")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(true)
                .default_width(440.0)
                .show(context, |ui| {
                    ui.label(RichText::new(&notice).color(TEXT));
                    if ui.button("Close").clicked() {
                        self.action_notice = None;
                    }
                });
        }
        self.render_confirmation(context);
    }
}

fn draw_nav_icon(painter: &egui::Painter, page: Page, center: egui::Pos2, color: Color32) {
    let stroke = Stroke::new(1.5_f32, color);
    let line = |from: egui::Pos2, to: egui::Pos2| painter.line_segment([from, to], stroke);
    match page {
        Page::Overview => {
            painter.circle_stroke(center, 7.0, stroke);
            line(center, center + egui::vec2(4.0, -4.0));
            painter.circle_filled(center, 1.5, color);
        }
        Page::Cleanup => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(12.0, 12.0)),
                egui::CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            line(
                center + egui::vec2(-7.0, -8.0),
                center + egui::vec2(7.0, -8.0),
            );
            line(
                center + egui::vec2(-3.0, -3.0),
                center + egui::vec2(-3.0, 4.0),
            );
            line(center, center + egui::vec2(0.0, 4.0));
            line(
                center + egui::vec2(3.0, -3.0),
                center + egui::vec2(3.0, 4.0),
            );
        }
        Page::EmptyFolders => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center + egui::vec2(-2.0, 2.0), egui::vec2(9.0, 9.0)),
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_center_size(center + egui::vec2(3.0, -3.0), egui::vec2(9.0, 9.0)),
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Page::Storage => {
            painter.circle_stroke(center + egui::vec2(-1.0, -1.0), 6.0, stroke);
            line(center + egui::vec2(4.0, 4.0), center + egui::vec2(8.0, 8.0));
            for (x, height) in [(-4.0, 4.0), (0.0, 7.0), (4.0, 5.0)] {
                line(
                    center + egui::vec2(x, 3.0),
                    center + egui::vec2(x, 3.0 - height),
                );
            }
        }
        Page::Memory => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(12.0, 10.0)),
                egui::CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            for x in [-4.0, 0.0, 4.0] {
                line(center + egui::vec2(x, -8.0), center + egui::vec2(x, -5.0));
                line(center + egui::vec2(x, 5.0), center + egui::vec2(x, 8.0));
            }
        }
        Page::Network => {
            let left = center + egui::vec2(-6.0, 0.0);
            let right = center + egui::vec2(6.0, -5.0);
            let bottom = center + egui::vec2(6.0, 5.0);
            line(left, right);
            line(left, bottom);
            painter.circle_stroke(left, 2.0, stroke);
            painter.circle_stroke(right, 2.0, stroke);
            painter.circle_stroke(bottom, 2.0, stroke);
        }
        Page::About => {
            painter.circle_stroke(center, 7.0, stroke);
            painter.circle_filled(center + egui::vec2(0.0, -3.0), 1.0, color);
            line(center + egui::vec2(0.0, 0.0), center + egui::vec2(0.0, 4.0));
        }
    }
}

fn storage_row(ui: &mut egui::Ui, path: &Path, size_bytes: u64, total_bytes: u64) {
    ui.label(
        RichText::new(path.display().to_string())
            .size(12.0)
            .color(TEXT),
    );
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(inventory::format_bytes(size_bytes))
                .strong()
                .color(GREEN),
        );
        if total_bytes > 0 {
            ui.label(
                RichText::new(format!(
                    "{:.1}%",
                    size_bytes as f64 * 100.0 / total_bytes as f64
                ))
                .small()
                .color(MUTED),
            );
        }
    });
    let fraction = if total_bytes == 0 {
        0.0
    } else {
        size_bytes as f32 / total_bytes as f32
    };
    ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).fill(GREEN));
    ui.add_space(6.0);
}

fn render_storage_treemap(ui: &mut egui::Ui, report: &StorageReport) {
    let mut groups = report
        .largest_folders
        .iter()
        .take(11)
        .map(|entry| {
            let name = entry
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| entry.path.display().to_string());
            (name, entry.size_bytes)
        })
        .collect::<Vec<_>>();
    let shown_bytes = groups.iter().map(|(_, size)| *size).sum::<u64>();
    let remaining = report.total_file_bytes.saturating_sub(shown_bytes);
    if remaining > 0 {
        groups.push(("Other files".to_owned(), remaining));
    }
    if groups.is_empty() || report.total_file_bytes == 0 {
        ui.label(RichText::new("No readable file data to visualize.").color(MUTED));
        return;
    }

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 260.0),
        egui::Sense::hover(),
    );
    let mut tiles = Vec::new();
    layout_treemap(&groups, 0, groups.len(), rect.shrink(3.0), &mut tiles);
    let palette = [
        Color32::from_rgb(34, 197, 94),
        Color32::from_rgb(22, 163, 74),
        Color32::from_rgb(16, 185, 129),
        Color32::from_rgb(5, 150, 105),
        Color32::from_rgb(132, 204, 22),
        Color32::from_rgb(101, 163, 13),
        Color32::from_rgb(20, 184, 166),
        Color32::from_rgb(13, 148, 136),
        Color32::from_rgb(74, 222, 128),
        Color32::from_rgb(21, 128, 61),
        Color32::from_rgb(52, 211, 153),
        Color32::from_rgb(63, 98, 18),
    ];
    for (index, tile) in tiles {
        let tile_rect = tile.shrink(2.0);
        let fill = palette[index % palette.len()];
        ui.painter()
            .rect_filled(tile_rect, egui::CornerRadius::same(7), fill);
        ui.painter().rect_stroke(
            tile_rect,
            egui::CornerRadius::same(7),
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 70)),
            egui::StrokeKind::Inside,
        );
        if tile_rect.width() > 64.0 && tile_rect.height() > 38.0 {
            let (name, bytes) = &groups[index];
            ui.painter().text(
                tile_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{name}\n{}", inventory::format_bytes(*bytes)),
                egui::FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
        let response = ui.interact(
            tile_rect,
            egui::Id::new(("storage-tile", index)),
            egui::Sense::hover(),
        );
        if response.hovered() {
            let (name, bytes) = &groups[index];
            response.on_hover_text(format!(
                "{name}: {} ({:.1}% of readable file data)",
                inventory::format_bytes(*bytes),
                *bytes as f64 * 100.0 / report.total_file_bytes.max(1) as f64
            ));
        }
    }
}

fn layout_treemap(
    groups: &[(String, u64)],
    start: usize,
    end: usize,
    rect: egui::Rect,
    result: &mut Vec<(usize, egui::Rect)>,
) {
    if start >= end {
        return;
    }
    if end - start == 1 {
        result.push((start, rect));
        return;
    }

    let remaining_weight = groups[start..end]
        .iter()
        .map(|(_, size)| *size)
        .sum::<u64>()
        .max(1);
    let target_weight = remaining_weight / 2;
    let mut split = start + 1;
    let mut left_weight = groups[start].1;
    while split < end - 1 && left_weight + groups[split].1 <= target_weight.max(groups[start].1) {
        left_weight += groups[split].1;
        split += 1;
    }
    let ratio = (left_weight as f32 / remaining_weight as f32).clamp(0.08, 0.92);
    if rect.width() >= rect.height() {
        let split_x = rect.left() + rect.width() * ratio;
        layout_treemap(
            groups,
            start,
            split,
            egui::Rect::from_min_max(rect.min, egui::pos2(split_x, rect.bottom())),
            result,
        );
        layout_treemap(
            groups,
            split,
            end,
            egui::Rect::from_min_max(egui::pos2(split_x, rect.top()), rect.max),
            result,
        );
    } else {
        let split_y = rect.top() + rect.height() * ratio;
        layout_treemap(
            groups,
            start,
            split,
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), split_y)),
            result,
        );
        layout_treemap(
            groups,
            split,
            end,
            egui::Rect::from_min_max(egui::pos2(rect.left(), split_y), rect.max),
            result,
        );
    }
}

fn render_monitored_hardware(ui: &mut egui::Ui, hardware: &inventory::MonitoredHardware) {
    egui::CollapsingHeader::new(format!("{} · {}", hardware.name, hardware.hardware_type))
        .default_open(true)
        .show(ui, |ui| {
            for sensor in &hardware.sensors {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&sensor.name).color(MUTED));
                    ui.label(
                        RichText::new(format!("{:.1} ({})", sensor.value, sensor.sensor_type))
                            .strong()
                            .color(TEXT),
                    );
                });
            }
            for child in &hardware.sub_hardware {
                render_monitored_hardware(ui, child);
            }
        });
}

fn draw_success_check(ui: &mut egui::Ui, id: egui::Id) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
    let progress = ui
        .ctx()
        .animate_value_with_time(id, 1.0, 0.36)
        .clamp(0.0, 1.0);
    if progress < 1.0 {
        ui.ctx().request_repaint();
    }
    let center = rect.center();
    ui.painter().circle_filled(
        center,
        15.0 * progress,
        Color32::from_rgba_unmultiplied(74, 222, 128, (230.0 * progress) as u8),
    );
    if progress > 0.3 {
        let check_progress = ((progress - 0.3) / 0.7).clamp(0.0, 1.0);
        let stroke = Stroke::new(2.5_f32, BG);
        let start = center + egui::vec2(-6.0, 0.0);
        let middle = center + egui::vec2(-2.0, 4.0);
        let end = center + egui::vec2(7.0, -6.0);
        ui.painter()
            .line_segment([start, start.lerp(middle, check_progress)], stroke);
        if check_progress > 0.5 {
            ui.painter().line_segment(
                [middle, middle.lerp(end, (check_progress - 0.5) * 2.0)],
                stroke,
            );
        }
    }
}

fn is_local_volume_root(path: &std::path::Path) -> bool {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    bytes.len() == 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "The background task panicked before it could finish.".to_owned()
    }
}

fn scan_summary<T>(report: &ScanReport<T>) -> String {
    let truncation = if report.truncated {
        " The safety limit was reached; the scan or candidate list may be incomplete."
    } else {
        ""
    };
    format!(
        "Found {} candidates. {} entries were skipped due to access or metadata errors.{}",
        report.items.len(),
        report.skipped_entries,
        truncation
    )
}

fn age_label(modified: SystemTime) -> String {
    match SystemTime::now().duration_since(modified) {
        Ok(age) => format!("{} days ago", age.as_secs() / (24 * 60 * 60)),
        Err(_) => "just now".to_owned(),
    }
}

fn format_action_result(noun: &str, changed: usize, errors: Vec<String>) -> String {
    if errors.is_empty() {
        format!("{changed} {noun}.")
    } else {
        format!(
            "{changed} {noun}; {} items were skipped because access was denied or they changed during the scan. No permissions were changed.",
            errors.len()
        )
    }
}

fn busy_action_message() -> &'static str {
    "Another scan or action is running. Finish that first before starting another."
}
