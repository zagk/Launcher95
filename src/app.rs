use crate::{
    data::{
        assign_entries_to_group, load_entries, load_settings, make_entry, save_entries,
        save_settings, AppEntry, EntrySource, Settings,
    },
    icons::IconCache,
    theme,
};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Key, Layout, Margin, RichText,
    Sense, Stroke,
};
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy)]
enum TitleButton {
    Minimize,
    Maximize,
    Restore,
    Close,
}

#[derive(Clone, Copy)]
enum MenuAction {
    Add,
    NewGroup,
    Find,
    Exit,
    Rename,
    Delete,
    CleanMissing,
    ClearSearch,
    OpenData,
    Settings,
    About,
}

pub struct Launcher95App {
    data_dir: PathBuf,
    entries: Vec<AppEntry>,
    settings: Settings,
    search: String,
    group_filter: String,
    status: String,
    show_settings: bool,
    show_about: bool,
    show_search: bool,
    show_group_dialog: bool,
    group_name: String,
    group_members: HashSet<String>,
    focus_group_name: bool,
    hotkey_text: String,
    hotkey_manager: Option<GlobalHotKeyManager>,
    hotkey: Option<HotKey>,
    hotkey_listener_started: bool,
    hotkey_id: Arc<AtomicU32>,
    focus_request: Arc<AtomicBool>,
    dragging_id: Option<String>,
    selected_id: Option<String>,
    edit_id: Option<String>,
    edit_name: String,
    focus_rename: bool,
    focus_search: bool,
    was_focused: bool,
    was_minimized: bool,
    blur_guard: Option<Instant>,
    icons: IconCache,
    tile_rects: Vec<(String, egui::Rect)>,
    applied_always_on_top: Option<bool>,
    settings_dirty: bool,
    last_settings_save: Instant,
    last_launch: Option<(String, Instant)>,
    exe_path: String,
}

impl Launcher95App {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: PathBuf) -> Self {
        let mut app = Self::new_with_context(&cc.egui_ctx, data_dir);
        app.register_hotkey();
        app
    }

    fn new_with_context(ctx: &egui::Context, data_dir: PathBuf) -> Self {
        theme::apply(ctx);
        let settings = load_settings(&data_dir);
        let entries = load_entries(&data_dir);
        let exe_path = std::env::current_exe()
            .ok()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
        let app = Self {
            icons: IconCache::new(&data_dir),
            hotkey_text: settings.hotkey.clone(),
            settings,
            data_dir,
            search: String::new(),
            group_filter: String::new(),
            status: "Ready".into(),
            show_settings: false,
            show_about: false,
            show_search: false,
            show_group_dialog: false,
            group_name: String::new(),
            group_members: HashSet::new(),
            focus_group_name: false,
            hotkey_manager: GlobalHotKeyManager::new().ok(),
            hotkey: None,
            hotkey_listener_started: false,
            hotkey_id: Arc::new(AtomicU32::new(u32::MAX)),
            focus_request: Arc::new(AtomicBool::new(false)),
            dragging_id: None,
            selected_id: None,
            edit_id: None,
            edit_name: String::new(),
            focus_rename: false,
            focus_search: false,
            was_focused: false,
            was_minimized: false,
            blur_guard: None,
            tile_rects: Vec::new(),
            applied_always_on_top: None,
            settings_dirty: false,
            last_settings_save: Instant::now(),
            last_launch: None,
            exe_path,
            entries,
        };
        app
    }

    fn persist_entries(&mut self) {
        match save_entries(&self.data_dir, &self.entries) {
            Ok(()) => self.status = format!("{} programs", self.entries.len()),
            Err(error) => self.status = format!("Save failed: {error}"),
        }
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        let mut added = 0usize;
        let mut duplicates = 0usize;
        let mut rejected = 0usize;
        let mut last_added: Option<String> = None;

        for path in paths {
            match make_entry(&path, EntrySource::Manual) {
                Some(entry) => {
                    if self
                        .entries
                        .iter()
                        .any(|existing| existing.path.eq_ignore_ascii_case(&entry.path))
                    {
                        duplicates += 1;
                    } else {
                        last_added = Some(entry.name.clone());
                        self.entries.push(entry);
                        added += 1;
                    }
                }
                None => rejected += 1,
            }
        }

        if added > 0 {
            self.search.clear();
            self.group_filter.clear();
            if let Some(name) = last_added {
                self.selected_id = self
                    .entries
                    .iter()
                    .find(|entry| entry.name == name)
                    .map(|entry| entry.id.clone());
            }
            self.persist_entries();
            self.status = format!("Added {added} program{}", plural(added));
        } else if duplicates > 0 {
            self.status = format!("{duplicates} item{} already registered", plural(duplicates));
        } else if rejected > 0 {
            self.status = "Supported files: EXE, LNK, URL".into();
        }
    }

    fn add_dialog(&mut self) {
        // The native file dialog steals focus; do not treat that as "clicked outside".
        self.hold_blur(1500);
        if let Some(files) = rfd::FileDialog::new()
            .add_filter("Programs and shortcuts", &["exe", "lnk", "url"])
            .pick_files()
        {
            self.add_paths(files);
        }
        self.hold_blur(600);
    }

    fn launch(path: &str) -> Result<(), String> {
        let path_obj = Path::new(path);
        if !path_obj.exists() {
            return Err(format!("File not found: {path}"));
        }
        let extension = path_obj
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let spawn = if extension == "exe" {
            Command::new(path_obj).spawn()
        } else {
            let mut command = Command::new("cmd");
            command.args(["/C", "start", "", path]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(CREATE_NO_WINDOW);
            }
            command.spawn()
        };

        spawn.map(|_| ()).map_err(|error| error.to_string())
    }

    fn launch_entry(&mut self, entry: &AppEntry) {
        let now = Instant::now();
        let should_launch = self
            .last_launch
            .as_ref()
            .map(|(id, when)| id != &entry.id || when.elapsed() > Duration::from_millis(600))
            .unwrap_or(true);
        if !should_launch {
            return;
        }
        self.last_launch = Some((entry.id.clone(), now));
        // Launching steals the focus from us, but that is a click *inside* the
        // window, not outside it. Do not minimize for this focus loss.
        self.hold_blur(1200);

        match Self::launch(&entry.path) {
            Ok(()) => self.status = format!("Launched {}", entry.name),
            Err(error) => self.status = format!("Launch failed: {error}"),
        }
    }

    fn open_location(path: &str) {
        let _ = Command::new("explorer.exe")
            .args(["/select,", path])
            .spawn();
    }

    fn open_dir(path: &Path) {
        let _ = Command::new("explorer.exe").arg(path).spawn();
    }

    fn delete_entry(&mut self, id: &str) {
        self.entries.retain(|entry| entry.id != id);
        if self.selected_id.as_deref() == Some(id) {
            self.selected_id = None;
        }
        if self.edit_id.as_deref() == Some(id) {
            self.edit_id = None;
        }
        self.persist_entries();
    }

    fn begin_rename(&mut self) {
        let Some(id) = self.selected_id.clone() else {
            self.status = "Select a program first".into();
            return;
        };
        if let Some(entry) = self.entries.iter().find(|entry| entry.id == id) {
            self.edit_id = Some(id);
            self.edit_name = entry.name.clone();
            self.focus_rename = true;
        }
    }

    fn remove_missing(&mut self) {
        let before = self.entries.len();
        self.entries
            .retain(|entry| Path::new(&entry.path).exists());
        let removed = before - self.entries.len();
        if removed > 0 {
            self.persist_entries();
        }
        self.status = format!("Removed {removed} missing entr{}", if removed == 1 { "y" } else { "ies" });
    }

    fn groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = self
            .entries
            .iter()
            .map(|entry| entry.group.clone())
            .filter(|group| !group.is_empty())
            .collect();
        groups.sort_by_key(|group| group.to_ascii_lowercase());
        groups.dedup();
        groups
    }

    fn assign_group(&mut self, source_id: &str, target_id: &str) {
        if source_id == target_id {
            return;
        }
        let target_group = self
            .entries
            .iter()
            .find(|entry| entry.id == target_id)
            .map(|entry| {
                if entry.group.is_empty() {
                    entry.name.clone()
                } else {
                    entry.group.clone()
                }
            });
        let Some(group) = target_group else {
            return;
        };

        for entry in &mut self.entries {
            if entry.id == source_id || entry.id == target_id {
                entry.group = group.clone();
            }
        }
        self.group_filter.clear();
        self.persist_entries();
        self.status = format!("Group created: {group}");
    }

    fn begin_group(&mut self, seed: Option<&str>) {
        self.group_name.clear();
        self.group_members.clear();
        if let Some(id) = seed {
            self.group_members.insert(id.to_owned());
        }
        self.focus_group_name = true;
        self.show_group_dialog = true;
    }

    fn create_group(&mut self) {
        let name = self.group_name.trim().to_owned();
        if name.is_empty() {
            self.status = "Group name is empty".into();
            return;
        }
        if self.group_members.is_empty() {
            self.status = "Select at least one program".into();
            return;
        }

        let members = self.group_members.clone();
        let moved = assign_entries_to_group(&mut self.entries, &members, &name);
        if moved == 0 {
            self.status = "Select at least one program".into();
            return;
        }

        self.group_filter = name.clone();
        self.persist_entries();
        self.show_group_dialog = false;
        self.status = format!(
            "Group '{name}' has {moved} program{}",
            plural(moved)
        );
    }

    fn register_hotkey(&mut self) {
        if let (Some(manager), Some(old)) = (&self.hotkey_manager, self.hotkey.take()) {
            let _ = manager.unregister(old);
        }
        let Ok(key) = HotKey::from_str(&self.settings.hotkey) else {
            self.status = "Invalid shortcut. Example: Alt+Space".into();
            return;
        };
        if let Some(manager) = &self.hotkey_manager {
            match manager.register(key) {
                Ok(()) => {
                    self.hotkey = Some(key);
                    self.hotkey_id.store(key.id(), Ordering::Relaxed);
                    if self.status.starts_with("Shortcut") {
                        self.status = "Ready".into();
                    }
                }
                Err(error) => self.status = format!("Shortcut registration failed: {error}"),
            }
        } else {
            self.status = "Global shortcut unavailable".into();
        }
    }

    /// Spawns a dedicated thread that owns the global-hotkey receiver.
    ///
    /// The egui update loop stops while the window is minimized, so toggling the
    /// window from inside `update` is unreliable. The listener thread only uses
    /// the Win32 handle and works no matter what state the window is in.
    fn ensure_hotkey_listener(&mut self, frame: &eframe::Frame) {
        if self.hotkey_listener_started {
            return;
        }
        let Some(hotkey) = self.hotkey else {
            return;
        };

        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let Ok(handle) = frame.window_handle() else {
                return;
            };
            let RawWindowHandle::Win32(win32) = handle.as_raw() else {
                return;
            };
            let hwnd = win32.hwnd.get();
            self.hotkey_id.store(hotkey.id(), Ordering::Relaxed);
            let hotkey_id = self.hotkey_id.clone();
            let focus_request = self.focus_request.clone();
            std::thread::spawn(move || hotkey_listener(hotkey_id, hwnd, focus_request));
            self.hotkey_listener_started = true;
        }

        #[cfg(not(windows))]
        {
            let _ = (hotkey, frame);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, Key::F)) {
            self.show_search = true;
            self.focus_search = true;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, Key::O)) {
            self.add_dialog();
        }
        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            if !self.search.is_empty() {
                self.search.clear();
            } else {
                self.show_settings = false;
                self.show_about = false;
                self.show_search = false;
                self.edit_id = None;
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_paths(dropped);
        }
    }

    fn sync_window_geometry(&mut self, ctx: &egui::Context) {
        if !self.settings.remember_window_position {
            return;
        }
        let (inner, outer, maximized, minimized) = ctx.input(|input| {
            let viewport = input.viewport();
            (
                viewport.inner_rect,
                viewport.outer_rect,
                viewport.maximized.unwrap_or(false),
                viewport.minimized.unwrap_or(false),
            )
        });
        if maximized || minimized {
            return;
        }

        let mut changed = false;
        if let Some(rect) = inner {
            let width = rect.width();
            let height = rect.height();
            if (width - self.settings.window_width).abs() > 0.5 {
                self.settings.window_width = width;
                changed = true;
            }
            if (height - self.settings.window_height).abs() > 0.5 {
                self.settings.window_height = height;
                changed = true;
            }
        }
        if let Some(rect) = outer {
            let x = rect.min.x;
            let y = rect.min.y;
            if self
                .settings
                .window_x
                .map(|old| (old - x).abs() > 0.5)
                .unwrap_or(true)
            {
                self.settings.window_x = Some(x);
                changed = true;
            }
            if self
                .settings
                .window_y
                .map(|old| (old - y).abs() > 0.5)
                .unwrap_or(true)
            {
                self.settings.window_y = Some(y);
                changed = true;
            }
        }
        if changed {
            self.settings_dirty = true;
        }
    }

    fn apply_window_level(&mut self, ctx: &egui::Context) {
        if self.applied_always_on_top != Some(self.settings.always_on_top) {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                if self.settings.always_on_top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                },
            ));
            self.applied_always_on_top = Some(self.settings.always_on_top);
        }
    }

    /// Suppresses the next focus loss, e.g. while a native file dialog is open
    /// or right after the window was restored by the global hotkey.
    fn hold_blur(&mut self, millis: u64) {
        self.blur_guard = Some(Instant::now() + Duration::from_millis(millis));
    }

    fn blur_is_held(&self) -> bool {
        self.blur_guard
            .map(|until| Instant::now() < until)
            .unwrap_or(false)
    }

    /// Minimizes the launcher when it loses focus, like a classic tray launcher.
    fn handle_blur(&mut self, ctx: &egui::Context) {
        let focused = ctx
            .input(|input| input.viewport().focused)
            .unwrap_or(false);
        let minimized = ctx
            .input(|input| input.viewport().minimized)
            .unwrap_or(false);

        if minimized {
            self.was_focused = false;
            self.was_minimized = true;
            return;
        }
        // Restoring from the taskbar delivers a frame where the window is no
        // longer minimized but not focused yet; do not bounce straight back.
        if self.was_minimized {
            self.was_minimized = false;
            self.hold_blur(600);
        }
        if !self.settings.minimize_on_blur {
            self.was_focused = focused;
            return;
        }

        let lost_focus = self.was_focused && !focused;
        if lost_focus && !self.blur_is_held() {
            // A drag that started inside the window must keep working; only
            // minimize once the mouse button has been released.
            if !mouse_button_down() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                self.status = "Minimized".into();
            } else {
                self.was_focused = true; // re-check next frame
                return;
            }
        }
        self.was_focused = focused;
    }

    fn autosave(&mut self) {
        if self.settings_dirty && self.last_settings_save.elapsed() > Duration::from_millis(800) {
            let _ = save_settings(&self.data_dir, &self.settings);
            self.settings_dirty = false;
            self.last_settings_save = Instant::now();
        }
    }

    fn title_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut start_drag = false;
        let mut toggle_maximize = false;
        let mut minimize = false;
        let mut close = false;
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        let exe_path = self.exe_path.clone();

        ui.horizontal(|ui| {
            if !exe_path.is_empty() {
                if let Some(texture) = self.icons.texture(ui.ctx(), &exe_path, 32) {
                    ui.add(egui::Image::new((texture.id(), egui::vec2(16.0, 16.0))));
                } else {
                    ui.add_space(16.0);
                }
            }

            let buttons_width = 3.0 * 19.0;
            let title_width = (ui.available_width() - buttons_width - 4.0).max(40.0);
            let (title_rect, title) =
                ui.allocate_exact_size(egui::vec2(title_width, 16.0), Sense::click_and_drag());
            ui.painter().text(
                egui::pos2(title_rect.left() + 5.0, title_rect.center().y),
                Align2::LEFT_CENTER,
                "Launcher95",
                FontId::proportional(12.5),
                Color32::WHITE,
            );
            if title.drag_started() {
                start_drag = true;
            }
            if title.double_clicked() {
                toggle_maximize = true;
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if title_button(ui, TitleButton::Close) {
                    close = true;
                }
                if title_button(
                    ui,
                    if maximized {
                        TitleButton::Restore
                    } else {
                        TitleButton::Maximize
                    },
                ) {
                    toggle_maximize = true;
                }
                if title_button(ui, TitleButton::Minimize) {
                    minimize = true;
                }
            });
        });

        if start_drag {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if toggle_maximize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if minimize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let mut action = None;
        let mut dirty = false;

        ui.horizontal(|ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Add programs...").clicked() {
                    action = Some(MenuAction::Add);
                    ui.close_menu();
                }
                if ui.button("Find program...").clicked() {
                    action = Some(MenuAction::Find);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    action = Some(MenuAction::Exit);
                    ui.close_menu();
                }
            });

            ui.menu_button("Edit", |ui| {
                if ui.button("New group...").clicked() {
                    action = Some(MenuAction::NewGroup);
                    ui.close_menu();
                }
                ui.separator();
                let has_selection = self.selected_id.is_some();
                ui.add_enabled_ui(has_selection, |ui| {
                    if ui.button("Rename selected...").clicked() {
                        action = Some(MenuAction::Rename);
                        ui.close_menu();
                    }
                    if ui.button("Delete selected").clicked() {
                        action = Some(MenuAction::Delete);
                        ui.close_menu();
                    }
                });
                ui.separator();
                if ui.button("Remove missing files").clicked() {
                    action = Some(MenuAction::CleanMissing);
                    ui.close_menu();
                }
                if ui.button("Clear search").clicked() {
                    action = Some(MenuAction::ClearSearch);
                    ui.close_menu();
                }
            });

            ui.menu_button("View", |ui| {
                let groups = self.groups();
                ui.menu_button("Group", |ui| {
                    if ui
                        .selectable_label(self.group_filter.is_empty(), "All programs")
                        .clicked()
                    {
                        self.group_filter.clear();
                        ui.close_menu();
                    }
                    ui.separator();
                    for value in &groups {
                        let current = self.group_filter == *value;
                        if ui.selectable_label(current, value.as_str()).clicked() {
                            self.group_filter = value.clone();
                            ui.close_menu();
                        }
                    }
                    if groups.is_empty() {
                        ui.add_enabled(false, egui::Button::new("(no groups)"));
                    }
                });
                ui.menu_button("Icon size", |ui| {
                    for (size, label) in [
                        (32.0, "Small (32)"),
                        (40.0, "Normal (40)"),
                        (48.0, "Medium (48)"),
                        (64.0, "Large (64)"),
                        (96.0, "Extra large (96)"),
                    ] {
                        let selected = (self.settings.icon_size - size).abs() < 0.1;
                        if ui.selectable_label(selected, label).clicked() {
                            self.settings.icon_size = size;
                            dirty = true;
                            ui.close_menu();
                        }
                    }
                });
                if ui.checkbox(&mut self.settings.show_names, "Show names").changed() {
                    dirty = true;
                }
                if ui
                    .checkbox(&mut self.settings.sort_by_name, "Sort by name")
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(&mut self.settings.always_on_top, "Always on top")
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(
                        &mut self.settings.minimize_on_blur,
                        "Minimize when clicking outside",
                    )
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(
                        &mut self.settings.open_at_mouse,
                        "Open at the mouse pointer",
                    )
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(
                        &mut self.settings.remember_window_position,
                        "Remember window position",
                    )
                    .changed()
                {
                    dirty = true;
                }
            });

            ui.menu_button("Tools", |ui| {
                if ui.button("Open data folder").clicked() {
                    action = Some(MenuAction::OpenData);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Settings...").clicked() {
                    action = Some(MenuAction::Settings);
                    ui.close_menu();
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("About Launcher95").clicked() {
                    action = Some(MenuAction::About);
                    ui.close_menu();
                }
            });
        });

        if dirty {
            self.settings_dirty = true;
        }

        match action {
            Some(MenuAction::Add) => self.add_dialog(),
            Some(MenuAction::Find) => {
                self.show_search = true;
                self.focus_search = true;
            }
            Some(MenuAction::NewGroup) => {
                let seed = self.selected_id.clone();
                self.begin_group(seed.as_deref());
            }
            Some(MenuAction::Exit) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
            Some(MenuAction::Rename) => self.begin_rename(),
            Some(MenuAction::Delete) => {
                if let Some(id) = self.selected_id.clone() {
                    self.delete_entry(&id);
                }
            }
            Some(MenuAction::CleanMissing) => self.remove_missing(),
            Some(MenuAction::ClearSearch) => {
                self.search.clear();
                self.group_filter.clear();
            }
            Some(MenuAction::OpenData) => Self::open_dir(&self.data_dir),
            Some(MenuAction::Settings) => self.show_settings = true,
            Some(MenuAction::About) => self.show_about = true,
            None => {}
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let count = self.entries.len();
        let rect = ui.max_rect();
        let painter = ui.painter();
        let text_color = theme::INK;
        let font = FontId::proportional(11.0);

        // Flat Win95 status bar: count on the left, state on the right.
        let mut left = format!("{count} Program{}", plural(count));
        let shown = self.filtered_entries().len();
        if shown != count {
            left.push_str(&format!(" ({shown} shown)"));
        }
        if !self.group_filter.is_empty() {
            left.push_str(&format!(" - Group: {}", self.group_filter));
        }
        painter.text(
            egui::pos2(rect.left() + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            left,
            font.clone(),
            text_color,
        );
        painter.text(
            egui::pos2(rect.right() - 20.0, rect.center().y),
            Align2::RIGHT_CENTER,
            truncate(&self.status, 60),
            font,
            text_color,
        );

        draw_size_grip(painter, rect);
    }

    fn filtered_entries(&self) -> Vec<AppEntry> {
        let tokens: Vec<String> = self
            .search
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let group = self.group_filter.clone();

        let mut visible: Vec<AppEntry> = self
            .entries
            .iter()
            .filter(|entry| {
                (group.is_empty() || entry.group == group)
                    && tokens.iter().all(|token| {
                        entry.name.to_lowercase().contains(token)
                            || entry.path.to_lowercase().contains(token)
                    })
            })
            .cloned()
            .collect();

        if self.settings.sort_by_name {
            visible.sort_by_key(|entry| entry.name.to_ascii_lowercase());
        }
        visible
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        let visible = self.filtered_entries();
        let full = ui.max_rect();
        let side = (full.width() * 0.042).clamp(6.0, 34.0);
        let panel = full.shrink2(egui::vec2(side, 1.0));

        // The icon area is a flat white panel with a one-pixel shadow outline,
        // flush between the menu bar and the status bar.
        let painter = ui.painter();
        painter.rect_filled(full, CornerRadius::ZERO, theme::WIN95);
        painter.rect_filled(panel, CornerRadius::ZERO, Color32::WHITE);
        painter.rect_stroke(
            panel.shrink(0.5),
            CornerRadius::ZERO,
            Stroke::new(1.0_f32, theme::WIN95_SHADOW),
            egui::StrokeKind::Inside,
        );

        let inner = panel.shrink2(egui::vec2(4.0, 6.0));
        let mut grid_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        self.grid(&mut grid_ui, &visible);
    }

    fn grid(&mut self, ui: &mut egui::Ui, visible: &[AppEntry]) {
        self.tile_rects.clear();

        if visible.is_empty() {
            ui.add_space(28.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(if self.entries.is_empty() {
                        "No programs yet"
                    } else {
                        "No matching programs"
                    })
                    .strong()
                    .size(13.0),
                );
                ui.add_space(4.0);
                let hint = if self.entries.is_empty() {
                    "Use File > Add programs, or drag an EXE / LNK / URL file onto this window."
                } else {
                    "Try another search term or group filter."
                };
                ui.label(
                    RichText::new(hint)
                        .size(11.0)
                        .color(Color32::from_gray(70)),
                );
            });
            return;
        }

        let icon_size = self.settings.icon_size.clamp(24.0, 128.0);
        let label_height = if self.settings.show_names { 16.0 } else { 0.0 };
        let cell_height = icon_size + label_height + 3.0;

        // Equal-width columns centred in the panel, as in the mockup.
        let width = ui.available_width();
        let (columns, cell_width) = grid_columns(width, icon_size);
        let row_width = cell_width * columns as f32;
        let row_left = ui.max_rect().center().x - row_width * 0.5;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
                for row in visible.chunks(columns) {
                    let left_pad = (row_left - ui.max_rect().left()).max(0.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        ui.add_space(left_pad);
                        for entry in row {
                            self.draw_tile(ui, entry, cell_width, cell_height);
                        }
                    });
                }
            });

        if let Some(source) = self.dragging_id.clone() {
            let pointer_down = ui.input(|input| input.pointer.any_down());
            let released = ui.input(|input| input.pointer.any_released());
            if !pointer_down {
                if released {
                    if let Some(pointer) = ui.ctx().pointer_interact_pos() {
                        let target = self
                            .tile_rects
                            .iter()
                            .find(|(id, rect)| id != &source && rect.contains(pointer))
                            .map(|(id, _)| id.clone());
                        if let Some(target) = target {
                            self.assign_group(&source, &target);
                        }
                    }
                }
                self.dragging_id = None;
            }
        }
    }

    fn draw_tile(
        &mut self,
        ui: &mut egui::Ui,
        entry: &AppEntry,
        tile_width: f32,
        tile_height: f32,
    ) {
        let icon_size = self.settings.icon_size.clamp(24.0, 128.0);
        let show_names = self.settings.show_names;

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(tile_width, tile_height),
            Sense::click_and_drag(),
        );
        self.tile_rects.push((entry.id.clone(), rect));

        let hovered = response.hovered();
        let selected = self.selected_id.as_deref() == Some(entry.id.as_str());
        let dragging_this = self.dragging_id.as_deref() == Some(entry.id.as_str());
        let drop_target = self.dragging_id.is_some() && hovered && !dragging_this;
        let highlighted = selected || hovered || drop_target;

        let pixels_per_point = ui.ctx().pixels_per_point();
        let pixel_size = (icon_size * pixels_per_point)
            .round()
            .clamp(16.0, 256.0) as u32;
        let texture = self.icons.texture(ui.ctx(), &entry.path, pixel_size);

        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 6.0 + icon_size * 0.5),
            egui::vec2(icon_size, icon_size),
        );

        let extension = Path::new(&entry.path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let glyph = match extension.as_str() {
            "exe" => "▣",
            "lnk" => "↗",
            "url" => "◆",
            _ => "□",
        };
        let glyph_color = match extension.as_str() {
            "exe" => Color32::from_rgb(0, 0, 128),
            "lnk" => Color32::from_rgb(0, 110, 0),
            _ => Color32::from_rgb(128, 64, 0),
        };

        let label_size = 11.5;
        let label_top = icon_rect.bottom() + 4.0;
        let label_band = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 3.0, icon_rect.top() - 4.0),
            egui::pos2(rect.right() - 3.0, label_top + label_size + 4.0),
        );

        {
            let painter = ui.painter();
            // Classic behaviour: hovering tints the cell, selecting darkens it.
            if highlighted {
                let fill = if selected || drop_target {
                    theme::NAVY
                } else {
                    theme::WIN95_LIGHT
                };
                painter.rect_filled(label_band, CornerRadius::ZERO, fill);
            }
            if drop_target {
                painter.rect_stroke(
                    rect.shrink(1.0),
                    CornerRadius::ZERO,
                    Stroke::new(2.0_f32, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }

            match &texture {
                Some(texture) => {
                    painter.image(
                        texture.id(),
                        icon_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                None => {
                    painter.text(
                        icon_rect.center(),
                        Align2::CENTER_CENTER,
                        glyph,
                        FontId::proportional(icon_size * 0.55),
                        glyph_color,
                    );
                }
            }

            if show_names {
                let color = if selected || drop_target {
                    Color32::WHITE
                } else {
                    theme::INK
                };
                let max_chars = (((tile_width - 8.0) / 6.2) as usize).max(4);
                let label = truncate(&entry.name, max_chars);
                let mut job = egui::text::LayoutJob::simple(
                    label,
                    FontId::proportional(label_size),
                    color,
                    tile_width - 8.0,
                );
                job.wrap.max_rows = 1;
                let galley = painter.layout_job(job);
                let position = egui::pos2(
                    rect.center().x - galley.size().x * 0.5,
                    label_top,
                );
                painter.galley(position, galley, color);
            }

            if selected {
                dotted_rect(painter, label_band.shrink(1.0));
            }
        }

        if response.clicked() {
            self.selected_id = Some(entry.id.clone());
            self.launch_entry(entry);
        }
        if response.secondary_clicked() {
            self.selected_id = Some(entry.id.clone());
        }
        if response.drag_started() {
            self.selected_id = Some(entry.id.clone());
            self.dragging_id = Some(entry.id.clone());
        }

        let id = entry.id.clone();
        let name = entry.name.clone();
        let path = entry.path.clone();
        let group = entry.group.clone();
        let all_groups = self.groups();
        let mut assign_to: Option<String> = None;
        let mut new_group = false;
        response.context_menu(|ui| {
            if ui.button("Run").clicked() {
                self.launch_entry(entry);
                ui.close_menu();
            }
            if ui.button("Open file location").clicked() {
                Self::open_location(&path);
                ui.close_menu();
            }
            if ui.button("Rename...").clicked() {
                self.edit_id = Some(id.clone());
                self.edit_name = name.clone();
                self.focus_rename = true;
                ui.close_menu();
            }
            ui.menu_button("Add to group", |ui| {
                for value in &all_groups {
                    let current = value == &group;
                    if ui.selectable_label(current, value.as_str()).clicked() {
                        assign_to = Some(value.clone());
                        ui.close_menu();
                    }
                }
                if !all_groups.is_empty() {
                    ui.separator();
                }
                if ui.button("New group...").clicked() {
                    new_group = true;
                    ui.close_menu();
                }
            });
            if !group.is_empty() && ui.button("Remove from group").clicked() {
                if let Some(item) = self.entries.iter_mut().find(|item| item.id == id) {
                    item.group.clear();
                }
                self.persist_entries();
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Delete from launcher").clicked() {
                self.delete_entry(&id);
                ui.close_menu();
            }
        });

        if let Some(target) = assign_to {
            if let Some(item) = self.entries.iter_mut().find(|item| item.id == id) {
                item.group = target.clone();
            }
            self.group_filter.clear();
            self.persist_entries();
            self.status = format!("Added to group: {target}");
        }
        if new_group {
            self.begin_group(Some(&id));
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let mut dirty = false;
        let mut apply_hotkey = false;
        let mut close = false;
        let data_dir = self.data_dir.display().to_string();

        egui::Window::new("Launcher Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(390.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Icon size:");
                    for (size, label) in [
                        (32.0, "32"),
                        (40.0, "40"),
                        (48.0, "48"),
                        (64.0, "64"),
                        (96.0, "96"),
                    ] {
                        if ui
                            .selectable_label((self.settings.icon_size - size).abs() < 0.1, label)
                            .clicked()
                        {
                            self.settings.icon_size = size;
                            dirty = true;
                        }
                    }
                });
                if ui
                    .checkbox(&mut self.settings.show_names, "Show application names")
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(&mut self.settings.sort_by_name, "Sort by name")
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(&mut self.settings.always_on_top, "Always on top")
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(
                        &mut self.settings.minimize_on_blur,
                        "Minimize when clicking outside",
                    )
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(
                        &mut self.settings.open_at_mouse,
                        "Open at the mouse pointer",
                    )
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(
                        &mut self.settings.remember_window_position,
                        "Remember window position",
                    )
                    .changed()
                {
                    dirty = true;
                }

                ui.separator();
                ui.label(RichText::new("Global shortcut").strong());
                ui.horizontal(|ui| {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.hotkey_text).desired_width(150.0),
                    );
                    if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                        apply_hotkey = true;
                    }
                    if ui.button("Apply").clicked() {
                        apply_hotkey = true;
                    }
                });
                ui.label(
                    RichText::new("Examples: Alt+Space, Ctrl+Shift+L, Ctrl+Alt+Space")
                        .size(10.0)
                        .color(Color32::from_gray(80)),
                );

                ui.separator();
                ui.label(RichText::new("Data folder").strong());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&data_dir).size(10.0));
                    if ui.button("Open").clicked() {
                        Self::open_dir(&self.data_dir);
                    }
                });

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save now").clicked() {
                        let _ = save_settings(&self.data_dir, &self.settings);
                        self.settings_dirty = false;
                        self.status = "Settings saved".into();
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });

        if dirty {
            self.settings_dirty = true;
        }
        if apply_hotkey {
            self.settings.hotkey = self.hotkey_text.trim().to_owned();
            self.register_hotkey();
            self.settings_dirty = true;
        }
        if close {
            open = false;
        }
        self.show_settings = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        let mut close = false;

        egui::Window::new("About Launcher95")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.heading("Launcher95 0.4");
                ui.label("A portable Windows 95-style application launcher.");
                ui.add_space(6.0);
                ui.label(RichText::new("Controls").strong());
                ui.label("Single click: run a program");
                ui.label("Right click: program menu, including Add to group");
                ui.label("Drag one tile onto another: create a group");
                ui.label("Edit > New group, or View > Group: pick programs for a group");
                ui.label("Ctrl+F or File > Find program: search");
                ui.label("Drag an EXE / LNK / URL file here: add it");
                ui.label(format!(
                    "{}: show or hide the launcher",
                    self.settings.hotkey
                ));
                ui.label("Clicking outside the window minimizes it");
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Data stays in the Data folder beside the executable.")
                        .size(10.0)
                        .color(Color32::from_gray(80)),
                );
                ui.add_space(6.0);
                if ui.button("OK").clicked() {
                    close = true;
                }
            });

        if close {
            open = false;
        }
        self.show_about = open;
    }

    fn rename_window(&mut self, ctx: &egui::Context) {
        let Some(target) = self.edit_id.clone() else {
            return;
        };
        let mut open = true;
        let mut commit = false;
        let mut cancel = false;

        egui::Window::new("Rename program")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.label("New name:");
                let response = ui.text_edit_singleline(&mut self.edit_name);
                if self.focus_rename {
                    response.request_focus();
                    self.focus_rename = false;
                }
                if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    commit = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
                if ui.input(|input| input.key_pressed(Key::Escape)) {
                    cancel = true;
                }
            });

        if commit {
            let name = self.edit_name.trim().to_owned();
            if !name.is_empty() {
                if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == target) {
                    entry.name = name;
                }
                self.persist_entries();
                self.status = "Renamed".into();
            }
        }
        if commit || cancel || !open {
            self.edit_id = None;
        }
    }

    fn search_window(&mut self, ctx: &egui::Context) {
        if !self.show_search {
            return;
        }
        let mut open = true;
        let mut launch_first = false;
        let mut close = false;
        let visible_count = self.filtered_entries().len();

        egui::Window::new("Find Program")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.label("Search by name or path:");                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .desired_width(f32::INFINITY)
                        .hint_text("Program name or path"),
                );
                if self.focus_search {
                    response.request_focus();
                    self.focus_search = false;
                }
                if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    launch_first = true;
                }
                if response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    launch_first = true;
                }

                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "{visible_count} matching program{}",
                        plural(visible_count)
                    ))
                    .size(11.0)
                    .color(Color32::from_gray(70)),
                );

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Launch first").clicked() {
                        launch_first = true;
                    }
                    if ui.button("Clear").clicked() {
                        self.search.clear();
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                if ui.input(|input| input.key_pressed(Key::Escape)) {
                    close = true;
                }
            });

        if launch_first {
            if let Some(entry) = self.filtered_entries().first().cloned() {
                self.launch_entry(&entry);
                close = true;
            }
        }
        if close || !open {
            self.show_search = false;
        }
    }

    fn group_window(&mut self, ctx: &egui::Context) {        if !self.show_group_dialog {
            return;
        }
        let mut open = true;
        let mut create = false;
        let mut cancel = false;

        let entries: Vec<(String, String, String)> = self
            .entries
            .iter()
            .map(|entry| (entry.id.clone(), entry.name.clone(), entry.group.clone()))
            .collect();

        egui::Window::new("New Group")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(340.0)
            .show(ctx, |ui| {
                ui.label("Group name:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.group_name)
                        .desired_width(f32::INFINITY)
                        .hint_text("e.g. Office, Games, Audio"),
                );
                if self.focus_group_name {
                    response.request_focus();
                    self.focus_group_name = false;
                }
                if response.lost_focus()
                    && ui.input(|input| input.key_pressed(Key::Enter))
                    && !self.group_name.trim().is_empty()
                {
                    create = true;
                }

                ui.add_space(6.0);
                ui.label(RichText::new("Programs in this group").strong());
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if entries.is_empty() {
                            ui.label(
                                RichText::new("No programs yet.")
                                    .size(11.0)
                                    .color(Color32::from_gray(80)),
                            );
                        }
                        for (id, name, group) in &entries {
                            let mut checked = self.group_members.contains(id);
                            let label = if group.is_empty() {
                                name.clone()
                            } else {
                                format!("{name}  [{group}]")
                            };
                            if ui.checkbox(&mut checked, label).changed() {
                                if checked {
                                    self.group_members.insert(id.clone());
                                } else {
                                    self.group_members.remove(id);
                                }
                            }
                        }
                    });

                ui.horizontal(|ui| {
                    if ui.button("Select all").clicked() {
                        for (id, _, _) in &entries {
                            self.group_members.insert(id.clone());
                        }
                    }
                    if ui.button("Clear").clicked() {
                        self.group_members.clear();
                    }
                });

                ui.separator();
                ui.horizontal(|ui| {
                    let can_create =
                        !self.group_name.trim().is_empty() && !self.group_members.is_empty();
                    if ui
                        .add_enabled(can_create, egui::Button::new("Create"))
                        .clicked()
                    {
                        create = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
                if ui.input(|input| input.key_pressed(Key::Escape)) {
                    cancel = true;
                }
            });

        if create {
            self.create_group();
        }
        if cancel || !open {
            self.show_group_dialog = false;
        }
    }
}

/// True while the left mouse button is physically held down anywhere.
fn mouse_button_down() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
        // High bit set means the key is currently down.
        unsafe { (GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000) != 0 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn hotkey_listener(hotkey_id: Arc<AtomicU32>, hwnd: isize, focus_request: Arc<AtomicBool>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, SW_MINIMIZE,
        SW_RESTORE,
    };

    let hwnd = hwnd as *mut std::ffi::c_void;
    let receiver = GlobalHotKeyEvent::receiver();
    while let Ok(event) = receiver.recv() {
        if event.state != global_hotkey::HotKeyState::Pressed
            || event.id != hotkey_id.load(Ordering::Relaxed)
        {
            continue;
        }
        unsafe {
            if IsIconic(hwnd) != 0 {
                // Minimized: restore and focus.
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
                focus_request.store(true, Ordering::Relaxed);
            } else if GetForegroundWindow() == hwnd {
                // Already in front: hide to the taskbar.
                ShowWindow(hwnd, SW_MINIMIZE);
            } else {
                // Visible but behind another window: bring it forward.
                SetForegroundWindow(hwnd);
                focus_request.store(true, Ordering::Relaxed);
            }
        }
    }
}

impl eframe::App for Launcher95App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.ensure_hotkey_listener(frame);
        if self.focus_request.swap(false, Ordering::Relaxed) {
            self.focus_search = true;
            self.hold_blur(700);
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.handle_shortcuts(ctx);
        self.handle_dropped_files(ctx);
        self.handle_blur(ctx);
        self.sync_window_geometry(ctx);
        self.apply_window_level(ctx);
        self.autosave();

        egui::TopBottomPanel::top("titlebar")
            .exact_height(22.0)
            .frame(
                Frame::new()
                    .fill(theme::NAVY)
                    .inner_margin(Margin::symmetric(2, 2)),
            )
            .show(ctx, |ui| self.title_bar(ui, ctx));

        egui::TopBottomPanel::top("menubar")
            .exact_height(22.0)
            .frame(
                Frame::new()
                    .fill(theme::WIN95)
                    .inner_margin(Margin::symmetric(4, 1)),
            )
            .show(ctx, |ui| self.menu_bar(ui));

        egui::TopBottomPanel::bottom("statusbar")
            .exact_height(22.0)
            .frame(
                Frame::new()
                    .fill(theme::WIN95)
                    .inner_margin(Margin::symmetric(0, 0)),
            )
            .show(ctx, |ui| self.status_bar(ui));

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(theme::WIN95)
                    .inner_margin(Margin::symmetric(1, 1)),
            )
            .show(ctx, |ui| self.content(ui));

        if self.show_settings {
            self.settings_window(ctx);
        }
        if self.show_about {
            self.about_window(ctx);
        }
        if self.edit_id.is_some() {
            self.rename_window(ctx);
        }
        if self.show_group_dialog {
            self.group_window(ctx);
        }
        if self.show_search {
            self.search_window(ctx);
        }

        // Classic raised window edge around the whole client area.
        let border = ctx.screen_rect().shrink(0.5);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("window_edge"),
        ));
        painter.line_segment(
            [border.left_top(), border.right_top()],
            Stroke::new(1.0_f32, Color32::WHITE),
        );
        painter.line_segment(
            [border.left_top(), border.left_bottom()],
            Stroke::new(1.0_f32, Color32::WHITE),
        );
        painter.line_segment(
            [border.right_top(), border.right_bottom()],
            Stroke::new(1.0_f32, theme::WIN95_DARK),
        );
        painter.line_segment(
            [border.left_bottom(), border.right_bottom()],
            Stroke::new(1.0_f32, theme::WIN95_DARK),
        );

        if ctx.input(|input| input.viewport().close_requested()) {
            let _ = save_settings(&self.data_dir, &self.settings);
            let _ = save_entries(&self.data_dir, &self.entries);
        }

        ctx.request_repaint_after(Duration::from_millis(120));
    }
}

fn title_button(ui: &mut egui::Ui, kind: TitleButton) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), Sense::click());
    let pressed = response.is_pointer_button_down_on();
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::ZERO,
        if pressed {
            theme::WIN95_LIGHT
        } else {
            theme::WIN95
        },
    );
    theme::bevel(painter, rect, !pressed);

    let ink = Stroke::new(1.0_f32, theme::INK);
    let center = rect.center();
    match kind {
        TitleButton::Minimize => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 3.0, rect.bottom() - 5.0),
                    egui::pos2(center.x + 3.0, rect.bottom() - 5.0),
                ],
                Stroke::new(1.5_f32, theme::INK),
            );
        }
        TitleButton::Maximize => {
            let box_rect = egui::Rect::from_center_size(center, egui::vec2(8.0, 7.0));
            painter.line_segment([box_rect.left_top(), box_rect.right_top()], ink);
            painter.line_segment([box_rect.right_top(), box_rect.right_bottom()], ink);
            painter.line_segment([box_rect.right_bottom(), box_rect.left_bottom()], ink);
            painter.line_segment([box_rect.left_bottom(), box_rect.left_top()], ink);
        }
        TitleButton::Restore => {
            let front = egui::Rect::from_center_size(center + egui::vec2(1.0, 1.0), egui::vec2(7.0, 6.0));
            let back = egui::Rect::from_center_size(center - egui::vec2(1.0, 1.0), egui::vec2(7.0, 6.0));
            painter.line_segment([back.left_top(), back.right_top()], ink);
            painter.line_segment([back.right_top(), back.right_bottom()], ink);
            painter.line_segment([front.left_top(), front.right_top()], ink);
            painter.line_segment([front.right_top(), front.right_bottom()], ink);
            painter.line_segment([front.right_bottom(), front.left_bottom()], ink);
            painter.line_segment([front.left_bottom(), front.left_top()], ink);
        }
        TitleButton::Close => {
            painter.line_segment(
                [
                    center + egui::vec2(-3.0, -3.0),
                    center + egui::vec2(3.0, 3.0),
                ],
                Stroke::new(1.5_f32, theme::INK),
            );
            painter.line_segment(
                [
                    center + egui::vec2(3.0, -3.0),
                    center + egui::vec2(-3.0, 3.0),
                ],
                Stroke::new(1.5_f32, theme::INK),
            );
        }
    }

    response.clicked()
}

/// Splits the icon panel into equal-width columns, capped so labels stay readable.
fn grid_columns(width: f32, icon_size: f32) -> (usize, f32) {
    let ideal_cell = icon_size + 103.0;
    let columns = ((width / ideal_cell).floor() as usize).clamp(1, 8);
    let cell_width = ideal_cell.min(width / columns as f32);
    (columns, cell_width)
}

/// Draws the classic resize grip in the bottom-right corner of the status bar.
fn draw_size_grip(painter: &egui::Painter, rect: egui::Rect) {
    let light = theme::WIN95_LIGHT;
    let dark = theme::WIN95_DARK;
    let right = rect.right() - 3.0;
    let bottom = rect.bottom() - 3.0;
    for i in 0..3 {
        let offset = i as f32 * 4.0;
        painter.line_segment(
            [
                egui::pos2(right - offset - 3.0, bottom),
                egui::pos2(right, bottom - offset - 3.0),
            ],
            Stroke::new(1.0_f32, dark),
        );
        painter.line_segment(
            [
                egui::pos2(right - offset - 2.0, bottom),
                egui::pos2(right, bottom - offset - 2.0),
            ],
            Stroke::new(1.0_f32, light),
        );
    }
}

/// Win95 selection marker: a 1px dotted rectangle around the icon label.
fn dotted_rect(painter: &egui::Painter, rect: egui::Rect) {
    let color = Color32::WHITE;
    let step = 2.0;
    let mut x = rect.left();
    while x < rect.right() {
        painter.line_segment(
            [
                egui::pos2(x, rect.top()),
                egui::pos2((x + 1.0).min(rect.right()), rect.top()),
            ],
            Stroke::new(1.0_f32, color),
        );
        painter.line_segment(
            [
                egui::pos2(x, rect.bottom()),
                egui::pos2((x + 1.0).min(rect.right()), rect.bottom()),
            ],
            Stroke::new(1.0_f32, color),
        );
        x += step;
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.line_segment(
            [
                egui::pos2(rect.left(), y),
                egui::pos2(rect.left(), (y + 1.0).min(rect.bottom())),
            ],
            Stroke::new(1.0_f32, color),
        );
        painter.line_segment(
            [
                egui::pos2(rect.right(), y),
                egui::pos2(rect.right(), (y + 1.0).min(rect.bottom())),
            ],
            Stroke::new(1.0_f32, color),
        );
        y += step;
    }
}

fn plural(count: usize) -> &'static str {    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut shortened: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    shortened.push('…');
    shortened
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::load_entries;

    fn temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("launcher95-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn entry(id: &str, name: &str, group: &str) -> AppEntry {
        AppEntry {
            id: id.into(),
            name: name.into(),
            path: format!("C:/{id}.exe"),
            group: group.into(),
            source: EntrySource::Manual,
        }
    }

    #[test]
    fn grid_uses_mockup_column_count() {
        // The reference mockup uses ~40px icons spread over 5 equal columns.
        let (columns, cell) = grid_columns(810.0, 40.0);
        assert_eq!(columns, 5);
        assert!((cell - 143.0).abs() < 1.0, "cell was {cell}");

        // Very narrow panes degrade to one column instead of producing zero.
        let (columns, _) = grid_columns(90.0, 96.0);
        assert_eq!(columns, 1);

        // Wide panes stop at the cap.
        let (columns, _) = grid_columns(4000.0, 32.0);
        assert_eq!(columns, 8);
    }

    #[test]
    fn search_window_shortcut_opens_find() {
        let dir = temp_dir("search-key");
        save_entries(&dir, &[entry("a", "Alpha", "")]).unwrap();
        let ctx = egui::Context::default();
        let mut app = Launcher95App::new_with_context(&ctx, dir.clone());

        assert!(!app.show_search);
        let mut input = egui::RawInput::default();
        input.modifiers.ctrl = true;
        input.events.push(egui::Event::Key {
            key: Key::F,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        });
        ctx.begin_pass(input);
        app.handle_shortcuts(&ctx);
        let _ = ctx.end_pass();

        assert!(app.show_search, "Ctrl+F opens the find window");
        assert!(app.focus_search, "the search box takes focus");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_group_moves_selected_entries_and_persists() {
        let dir = temp_dir("group");
        save_entries(
            &dir,
            &[
                entry("a", "Alpha", ""),
                entry("b", "Beta", "Old"),
                entry("c", "Gamma", ""),
            ],
        )
        .unwrap();

        let ctx = egui::Context::default();
        let mut app = Launcher95App::new_with_context(&ctx, dir.clone());

        app.group_name = "  Office  ".to_owned();
        app.group_members = ["a".to_owned(), "c".to_owned()].into_iter().collect();
        app.show_group_dialog = true;
        app.create_group();

        assert!(!app.show_group_dialog, "dialog closes after creating a group");
        assert_eq!(app.group_filter, "Office");
        assert_eq!(app.entries[0].group, "Office");
        assert_eq!(app.entries[1].group, "Old");
        assert_eq!(app.entries[2].group, "Office");

        let saved = load_entries(&dir);
        assert_eq!(saved[0].group, "Office");
        assert_eq!(saved[1].group, "Old");
        assert_eq!(saved[2].group, "Office");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_group_needs_a_name_and_at_least_one_member() {
        let dir = temp_dir("group-empty");
        save_entries(&dir, &[entry("a", "Alpha", "")]).unwrap();
        let ctx = egui::Context::default();
        let mut app = Launcher95App::new_with_context(&ctx, dir.clone());

        app.group_name = "   ".to_owned();
        app.group_members = ["a".to_owned()].into_iter().collect();
        app.create_group();
        assert_eq!(app.entries[0].group, "", "blank group name is rejected");

        app.group_name = "Office".to_owned();
        app.group_members.clear();
        app.create_group();
        assert_eq!(app.entries[0].group, "", "empty member list is rejected");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
