use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

static EN_US_JSON: &str = include_str!("../locales/en-us.json");
static PT_BR_JSON: &str = include_str!("../locales/pt-br.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I18nStrings {
    // AppWindow Strings
    pub cpu_title: String,
    pub ram_title: String,
    pub gpu_title: String,
    pub disks_title: String,
    pub network_title: String,
    pub clock: String,
    pub temp: String,
    pub usage_per_core: String,
    #[serde(default = "default_usage_label")]
    pub usage: String,
    pub available: String,
    pub fan: String,
    pub local_ip: String,
    pub wan_ip: String,
    pub hotkey_hint: String,

    // SettingsWindow Header & Tabs
    pub window_title: String,
    pub header_title: String,
    pub header_desc: String,
    pub tab_general: String,
    pub tab_advanced: String,
    pub tab_style: String,
    pub tab_monitors: String,
    pub tab_hotkeys: String,

    // Tab General
    pub language_label: String,
    pub language_desc: String,
    pub dock_label: String,
    pub dock_desc: String,
    pub dock_left: String,
    pub dock_right: String,
    pub monitor_label: String,
    pub monitor_desc: String,
    pub monitor_primary: String,
    pub monitor_2: String,
    pub monitor_3: String,
    pub appbar_label: String,
    pub appbar_desc: String,
    pub always_top_label: String,
    pub always_top_desc: String,
    pub startup_label: String,
    pub startup_desc: String,
    pub initially_hidden_label: String,
    pub initially_hidden_desc: String,

    // Tab Advanced
    pub polling_label: String,
    pub polling_desc: String,
    pub tray_icon_label: String,
    pub tray_icon_desc: String,
    pub click_through_label: String,
    pub click_through_desc: String,
    pub collapse_menu_label: String,
    pub collapse_menu_desc: String,
    pub auto_update_label: String,
    pub auto_update_desc: String,

    // Tab Style
    pub sidebar_width_label: String,
    pub sidebar_width_desc: String,
    pub bg_opacity_label: String,
    pub bg_opacity_desc: String,
    pub text_align_label: String,
    pub text_align_desc: String,
    pub align_left: String,
    pub align_right: String,
    pub show_hostname_label: String,
    pub show_hostname_desc: String,
    pub show_clock_label: String,
    pub show_clock_desc: String,
    pub clock_24hr_label: String,
    pub clock_24hr_desc: String,
    pub alert_blink_label: String,
    pub alert_blink_desc: String,

    // Tab Hardware
    pub enable_cpu_label: String,
    pub enable_cpu_desc: String,
    pub show_cores_label: String,
    pub show_cores_desc: String,
    pub enable_ram_label: String,
    pub enable_ram_desc: String,
    pub enable_gpu_label: String,
    pub enable_gpu_desc: String,
    pub enable_disks_label: String,
    pub enable_disks_desc: String,
    pub show_all_disks_label: String,
    pub show_all_disks_desc: String,
    pub enable_network_label: String,
    pub enable_network_desc: String,
    pub show_sparklines_label: String,
    pub show_sparklines_desc: String,

    // Tab Hotkeys
    pub hotkey_toggle_label: String,
    pub hotkey_toggle_desc: String,
    pub hotkey_edge_label: String,
    pub hotkey_edge_desc: String,
    pub hotkey_screen_label: String,
    pub hotkey_screen_desc: String,
    pub hotkey_reserve_label: String,
    pub hotkey_reserve_desc: String,

    // Buttons
    pub btn_cancel: String,
    pub btn_apply: String,
    pub btn_save: String,

    // Tray Menu
    pub tray_toggle: String,
    pub tray_settings: String,
    pub tray_toggle_edge: String,
    pub tray_cycle_monitor: String,
    pub tray_reload_sensors: String,
    pub tray_github: String,
    pub tray_exit: String,

    // Updates
    pub btn_check_update: String,
    pub update_checking: String,
    pub update_available_title: String,
    pub update_available_msg: String,
    pub update_new_version: String,
    pub update_current_version: String,
    pub update_ask_prompt: String,
    pub update_up_to_date_title: String,
    pub update_up_to_date_msg: String,
    pub update_downloading: String,
    pub update_failed: String,
}

fn default_usage_label() -> String {
    "uso".to_string()
}

impl Default for I18nStrings {
    fn default() -> Self {
        serde_json::from_str(EN_US_JSON).expect("Built-in en-us.json must be valid JSON")
    }
}

pub fn load_locale(culture: &str) -> I18nStrings {
    let lower = culture.to_lowercase();
    let norm = lower.replace('_', "-");

    let possible_paths = [
        PathBuf::from(format!("locales/{}.json", norm)),
        PathBuf::from(format!("locales/{}.json", lower)),
        PathBuf::from(format!("../locales/{}.json", norm)),
    ];

    load_locale_from_paths(culture, &possible_paths)
}

pub(crate) fn load_locale_from_paths(culture: &str, candidates: &[PathBuf]) -> I18nStrings {
    let norm = culture.to_lowercase().replace('_', "-");

    for path in candidates {
        let p: &Path = path.as_path();
        if p.is_file()
            && let Ok(content) = fs::read_to_string(p)
                && let Ok(parsed) = serde_json::from_str::<I18nStrings>(&content) {
                    return parsed;
                }
    }

    // Fall back to built-in embedded resources
    if norm.starts_with("pt") {
        serde_json::from_str::<I18nStrings>(PT_BR_JSON)
            .unwrap_or_else(|_| I18nStrings::default())
    } else {
        I18nStrings::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "zenithbar_i18n_{}_{}_{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn embedded_locales_deserialize_with_non_empty_core_labels() {
        let en = serde_json::from_str::<I18nStrings>(EN_US_JSON).unwrap();
        let pt = serde_json::from_str::<I18nStrings>(PT_BR_JSON).unwrap();

        for s in [&en, &pt] {
            assert!(!s.cpu_title.trim().is_empty());
            assert!(!s.ram_title.trim().is_empty());
            assert!(!s.gpu_title.trim().is_empty());
            assert!(!s.disks_title.trim().is_empty());
            assert!(!s.network_title.trim().is_empty());
            assert!(!s.available.trim().is_empty());
            assert!(!s.usage.trim().is_empty());
            assert!(!s.tray_reload_sensors.trim().is_empty());
        }
        assert_ne!(en.cpu_title, pt.cpu_title);
    }

    #[test]
    fn load_locale_from_paths_normalizes_culture_and_falls_back_to_english() {
        let pt_under = load_locale_from_paths("pt_BR", &[]);
        let pt_upper = load_locale_from_paths("PT-pt", &[]);
        let en = load_locale_from_paths("en-US", &[]);
        let unknown = load_locale_from_paths("fr-FR", &[]);

        let expected_pt: I18nStrings = serde_json::from_str(PT_BR_JSON).unwrap();
        assert_eq!(pt_under.cpu_title, expected_pt.cpu_title);
        assert_eq!(pt_upper.cpu_title, expected_pt.cpu_title);
        assert_eq!(unknown.cpu_title, en.cpu_title);
    }

    #[test]
    fn load_locale_from_paths_prefers_valid_external_file_and_ignores_corrupt_file() {
        let dir = unique_temp_dir("override");
        let valid_path = dir.join("custom.json");
        let corrupt_path = dir.join("broken.json");

        let custom = I18nStrings {
            cpu_title: "CUSTOM CPU".to_string(),
            ..Default::default()
        };
        fs::write(&valid_path, serde_json::to_string(&custom).unwrap()).unwrap();
        fs::write(&corrupt_path, "{ broken json").unwrap();

        let loaded = load_locale_from_paths("en-US", &[corrupt_path.clone(), valid_path]);
        assert_eq!(loaded.cpu_title, "CUSTOM CPU");

        let fallback = load_locale_from_paths("pt-BR", &[corrupt_path]);
        assert_eq!(fallback.cpu_title, serde_json::from_str::<I18nStrings>(PT_BR_JSON).unwrap().cpu_title);

        let _ = fs::remove_dir_all(&dir);
    }
}
