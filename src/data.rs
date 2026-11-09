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
    let path = dir.join("apps.json");
    let Some(text) = read_json(path.clone()) else {
        return Vec::new();
    };
    match serde_json::from_str(&text) {
        Ok(entries) => entries,
        Err(_) => {
            backup_corrupt(&path, dir, "apps.json.bak");
            Vec::new()
        }
    }
}

pub fn load_settings(dir: &Path) -> Settings {
    let path = dir.join("settings.json");
    let Some(text) = read_json(path.clone()) else {
        return Settings::default();
    };
    match serde_json::from_str(&text) {
        Ok(settings) => settings,
        Err(_) => {
            backup_corrupt(&path, dir, "settings.json.bak");
            Settings::default()
        }
    }
}

/// Preserves a corrupt JSON file for manual recovery instead of silently
/// overwriting it on the next save.
fn backup_corrupt(path: &Path, dir: &Path, file_name: &str) {
    let backup = dir.join(file_name);
    if backup.exists() {
        return;
    }
    let _ = fs::copy(path, backup);
}

fn write_atomic(path: &Path, json: &str) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| "no parent directory".to_string())?;
    let tmp = dir.join(format!(".{}.tmp", path.file_name().unwrap_or_default().to_string_lossy()));
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

pub fn save_entries(dir: &Path, entries: &[AppEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    write_atomic(&dir.join("apps.json"), &json)
}

pub fn save_settings(dir: &Path, settings: &Settings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    write_atomic(&dir.join("settings.json"), &json)
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
    let id = stable_entry_id(&key);
    Some(AppEntry {
        id: format!("{id:016x}"),
        name,
        path: path.to_string_lossy().to_string(),
        group: String::new(),
        source,
    })
}

/// Stable FNV-1a so the same path always produces the same ID across runs.
fn stable_entry_id(key: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &byte in key.as_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
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

    #[test]
    fn corrupt_json_is_backed_up_not_silently_lost() {
        let dir = std::env::temp_dir().join(format!(
            "launcher95-test-corrupt-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("apps.json"), "{[not json").unwrap();
        std::fs::write(dir.join("settings.json"), "{{\"bad\"").unwrap();

        assert!(load_entries(&dir).is_empty());
        assert!(dir.join("apps.json.bak").exists());
        let _ = load_settings(&dir);
        assert!(dir.join("settings.json.bak").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_is_atomic_and_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "launcher95-test-atomic-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let entries = vec![AppEntry {
            id: "a".into(),
            name: "A".into(),
            path: "C:/a.exe".into(),
            group: String::new(),
            source: EntrySource::Manual,
        }];
        save_entries(&dir, &entries).unwrap();
        assert_eq!(load_entries(&dir), entries);
        // No temp files left behind.
        for name in std::fs::read_dir(&dir).unwrap().flatten() {
            assert!(
                !name.file_name().to_string_lossy().ends_with(".tmp"),
                "temp file leaked: {:?}",
                name.path()
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
