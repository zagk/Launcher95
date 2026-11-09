use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub source: EntrySource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum EntrySource {
    #[default]
    Manual,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hotkey: String,
    pub icon_size: f32,
    pub show_names: bool,
    pub sort_by_name: bool,
    pub always_on_top: bool,
    pub minimize_on_blur: bool,
    pub open_at_mouse: bool,
    pub remember_window_position: bool,
    pub window_x: Option<f32>,
    pub window_y: Option<f32>,
    pub window_width: f32,
    pub window_height: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Alt+Space".to_owned(),
            icon_size: 40.0,
            show_names: true,
            sort_by_name: true,
            always_on_top: false,
            minimize_on_blur: true,
            open_at_mouse: true,
            remember_window_position: true,
            window_x: None,
            window_y: None,
            window_width: 640.0,
            window_height: 480.0,
        }
    }
}

pub fn resolve_data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let portable = parent.join("Data");
            if fs::create_dir_all(&portable).is_ok() {
                return portable;
            }
        }
    }
    let fallback = dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Launcher95");
    let _ = fs::create_dir_all(&fallback);
    fallback
}

fn read_json(path: PathBuf) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    Some(text.trim_start_matches('\u{feff}').to_owned())
}

pub fn load_entries(dir: &Path) -> Vec<AppEntry> {
    read_json(dir.join("apps.json"))
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_entries(dir: &Path, entries: &[AppEntry]) -> Result<(), String> {
    let path = dir.join("apps.json");
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

pub fn load_settings(dir: &Path) -> Settings {
    read_json(dir.join("settings.json"))
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_settings(dir: &Path, settings: &Settings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(dir.join("settings.json"), json).map_err(|e| e.to_string())
}

/// Moves the selected entries into `group`. Returns how many entries were assigned.
pub fn assign_entries_to_group(
    entries: &mut [AppEntry],
    members: &std::collections::HashSet<String>,
    group: &str,
) -> usize {
    let group = group.trim();
    if group.is_empty() {
        return 0;
    }
    let mut assigned = 0usize;
    for entry in entries.iter_mut() {
        if members.contains(&entry.id) {
            entry.group = group.to_owned();
            assigned += 1;
        }
    }
    assigned
}

pub fn make_entry(path: &Path, source: EntrySource) -> Option<AppEntry> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if !["exe", "lnk", "url"].contains(&ext.as_str()) {
        return None;
    }
    let name = path.file_stem()?.to_string_lossy().to_string();
    let key = path.to_string_lossy().to_ascii_lowercase();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    key.hash(&mut hasher);
    Some(AppEntry {
        id: format!("{:016x}", hasher.finish()),
        name,
        path: path.to_string_lossy().to_string(),
        group: String::new(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_entry_rejects_unsupported_extensions() {
        assert!(make_entry(Path::new("C:/temp/readme.txt"), EntrySource::Manual).is_none());
        assert!(make_entry(Path::new("C:/temp/image.png"), EntrySource::Manual).is_none());
    }

    #[test]
    fn make_entry_accepts_supported_extensions_case_insensitively() {
        let entry = make_entry(Path::new("C:/temp/Notepad.EXE"), EntrySource::Auto).unwrap();
        assert_eq!(entry.name, "Notepad");
        assert_eq!(entry.source, EntrySource::Auto);
    }

    #[test]
    fn entry_id_ignores_path_case() {
        let a = make_entry(Path::new("C:/Temp/App.Exe"), EntrySource::Manual).unwrap();
        let b = make_entry(Path::new("c:/temp/app.exe"), EntrySource::Manual).unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn settings_fill_in_missing_fields() {
        let settings: Settings = serde_json::from_str(r#"{"hotkey":"Ctrl+Q"}"#).unwrap();
        assert_eq!(settings.hotkey, "Ctrl+Q");
        assert_eq!(settings.icon_size, 40.0);
        assert!(settings.show_names);
        assert!(settings.sort_by_name);
        assert!(settings.open_at_mouse);
    }

    #[test]
    fn settings_round_trip() {
        let settings = Settings {
            hotkey: "Ctrl+Alt+L".to_owned(),
            icon_size: 64.0,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.hotkey, "Ctrl+Alt+L");
        assert_eq!(restored.icon_size, 64.0);
    }

    #[test]
    fn assigns_only_selected_entries_to_a_group() {
        let mut entries = vec![
            AppEntry {
                id: "a".into(),
                name: "A".into(),
                path: "C:/a.exe".into(),
                group: String::new(),
                source: EntrySource::Manual,
            },
            AppEntry {
                id: "b".into(),
                name: "B".into(),
                path: "C:/b.exe".into(),
                group: "Old".into(),
                source: EntrySource::Manual,
            },
            AppEntry {
                id: "c".into(),
                name: "C".into(),
                path: "C:/c.exe".into(),
                group: String::new(),
                source: EntrySource::Manual,
            },
        ];
        let members: std::collections::HashSet<String> =
            ["a".to_string(), "c".to_string()].into_iter().collect();

        let assigned = assign_entries_to_group(&mut entries, &members, "  Office  ");
        assert_eq!(assigned, 2);
        assert_eq!(entries[0].group, "Office");
        assert_eq!(entries[1].group, "Old");
        assert_eq!(entries[2].group, "Office");

        // An empty group name is ignored instead of wiping existing groups.
        assert_eq!(assign_entries_to_group(&mut entries, &members, "   "), 0);
        assert_eq!(entries[0].group, "Office");
    }
}
