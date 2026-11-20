use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AppConfig {
    // Aba Geral (General)
    #[serde(default = "default_dock_edge")]
    pub dock_edge: String, // "Left" ou "Right"

    #[serde(default = "default_screen_index")]
    pub screen_index: usize,

    #[serde(default = "default_culture")]
    pub culture: String, // "pt-BR", "en-US", etc.

    #[serde(default = "default_true")]
    pub use_app_bar: bool,

    #[serde(default = "default_true")]
    pub always_top: bool,

    #[serde(default = "default_true")]
    pub auto_update: bool,

    #[serde(default = "default_false")]
    pub run_at_startup: bool,

    // Aba Avançado (Advanced)
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,

    #[serde(default = "default_zero_i32")]
    pub x_offset: i32,

    #[serde(default = "default_zero_i32")]
    pub y_offset: i32,

    #[serde(default = "default_polling_interval")]
    pub polling_interval: u64,

    #[serde(default = "default_true")]
    pub toolbar_mode: bool,

    #[serde(default = "default_false")]
    pub click_through: bool,

    #[serde(default = "default_true")]
    pub show_tray_icon: bool,

    #[serde(default = "default_false")]
    pub collapse_menu_bar: bool,

    #[serde(default = "default_false")]
    pub initially_hidden: bool,

    // Aba Personalização (Customize)
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u32,

    #[serde(default = "default_false")]
    pub auto_bg_color: bool,

    #[serde(default = "default_bg_color")]
    pub bg_color: String,

    #[serde(default = "default_bg_opacity")]
    pub bg_opacity: f32,

    #[serde(default = "default_text_align")]
    pub text_align: String, // "Left", "Right"

    #[serde(default = "default_font_size")]
    pub font_size: u32,

    #[serde(default = "default_font_color")]
    pub font_color: String,

    #[serde(default = "default_alert_color")]
    pub alert_font_color: String,

    #[serde(default = "default_true")]
    pub alert_blink: bool,

    #[serde(default = "default_true")]
    pub show_machine_name: bool,

    #[serde(default = "default_true")]
    pub show_clock: bool,

    #[serde(default = "default_true")]
    pub clock_24hr: bool,

    #[serde(default = "default_date_format")]
    pub date_setting: String, // "Disabled", "Short", "Normal", "Long"

    // Aba Monitores & Hardware
    #[serde(default = "default_true")]
    pub enable_cpu: bool,

    #[serde(default = "default_true")]
    pub enable_ram: bool,

    #[serde(default = "default_true")]
    pub enable_gpu: bool,

    #[serde(default = "default_true")]
    pub enable_disks: bool,

    #[serde(default = "default_true")]
    pub enable_network: bool,

    #[serde(default = "default_true")]
    pub show_cpu_cores: bool,

    #[serde(default = "default_false")]
    pub show_all_disks: bool,

    #[serde(default = "default_true")]
    pub show_sparklines: bool,

    #[serde(default = "default_module_order")]
    pub module_order: Vec<String>,

    // Aba Atalhos de Teclado
    #[serde(default = "default_toggle_hotkey")]
    pub hotkey_toggle: String,

    #[serde(default = "default_empty_string")]
    pub hotkey_cycle_edge: String,

    #[serde(default = "default_empty_string")]
    pub hotkey_cycle_screen: String,

    #[serde(default = "default_empty_string")]
    pub hotkey_reserve_space: String,

    // Nomes personalizados editáveis na interface (vazio = detectado automaticamente)
    #[serde(default = "default_empty_string")]
    pub custom_hostname: String,

    #[serde(default = "default_empty_string")]
    pub custom_cpu_name: String,

    #[serde(default = "default_empty_string")]
    pub custom_gpu_name: String,

    #[serde(default = "default_empty_string")]
    pub custom_net_name: String,
}

fn default_dock_edge() -> String { "Right".to_string() }
fn default_screen_index() -> usize { 0 }
fn default_culture() -> String { "en-US".to_string() }
fn default_true() -> bool { true }
fn default_false() -> bool { false }
fn default_zero_i32() -> i32 { 0 }
fn default_ui_scale() -> f32 { 1.0 }
fn default_polling_interval() -> u64 { 1000 }
fn default_sidebar_width() -> u32 { 300 }
fn default_bg_color() -> String { "#0f1015".to_string() }
fn default_bg_opacity() -> f32 { 0.95 }
fn default_text_align() -> String { "Left".to_string() }
fn default_font_size() -> u32 { 14 }
fn default_font_color() -> String { "#ffffff".to_string() }
fn default_alert_color() -> String { "#ef4444".to_string() }
fn default_date_format() -> String { "Normal".to_string() }
fn default_module_order() -> Vec<String> {
    vec![
        "cpu".to_string(),
        "ram".to_string(),
        "gpu".to_string(),
        "disks".to_string(),
        "network".to_string(),
    ]
}
fn default_toggle_hotkey() -> String { "Ctrl+Shift+S".to_string() }
fn default_empty_string() -> String { String::new() }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            dock_edge: default_dock_edge(),
            screen_index: default_screen_index(),
            culture: default_culture(),
            use_app_bar: default_true(),
            always_top: default_true(),
            auto_update: default_true(),
            run_at_startup: default_false(),
            ui_scale: default_ui_scale(),
            x_offset: default_zero_i32(),
            y_offset: default_zero_i32(),
            polling_interval: default_polling_interval(),
            toolbar_mode: default_true(),
            click_through: default_false(),
            show_tray_icon: default_true(),
            collapse_menu_bar: default_false(),
            initially_hidden: default_false(),
            sidebar_width: default_sidebar_width(),
            auto_bg_color: default_false(),
            bg_color: default_bg_color(),
            bg_opacity: default_bg_opacity(),
            text_align: default_text_align(),
            font_size: default_font_size(),
            font_color: default_font_color(),
            alert_font_color: default_alert_color(),
            alert_blink: default_true(),
            show_machine_name: default_true(),
            show_clock: default_true(),
            clock_24hr: default_true(),
            date_setting: default_date_format(),
            enable_cpu: default_true(),
            enable_ram: default_true(),
            enable_gpu: default_true(),
            enable_disks: default_true(),
            enable_network: default_true(),
            show_cpu_cores: default_true(),
            show_all_disks: default_false(),
            show_sparklines: default_true(),
            module_order: default_module_order(),
            hotkey_toggle: default_toggle_hotkey(),
            hotkey_cycle_edge: default_empty_string(),
            hotkey_cycle_screen: default_empty_string(),
            hotkey_reserve_space: default_empty_string(),
            custom_hostname: default_empty_string(),
            custom_cpu_name: default_empty_string(),
            custom_gpu_name: default_empty_string(),
            custom_net_name: default_empty_string(),
        }
    }
}

impl AppConfig {
    pub fn is_dock_right(&self) -> bool {
        self.dock_edge.eq_ignore_ascii_case("Right")
    }

    pub fn set_dock_right(&mut self, is_right: bool) {
        self.dock_edge = if is_right {
            "Right".to_string()
        } else {
            "Left".to_string()
        };
    }

    pub fn config_path() -> PathBuf {
        let local = PathBuf::from("Settings.json");
        if local.exists() {
            return local;
        }

        // Tenta APPDATA / LOCALAPPDATA no Windows
        #[cfg(target_os = "windows")]
        if let Ok(appdata) = std::env::var("LOCALAPPDATA").or_else(|_| std::env::var("APPDATA")) {
            let mut p = PathBuf::from(appdata);
            p.push("ZenithBar");
            let _ = fs::create_dir_all(&p);
            p.push("Settings.json");
            return p;
        }

        // Tenta XDG_CONFIG_HOME ou ~/.config no Linux
        #[cfg(not(target_os = "windows"))]
        {
            let config_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
                PathBuf::from(xdg).join("zenithbar")
            } else if let Ok(home) = std::env::var("HOME") {
                PathBuf::from(home).join(".config").join("zenithbar")
            } else {
                return local;
            };
            let _ = fs::create_dir_all(&config_dir);
            config_dir.join("Settings.json")
        }

        #[cfg(target_os = "windows")]
        local
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<Self>(&content) {
                let mut cfg = cfg;
                cfg.sidebar_width = cfg.sidebar_width.clamp(180, 360);
                return cfg;
            }
        }

        // Caso exista um config.toml legado da versão anterior, migra automaticamente
        let toml_legacy = PathBuf::from("config.toml");
        if toml_legacy.exists() {
            if let Ok(content) = fs::read_to_string(&toml_legacy) {
                #[derive(Deserialize)]
                struct LegacyConfig {
                    refresh_interval_ms: Option<u64>,
                    dock_right: Option<bool>,
                    use_appbar: Option<bool>,
                    bar_width: Option<u32>,
                    monitor_index: Option<usize>,
                    show_all_disks: Option<bool>,
                    show_gpu: Option<bool>,
                    show_sparklines: Option<bool>,
                }
                if let Ok(legacy) = toml::from_str::<LegacyConfig>(&content) {
                    let mut cfg = Self::default();
                    if let Some(r) = legacy.refresh_interval_ms { cfg.polling_interval = r; }
                    if let Some(d) = legacy.dock_right { cfg.set_dock_right(d); }
                    if let Some(u) = legacy.use_appbar { cfg.use_app_bar = u; }
                    if let Some(w) = legacy.bar_width { cfg.sidebar_width = w.clamp(180, 360); }
                    if let Some(m) = legacy.monitor_index { cfg.screen_index = m; }
                    if let Some(ad) = legacy.show_all_disks { cfg.show_all_disks = ad; }
                    if let Some(g) = legacy.show_gpu { cfg.enable_gpu = g; }
                    if let Some(s) = legacy.show_sparklines { cfg.show_sparklines = s; }
                    let _ = cfg.save();
                    let mut cfg = cfg;
                cfg.sidebar_width = cfg.sidebar_width.clamp(180, 360);
                return cfg;
                }
            }
        }

        let cfg = Self::default();
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        let serialized = serde_json::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
        fs::write(path, serialized)?;

        self.apply_startup();

        Ok(())
    }

    pub fn apply_startup(&self) {
        #[cfg(target_os = "windows")]
        self.apply_windows_startup();

        #[cfg(not(target_os = "windows"))]
        self.apply_linux_startup();
    }

    #[cfg(not(target_os = "windows"))]
    pub fn apply_linux_startup(&self) {
        let autostart_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("autostart")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config").join("autostart")
        } else {
            return;
        };

        let desktop_file = autostart_dir.join("zenithbar.desktop");

        if self.run_at_startup {
            let _ = fs::create_dir_all(&autostart_dir);
            let exe_path = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "zenithbar".to_string());

            if let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
            {
                let icon_dir = data_home.join("icons/hicolor/512x512/apps");
                let icon_dest = icon_dir.join("zenithbar.png");
                if !icon_dest.exists() {
                    let _ = fs::create_dir_all(&icon_dir);
                    const ICON_BYTES: &[u8] = include_bytes!("../assets/icon.png");
                    let _ = fs::write(&icon_dest, ICON_BYTES);
                }
            }

            let content = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=ZenithBar\n\
                 Comment=Modern, lightweight hardware telemetry sidebar\n\
                 Exec={}\n\
                 Icon=zenithbar\n\
                 Terminal=false\n\
                 Categories=System;Monitor;\n",
                exe_path
            );
            let _ = fs::write(desktop_file, content);
        } else if desktop_file.exists() {
            let _ = fs::remove_file(desktop_file);
        }
    }

    #[cfg(target_os = "windows")]
    pub fn apply_windows_startup(&self) {
        use windows::core::HSTRING;
        use windows::Win32::System::Registry::{
            RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER,
            KEY_SET_VALUE, REG_SZ,
        };

        unsafe {
            let mut hkey = windows::Win32::System::Registry::HKEY::default();
            let subkey = HSTRING::from("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
            let key_name = HSTRING::from("ZenithBar");

            let res = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                &subkey,
                Some(0),
                KEY_SET_VALUE,
                &mut hkey,
            );

            if res.is_ok() {
                if self.run_at_startup {
                    if let Ok(exe_path) = std::env::current_exe() {
                        let path_str = format!("\"{}\"", exe_path.to_string_lossy());
                        let wide: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
                        let bytes: &[u8] = std::slice::from_raw_parts(
                            wide.as_ptr() as *const u8,
                            wide.len() * 2,
                        );
                        let _ = RegSetValueExW(
                            hkey,
                            &key_name,
                            Some(0),
                            REG_SZ,
                            Some(bytes),
                        );
                    }
                } else {
                    let _ = RegDeleteValueW(hkey, &key_name);
                }
                let _ = RegCloseKey(hkey);
            }
        }
    }
}
