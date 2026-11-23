#![windows_subsystem = "windows"]

mod appbar;
mod config;
mod dwm;
mod i18n;
mod monitor;
mod single_instance;
mod telemetry;
mod tray;
mod updater;

#[allow(unused_imports)]
use appbar::{win32::AppBarManager, DockEdge};
use chrono::Local;
use config::AppConfig;
use monitor::win32::list_monitors;
#[cfg(not(target_os = "windows"))]
use monitor::win32::list_monitors_for_window;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use telemetry::{TelemetryEngine, TelemetrySnapshot};
use tray::win32::TrayIcon;

slint::include_modules!();

const MAX_LOG_BYTES: u64 = 1_048_576; // 1 MB
static LOG_LOCK: Mutex<()> = Mutex::new(());
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

fn resolve_log_path() -> PathBuf {
    LOG_PATH
        .get_or_init(|| {
            if std::path::Path::new("Settings.json").exists() {
                return PathBuf::from("zenithbar.log");
            }

            #[cfg(target_os = "windows")]
            if let Ok(appdata) = std::env::var("LOCALAPPDATA").or_else(|_| std::env::var("APPDATA"))
            {
                let dir = PathBuf::from(appdata).join("ZenithBar");
                let _ = std::fs::create_dir_all(&dir);
                return dir.join("zenithbar.log");
            }

            #[cfg(not(target_os = "windows"))]
            {
                let state_dir = if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
                    if !xdg.is_empty() {
                        Some(PathBuf::from(xdg).join("zenithbar"))
                    } else {
                        None
                    }
                } else {
                    None
                }
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|h| PathBuf::from(h).join(".local/state/zenithbar"))
                });

                if let Some(dir) = state_dir {
                    let _ = std::fs::create_dir_all(&dir);
                    return dir.join("zenithbar.log");
                }
            }

            PathBuf::from("zenithbar.log")
        })
        .clone()
}

fn rotate_log_file(path: &std::path::Path, max_bytes: u64) {
    let _guard = LOG_LOCK.lock();
    if let Ok(meta) = std::fs::metadata(path)
        && meta.len() > max_bytes {
            let backup = path.with_extension("log.old");
            let _ = std::fs::remove_file(&backup);
            let _ = std::fs::rename(path, backup);
        }
}

fn log_msg(msg: &str) {
    use std::io::Write;
    let _guard = LOG_LOCK.lock();
    let log_path = resolve_log_path();
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| writeln!(f, "[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg));
}

#[cfg(any(not(target_os = "windows"), test))]
fn should_prefer_xwayland(
    wayland_display: Option<&str>,
    xdg_session_type: Option<&str>,
    x11_display: Option<&str>,
    winit_backend: Option<&str>,
) -> bool {
    let already_set = winit_backend.is_some_and(|v| !v.trim().is_empty());
    if already_set {
        return false;
    }
    let has_x11 = x11_display.is_some_and(|v| !v.trim().is_empty());
    if !has_x11 {
        return false;
    }
    
    wayland_display.is_some_and(|v| !v.trim().is_empty())
        || xdg_session_type.is_some_and(|v| v.trim().eq_ignore_ascii_case("wayland"))
}

#[cfg(not(target_os = "windows"))]
fn configure_linux_display_backend() {
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let xdg_session = std::env::var("XDG_SESSION_TYPE").ok();
    let x11_display = std::env::var("DISPLAY").ok();
    let winit_backend = std::env::var("WINIT_UNIX_BACKEND").ok();

    if should_prefer_xwayland(
        wayland_display.as_deref(),
        xdg_session.as_deref(),
        x11_display.as_deref(),
        winit_backend.as_deref(),
    ) {
        // Native Wayland xdg_toplevel ignores client positioning and centers windows;
        // XWayland preserves edge docking and X11 hints when DISPLAY is available.
        unsafe {
            std::env::set_var("WINIT_UNIX_BACKEND", "x11");
        }
    }
}

fn appbar_hide(ui: &AppWindow, ab: &mut Option<AppBarManager>, is_vis: &RefCell<bool>) {
    if let Some(ab_mgr) = ab.as_mut() {
        ab_mgr.unregister();
        ab_mgr.hide_window();
    } else {
        let _ = ui.hide();
    }
    *is_vis.borrow_mut() = false;
    log_msg("Barra ocultada para a bandeja (AppBar liberado).");
}

fn appbar_show(ui: &AppWindow, ab: &mut Option<AppBarManager>, is_vis: &RefCell<bool>) {
    if let Some(ab_mgr) = ab.as_mut() {
        ab_mgr.show_window();
        ab_mgr.register();
    } else {
        let _ = ui.show();
    }
    *is_vis.borrow_mut() = true;
    log_msg("Barra reexibida na tela (AppBar registrado).");
}

fn apply_locale_to_app(ui: &AppWindow, s: &i18n::I18nStrings) {
    ui.set_i18n_cpu_title(s.cpu_title.clone().into());
    ui.set_i18n_ram_title(s.ram_title.clone().into());
    ui.set_i18n_gpu_title(s.gpu_title.clone().into());
    ui.set_i18n_disks_title(s.disks_title.clone().into());
    ui.set_i18n_network_title(s.network_title.clone().into());
    ui.set_i18n_clock(s.clock.clone().into());
    ui.set_i18n_temp(s.temp.clone().into());
    ui.set_i18n_usage_per_core(s.usage_per_core.clone().into());
    ui.set_i18n_usage(s.usage.clone().into());
    ui.set_i18n_available(s.available.clone().into());
    ui.set_i18n_fan(s.fan.clone().into());
    ui.set_i18n_local_ip(s.local_ip.clone().into());
    ui.set_i18n_wan_ip(s.wan_ip.clone().into());
    ui.set_i18n_hotkey_hint(s.hotkey_hint.clone().into());
}

fn apply_locale_to_settings(sw: &SettingsWindow, s: &i18n::I18nStrings) {
    sw.set_i18n_window_title(s.window_title.clone().into());
    sw.set_i18n_header_title(s.header_title.clone().into());
    sw.set_i18n_header_desc(s.header_desc.clone().into());
    sw.set_i18n_tab_general(s.tab_general.clone().into());
    sw.set_i18n_tab_advanced(s.tab_advanced.clone().into());
    sw.set_i18n_tab_style(s.tab_style.clone().into());
    sw.set_i18n_tab_monitors(s.tab_monitors.clone().into());
    sw.set_i18n_tab_hotkeys(s.tab_hotkeys.clone().into());

    sw.set_i18n_language_label(s.language_label.clone().into());
    sw.set_i18n_language_desc(s.language_desc.clone().into());
    sw.set_i18n_dock_label(s.dock_label.clone().into());
    sw.set_i18n_dock_desc(s.dock_desc.clone().into());
    sw.set_i18n_dock_left(s.dock_left.clone().into());
    sw.set_i18n_dock_right(s.dock_right.clone().into());
    sw.set_i18n_monitor_label(s.monitor_label.clone().into());
    sw.set_i18n_monitor_desc(s.monitor_desc.clone().into());
    sw.set_i18n_monitor_primary(s.monitor_primary.clone().into());
    sw.set_i18n_monitor_2(s.monitor_2.clone().into());
    sw.set_i18n_monitor_3(s.monitor_3.clone().into());
    sw.set_i18n_appbar_label(s.appbar_label.clone().into());
    sw.set_i18n_appbar_desc(s.appbar_desc.clone().into());
    sw.set_i18n_always_top_label(s.always_top_label.clone().into());
    sw.set_i18n_always_top_desc(s.always_top_desc.clone().into());
    sw.set_i18n_startup_label(s.startup_label.clone().into());
    sw.set_i18n_startup_desc(s.startup_desc.clone().into());
    sw.set_i18n_initially_hidden_label(s.initially_hidden_label.clone().into());
    sw.set_i18n_initially_hidden_desc(s.initially_hidden_desc.clone().into());

    sw.set_i18n_polling_label(s.polling_label.clone().into());
    sw.set_i18n_polling_desc(s.polling_desc.clone().into());
    sw.set_i18n_tray_icon_label(s.tray_icon_label.clone().into());
    sw.set_i18n_tray_icon_desc(s.tray_icon_desc.clone().into());
    sw.set_i18n_click_through_label(s.click_through_label.clone().into());
    sw.set_i18n_click_through_desc(s.click_through_desc.clone().into());
    sw.set_i18n_collapse_menu_label(s.collapse_menu_label.clone().into());
    sw.set_i18n_collapse_menu_desc(s.collapse_menu_desc.clone().into());
    sw.set_i18n_auto_update_label(s.auto_update_label.clone().into());
    sw.set_i18n_auto_update_desc(s.auto_update_desc.clone().into());
    sw.set_i18n_btn_check_update(s.btn_check_update.clone().into());

    sw.set_i18n_sidebar_width_label(s.sidebar_width_label.clone().into());
    sw.set_i18n_sidebar_width_desc(s.sidebar_width_desc.clone().into());
    sw.set_i18n_bg_opacity_label(s.bg_opacity_label.clone().into());
    sw.set_i18n_bg_opacity_desc(s.bg_opacity_desc.clone().into());
    sw.set_i18n_text_align_label(s.text_align_label.clone().into());
    sw.set_i18n_text_align_desc(s.text_align_desc.clone().into());
    sw.set_i18n_align_left(s.align_left.clone().into());
    sw.set_i18n_align_right(s.align_right.clone().into());
    sw.set_i18n_show_hostname_label(s.show_hostname_label.clone().into());
    sw.set_i18n_show_hostname_desc(s.show_hostname_desc.clone().into());
    sw.set_i18n_show_clock_label(s.show_clock_label.clone().into());
    sw.set_i18n_show_clock_desc(s.show_clock_desc.clone().into());
    sw.set_i18n_clock_24hr_label(s.clock_24hr_label.clone().into());
    sw.set_i18n_clock_24hr_desc(s.clock_24hr_desc.clone().into());
    sw.set_i18n_alert_blink_label(s.alert_blink_label.clone().into());
    sw.set_i18n_alert_blink_desc(s.alert_blink_desc.clone().into());

    sw.set_i18n_enable_cpu_label(s.enable_cpu_label.clone().into());
    sw.set_i18n_enable_cpu_desc(s.enable_cpu_desc.clone().into());
    sw.set_i18n_show_cores_label(s.show_cores_label.clone().into());
    sw.set_i18n_show_cores_desc(s.show_cores_desc.clone().into());
    sw.set_i18n_pawnio_label(s.pawnio_label.clone().into());
    sw.set_i18n_pawnio_desc(s.pawnio_desc.clone().into());
    sw.set_i18n_btn_setup_pawnio(s.btn_setup_pawnio.clone().into());
    sw.set_i18n_enable_ram_label(s.enable_ram_label.clone().into());
    sw.set_i18n_enable_ram_desc(s.enable_ram_desc.clone().into());
    sw.set_i18n_enable_gpu_label(s.enable_gpu_label.clone().into());
    sw.set_i18n_enable_gpu_desc(s.enable_gpu_desc.clone().into());
    sw.set_i18n_enable_disks_label(s.enable_disks_label.clone().into());
    sw.set_i18n_enable_disks_desc(s.enable_disks_desc.clone().into());
    sw.set_i18n_show_all_disks_label(s.show_all_disks_label.clone().into());
    sw.set_i18n_show_all_disks_desc(s.show_all_disks_desc.clone().into());
    sw.set_i18n_enable_network_label(s.enable_network_label.clone().into());
    sw.set_i18n_enable_network_desc(s.enable_network_desc.clone().into());
    sw.set_i18n_show_sparklines_label(s.show_sparklines_label.clone().into());
    sw.set_i18n_show_sparklines_desc(s.show_sparklines_desc.clone().into());

    sw.set_i18n_hotkey_toggle_label(s.hotkey_toggle_label.clone().into());
    sw.set_i18n_hotkey_toggle_desc(s.hotkey_toggle_desc.clone().into());
    sw.set_i18n_hotkey_edge_label(s.hotkey_edge_label.clone().into());
    sw.set_i18n_hotkey_edge_desc(s.hotkey_edge_desc.clone().into());
    sw.set_i18n_hotkey_screen_label(s.hotkey_screen_label.clone().into());
    sw.set_i18n_hotkey_screen_desc(s.hotkey_screen_desc.clone().into());
    sw.set_i18n_hotkey_reserve_label(s.hotkey_reserve_label.clone().into());
    sw.set_i18n_hotkey_reserve_desc(s.hotkey_reserve_desc.clone().into());

    sw.set_i18n_btn_cancel(s.btn_cancel.clone().into());
    sw.set_i18n_btn_apply(s.btn_apply.clone().into());
    sw.set_i18n_btn_save(s.btn_save.clone().into());
}

fn populate_settings_ui(sw: &SettingsWindow, cfg: &AppConfig, monitor_count: usize) {
    sw.set_culture(cfg.culture.clone().into());
    let strings = i18n::load_locale(&cfg.culture);
    apply_locale_to_settings(sw, &strings);

    sw.set_dock_right(cfg.is_dock_right());
    sw.set_monitor_idx(cfg.screen_index as i32);
    sw.set_monitor_count(monitor_count as i32);
    sw.set_use_app_bar(cfg.use_app_bar);
    sw.set_always_top(cfg.always_top);
    sw.set_run_at_startup(cfg.run_at_startup);
    sw.set_initially_hidden(cfg.initially_hidden);

    sw.set_polling_interval(cfg.polling_interval as i32);
    sw.set_ui_scale(cfg.ui_scale);
    sw.set_show_tray_icon(cfg.show_tray_icon);
    sw.set_click_through(cfg.click_through);
    sw.set_collapse_menu_bar(cfg.collapse_menu_bar);
    sw.set_auto_update(cfg.auto_update);

    sw.set_sidebar_width(cfg.sidebar_width as i32);
    sw.set_bg_opacity(cfg.bg_opacity);
    sw.set_text_align_right(cfg.text_align.eq_ignore_ascii_case("Right"));
    sw.set_show_machine_name(cfg.show_machine_name);
    sw.set_show_clock(cfg.show_clock);
    sw.set_clock_24hr(cfg.clock_24hr);
    sw.set_alert_blink(cfg.alert_blink);

    sw.set_enable_cpu(cfg.enable_cpu);
    sw.set_show_cpu_cores(cfg.show_cpu_cores);
    sw.set_show_pawnio_setup(cfg!(target_os = "windows"));
    sw.set_enable_ram(cfg.enable_ram);
    sw.set_enable_gpu(cfg.enable_gpu);
    sw.set_enable_disks(cfg.enable_disks);
    sw.set_show_all_disks(cfg.show_all_disks);
    sw.set_enable_network(cfg.enable_network);
    sw.set_show_sparklines(cfg.show_sparklines);

    sw.set_hotkey_toggle(cfg.hotkey_toggle.clone().into());
    sw.set_hotkey_cycle_edge(if cfg.hotkey_cycle_edge.is_empty() { "Ctrl+Shift+E".into() } else { cfg.hotkey_cycle_edge.clone().into() });
    sw.set_hotkey_cycle_screen(if cfg.hotkey_cycle_screen.is_empty() { "Ctrl+Shift+M".into() } else { cfg.hotkey_cycle_screen.clone().into() });
    sw.set_hotkey_reserve_space(if cfg.hotkey_reserve_space.is_empty() { "Ctrl+Shift+R".into() } else { cfg.hotkey_reserve_space.clone().into() });
}

enum TelemetryControl {
    SetInterval(u64),
    ReloadSensors,
}

fn sync_monitor_height(ui: &AppWindow, monitors: &[monitor::MonitorDevice], screen_index: usize) {
    let target_h = if screen_index < monitors.len() {
        monitors[screen_index].work_height
    } else {
        monitors.first().map(|m| m.work_height).unwrap_or(1080)
    };
    let scale = ui.window().scale_factor();
    let logical_h = if scale > 0.0 { target_h as f32 / scale } else { target_h as f32 };
    ui.set_sidebar_height_px(logical_h);
}

#[allow(clippy::too_many_arguments)]
fn apply_settings_to_app(
    ui: &AppWindow,
    sw: &SettingsWindow,
    cfg: &mut AppConfig,
    ab: &mut Option<AppBarManager>,
    tray_holder: &mut Option<TrayIcon>,
    hwnd_raw: isize,
    monitors: &[monitor::MonitorDevice],
    ctrl_tx: &std::sync::mpsc::Sender<TelemetryControl>,
) {
    let edge_changed = cfg.is_dock_right() != sw.get_dock_right();
    let monitor_changed = cfg.screen_index != (sw.get_monitor_idx().max(0) as usize);
    let width_changed = cfg.sidebar_width != (sw.get_sidebar_width().clamp(180, 360) as u32);
    let appbar_changed = cfg.use_app_bar != sw.get_use_app_bar();
    let always_top_changed = cfg.always_top != sw.get_always_top();
    let click_through_changed = cfg.click_through != sw.get_click_through();
    let tray_changed = cfg.show_tray_icon != sw.get_show_tray_icon();

    cfg.set_dock_right(sw.get_dock_right());
    cfg.screen_index = sw.get_monitor_idx().max(0) as usize;
    cfg.use_app_bar = sw.get_use_app_bar();
    cfg.always_top = sw.get_always_top();
    cfg.run_at_startup = sw.get_run_at_startup();
    cfg.initially_hidden = sw.get_initially_hidden();

    cfg.polling_interval = sw.get_polling_interval().clamp(100, 10000) as u64;
    let _ = ctrl_tx.send(TelemetryControl::SetInterval(cfg.polling_interval));
    cfg.ui_scale = sw.get_ui_scale();
    cfg.show_tray_icon = sw.get_show_tray_icon();
    cfg.click_through = sw.get_click_through();
    cfg.collapse_menu_bar = sw.get_collapse_menu_bar();
    cfg.auto_update = sw.get_auto_update();

    cfg.sidebar_width = sw.get_sidebar_width().clamp(180, 360) as u32;
    cfg.bg_opacity = sw.get_bg_opacity();
    cfg.text_align = if sw.get_text_align_right() { "Right".into() } else { "Left".into() };
    cfg.show_machine_name = sw.get_show_machine_name();
    cfg.show_clock = sw.get_show_clock();
    cfg.clock_24hr = sw.get_clock_24hr();
    cfg.alert_blink = sw.get_alert_blink();

    cfg.enable_cpu = sw.get_enable_cpu();
    cfg.show_cpu_cores = sw.get_show_cpu_cores();
    cfg.enable_ram = sw.get_enable_ram();
    cfg.enable_gpu = sw.get_enable_gpu();
    cfg.enable_disks = sw.get_enable_disks();
    cfg.show_all_disks = sw.get_show_all_disks();
    cfg.enable_network = sw.get_enable_network();
    cfg.show_sparklines = sw.get_show_sparklines();

    cfg.culture = sw.get_culture().to_string();
    let strings = i18n::load_locale(&cfg.culture);
    apply_locale_to_app(ui, &strings);
    apply_locale_to_settings(sw, &strings);

    let _ = cfg.save();

    // Atualiza a janela principal do Slint
    ui.set_is_docked_right(cfg.is_dock_right());
    ui.set_is_appbar(cfg.use_app_bar);
    ui.set_enable_cpu(cfg.enable_cpu);
    ui.set_show_cpu_cores(cfg.show_cpu_cores);
    ui.set_enable_ram(cfg.enable_ram);
    ui.set_enable_gpu(cfg.enable_gpu);
    ui.set_enable_disks(cfg.enable_disks);
    ui.set_show_all_disks(cfg.show_all_disks);
    ui.set_enable_network(cfg.enable_network);
    ui.set_show_sparklines(cfg.show_sparklines);
    ui.set_show_machine_name(cfg.show_machine_name);
    ui.set_show_clock(cfg.show_clock);
    ui.set_collapse_menu_bar(cfg.collapse_menu_bar);
    ui.set_sidebar_width_px(cfg.sidebar_width as f32);
    ui.set_bg_opacity_val(cfg.bg_opacity);
    ui.set_alert_blink_enabled(cfg.alert_blink);
    ui.set_text_align_right(cfg.text_align.eq_ignore_ascii_case("Right"));

    sync_monitor_height(ui, monitors, cfg.screen_index);
    ui.window().request_redraw();

    tray::win32::set_always_top_state(cfg.always_top);
    tray::win32::set_tray_culture(&cfg.culture);

    if tray_changed {
        if cfg.show_tray_icon {
            if tray_holder.is_none() {
                let mut tray = TrayIcon::new(hwnd_raw);
                if tray.init() {
                    *tray_holder = Some(tray);
                }
            }
        } else {
            *tray_holder = None;
        }
    }

    // Atualiza o AppBar nativo (Win32 / Linux)
    if let Some(ab_mgr) = ab.as_mut() {
        if always_top_changed {
            ab_mgr.set_always_top(cfg.always_top);
        }
        if click_through_changed {
            ab_mgr.set_click_through(cfg.click_through);
        }
        if monitor_changed {
            let m_idx = cfg.screen_index;
            if m_idx < monitors.len() {
                ab_mgr.set_monitor(monitors[m_idx].clone());
            }
        }
        if width_changed {
            ab_mgr.set_width(cfg.sidebar_width as i32);
        }
        if edge_changed {
            let edge = if cfg.is_dock_right() { DockEdge::Right } else { DockEdge::Left };
            ab_mgr.set_edge(edge);
        }
        if appbar_changed {
            ab_mgr.set_appbar_mode(cfg.use_app_bar);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(target_os = "windows"))]
    configure_linux_display_backend();

    // Setup panic hook with log file and native Windows Message Box
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("FATAL ERROR:\n{:?}\n", info);
        log_msg(&msg);
        let crash_path = resolve_log_path().with_file_name("zenithbar_crash.log");
        let _ = std::fs::write(crash_path, &msg);

        #[cfg(target_os = "windows")]
        unsafe {
            use windows::core::HSTRING;
            use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            let title = HSTRING::from("ZenithBar - Erro");
            let body = HSTRING::from(format!(
                "Ocorreu um erro ao executar o ZenithBar:\n\n{}\n\nConsulte o arquivo zenithbar.log para mais detalhes.",
                info
            ));
            let _ = MessageBoxW(None, &body, &title, MB_OK | MB_ICONERROR);
        }
    }));

    rotate_log_file(&resolve_log_path(), MAX_LOG_BYTES);

    #[cfg(target_os = "windows")]
    {
        if std::env::args().any(|a| a == "--sensor-service") {
            std::process::exit(telemetry::pawnio_win::run_sensor_service_entry());
        }
        if std::env::args().any(|a| a == "--setup-pawnio") {
            let cfg = AppConfig::load();
            std::process::exit(telemetry::pawnio_win::run_setup_pawnio_cli(&cfg.culture));
        }
    }

    let wait_timeout = if std::env::args().any(|a| a == "--restarted") {
        Duration::from_secs(3)
    } else {
        Duration::ZERO
    };
    let _instance_guard = match single_instance::SingleInstanceGuard::acquire(wait_timeout) {
        Some(guard) => guard,
        None => {
            log_msg("Outra instância do ZenithBar já está em execução. Encerrando nova instância.");
            return Ok(());
        }
    };

    log_msg("--- ZenithBar Iniciando ---");

    log_msg("1. Carregando configuraÃ§Ãµes 1:1...");
    let config = Rc::new(RefCell::new(AppConfig::load()));

    log_msg("2. Criando janela principal AppWindow (no-frame, STA)...");
    let ui = match AppWindow::new() {
        Ok(win) => {
            log_msg("Janela AppWindow instanciada com sucesso.");
            win
        }
        Err(e) => {
            let err_msg = format!("Falha ao criar AppWindow: {:?}", e);
            log_msg(&err_msg);
            #[cfg(target_os = "windows")]
            unsafe {
                use windows::core::HSTRING;
                use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
                let title = HSTRING::from("ZenithBar - Erro de RenderizaÃ§Ã£o");
                let body = HSTRING::from(format!(
                    "Falha ao inicializar a interface grÃ¡fica Slint:\n{}\n\nVerifique se os drivers de vÃ­deo estÃ£o atualizados.",
                    err_msg
                ));
                let _ = MessageBoxW(None, &body, &title, MB_OK | MB_ICONERROR);
            }
            return Err(Box::new(e));
        }
    };

    log_msg("3. Criando janela separada de ConfiguraÃ§Ãµes de 5 Abas 1:1...");
    let settings_ui = match SettingsWindow::new() {
        Ok(win) => {
            log_msg("Janela SettingsWindow instanciada com sucesso.");
            win
        }
        Err(e) => {
            log_msg(&format!("Aviso: Falha ao instanciar SettingsWindow: {:?}", e));
            return Err(Box::new(e));
        }
    };

    // Propriedades iniciais da UI Principal
    {
        let cfg = config.borrow();
        let init_strings = i18n::load_locale(&cfg.culture);
        apply_locale_to_app(&ui, &init_strings);
        apply_locale_to_settings(&settings_ui, &init_strings);
        settings_ui.set_culture(cfg.culture.clone().into());

        ui.set_is_docked_right(cfg.is_dock_right());
        ui.set_is_appbar(cfg.use_app_bar);
        ui.set_enable_cpu(cfg.enable_cpu);
        ui.set_show_cpu_cores(cfg.show_cpu_cores);
        ui.set_enable_ram(cfg.enable_ram);
        ui.set_enable_gpu(cfg.enable_gpu);
        ui.set_enable_disks(cfg.enable_disks);
        ui.set_show_all_disks(cfg.show_all_disks);
        ui.set_enable_network(cfg.enable_network);
        ui.set_show_sparklines(cfg.show_sparklines);
        ui.set_show_machine_name(cfg.show_machine_name);
        ui.set_show_clock(cfg.show_clock);
        ui.set_collapse_menu_bar(cfg.collapse_menu_bar);
        ui.set_sidebar_width_px(cfg.sidebar_width as f32);
        ui.set_bg_opacity_val(cfg.bg_opacity);
        ui.set_alert_blink_enabled(cfg.alert_blink);
        ui.set_text_align_right(cfg.text_align.eq_ignore_ascii_case("Right"));
    }

    log_msg("4. Inicializando thread de telemetria isolada em background (MTA)...");
    let (tx, rx) = std::sync::mpsc::sync_channel::<TelemetrySnapshot>(1);
    let (ctrl_tx, ctrl_rx) = std::sync::mpsc::channel::<TelemetryControl>();
    let initial_polling_interval = config.borrow().polling_interval.clamp(100, 10000);
    std::thread::spawn(move || {
        log_msg("[Telemetry Thread] Iniciada com sucesso.");
        let mut engine = TelemetryEngine::new();
        let mut interval = initial_polling_interval;
        loop {
            let snap = engine.snapshot();
            match tx.try_send(snap) {
                Ok(()) | Err(std::sync::mpsc::TrySendError::Full(_)) => {}
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
            }
            match ctrl_rx.recv_timeout(Duration::from_millis(interval)) {
                Ok(first_cmd) => {
                    let mut reload = false;
                    let mut apply_cmd = |cmd: TelemetryControl| match cmd {
                        TelemetryControl::SetInterval(next) => {
                            interval = next.clamp(100, 10000);
                            log_msg(&format!("[Telemetry Thread] Novo intervalo ativo: {} ms", interval));
                        }
                        TelemetryControl::ReloadSensors => {
                            reload = true;
                        }
                    };
                    apply_cmd(first_cmd);
                    while let Ok(cmd) = ctrl_rx.try_recv() {
                        apply_cmd(cmd);
                    }
                    if reload {
                        log_msg("[Telemetry Thread] Recarregando sensores...");
                        engine.reload_sensors();
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    // Detectar monitores
    let monitors = Rc::new(RefCell::new(list_monitors()));
    log_msg(&format!("Monitores detectados inicialmente: {}", monitors.borrow().len()));
    let target_monitor = {
        let mons = monitors.borrow();
        let cfg = config.borrow();
        if cfg.screen_index < mons.len() {
            mons[cfg.screen_index].clone()
        } else {
            mons[0].clone()
        }
    };

    {
        let scale = ui.window().scale_factor();
        let logical_h = if scale > 0.0 { target_monitor.work_height as f32 / scale } else { target_monitor.work_height as f32 };
        ui.set_sidebar_height_px(logical_h);
        tray::win32::set_always_top_state(config.borrow().always_top);
        tray::win32::set_tray_culture(&config.borrow().culture);
        config.borrow().apply_startup();

        let cfg = config.borrow();
        let edge = if cfg.is_dock_right() { DockEdge::Right } else { DockEdge::Left };
        let (init_x, init_y, _, _) =
            appbar::compute_dock_geometry(edge, cfg.sidebar_width as i32, scale, &target_monitor);
        ui.window().set_position(slint::PhysicalPosition::new(init_x, init_y));
        log_msg(&format!("Posição inicial ancorada pré-configurada: ({}, {})", init_x, init_y));
    }

    let hwnd_holder: Rc<RefCell<isize>> = Rc::new(RefCell::new(0));

    // Gerenciador do AppBar e Ãcone da Bandeja mantidos vivos
    let appbar_holder: Rc<RefCell<Option<AppBarManager>>> = Rc::new(RefCell::new(None));
    let tray_holder: Rc<RefCell<Option<TrayIcon>>> = Rc::new(RefCell::new(None));

    // Estado de visibilidade e nomes detectados para fallback ao limpar nome customizado
    let is_visible = Rc::new(RefCell::new(!config.borrow().initially_hidden));
    let detected_names = Rc::new(RefCell::new(DetectedNames::default()));

    // Callbacks para edição inline dos nomes de hardware/sistema
    {
        #[derive(Clone, Copy)]
        enum NameTarget { Hostname, Cpu, Gpu, Net }

        let register_name_cb = |target: NameTarget| {
            let ui_w = ui.as_weak();
            let cfg_c = config.clone();
            let det_c = detected_names.clone();
            move |new_val: slint::SharedString| {
                let trimmed = new_val.trim().to_string();
                let mut cfg = cfg_c.borrow_mut();
                let det = det_c.borrow();
                let display = match target {
                    NameTarget::Hostname => {
                        cfg.custom_hostname = trimmed.clone();
                        if trimmed.is_empty() { det.hostname.clone() } else { trimmed }
                    }
                    NameTarget::Cpu => {
                        cfg.custom_cpu_name = trimmed.clone();
                        if trimmed.is_empty() { det.cpu.clone() } else { trimmed }
                    }
                    NameTarget::Gpu => {
                        cfg.custom_gpu_name = trimmed.clone();
                        if trimmed.is_empty() { det.gpu.clone() } else { trimmed }
                    }
                    NameTarget::Net => {
                        cfg.custom_net_name = trimmed.clone();
                        if trimmed.is_empty() { det.net.clone() } else { trimmed }
                    }
                };
                let _ = cfg.save();
                if let Some(ui) = ui_w.upgrade() {
                    match target {
                        NameTarget::Hostname => ui.set_hostname(display.into()),
                        NameTarget::Cpu => ui.set_cpu_name(display.into()),
                        NameTarget::Gpu => ui.set_gpu_name(display.into()),
                        NameTarget::Net => ui.set_net_adapter_name(display.into()),
                    }
                }
            }
        };

        ui.on_request_set_custom_hostname(register_name_cb(NameTarget::Hostname));
        ui.on_request_set_custom_cpu_name(register_name_cb(NameTarget::Cpu));
        ui.on_request_set_custom_gpu_name(register_name_cb(NameTarget::Gpu));
        ui.on_request_set_custom_net_name(register_name_cb(NameTarget::Net));
    }

    // Callbacks da UI Principal
    let ui_weak = ui.as_weak();
    let config_clone = config.clone();
    let appbar_toggle = appbar_holder.clone();
    ui.on_request_toggle_dock(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let mut cfg = config_clone.borrow_mut();
            let new_right = !cfg.is_dock_right();
            cfg.set_dock_right(new_right);
            ui.set_is_docked_right(new_right);
            let _ = cfg.save();
            let edge = if new_right { DockEdge::Right } else { DockEdge::Left };
            if let Ok(mut ab_guard) = appbar_toggle.try_borrow_mut()
                && let Some(ab) = ab_guard.as_mut() {
                    ab.set_edge(edge);
                }
        }
    });

    let ui_weak_mode = ui.as_weak();
    let config_mode = config.clone();
    let appbar_mode = appbar_holder.clone();
    ui.on_request_toggle_mode(move || {
        if let Some(ui) = ui_weak_mode.upgrade() {
            let mut cfg = config_mode.borrow_mut();
            cfg.use_app_bar = !cfg.use_app_bar;
            ui.set_is_appbar(cfg.use_app_bar);
            let _ = cfg.save();
            if let Ok(mut ab_guard) = appbar_mode.try_borrow_mut()
                && let Some(ab) = ab_guard.as_mut() {
                    ab.set_appbar_mode(cfg.use_app_bar);
                }
        }
    });

    let settings_weak = settings_ui.as_weak();
    let config_for_settings = config.clone();
    let monitors_for_settings = monitors.clone();
    ui.on_request_open_settings(move || {
        if let Some(sw) = settings_weak.upgrade() {
            let cfg = config_for_settings.borrow();
            let count = monitors_for_settings.borrow().len();
            populate_settings_ui(&sw, &cfg, count);
            let _ = sw.show();
        }
    });

    // O botÃ£o 'âœ•' no rodapÃ© oculta para a bandeja (AppBarHide) liberando o desktop
    let ui_weak_quit = ui.as_weak();
    let appbar_quit = appbar_holder.clone();
    let is_vis_quit = is_visible.clone();
    ui.on_request_quit(move || {
        if let Some(ui) = ui_weak_quit.upgrade()
            && let Ok(mut ab_guard) = appbar_quit.try_borrow_mut() {
                appbar_hide(&ui, &mut ab_guard, &is_vis_quit);
            }
    });

    // Callbacks da Janela de ConfiguraÃ§Ãµes
    let settings_weak_close = settings_ui.as_weak();
    settings_ui.on_request_close(move || {
        if let Some(sw) = settings_weak_close.upgrade() {
            let _ = sw.hide();
        }
    });

    let settings_weak_culture = settings_ui.as_weak();
    let ui_weak_culture = ui.as_weak();
    settings_ui.on_culture_changed(move |new_culture| {
        let culture_str = new_culture.to_string();
        tray::win32::set_tray_culture(&culture_str);
        let strings = i18n::load_locale(&culture_str);
        if let Some(sw) = settings_weak_culture.upgrade() {
            apply_locale_to_settings(&sw, &strings);
        }
        if let Some(ui) = ui_weak_culture.upgrade() {
            apply_locale_to_app(&ui, &strings);
        }
    });

    // APLICAR (em tempo real)
    let ctrl_apply = ctrl_tx.clone();
    let settings_weak_apply = settings_ui.as_weak();
    let config_apply = config.clone();
    let ui_weak_apply = ui.as_weak();
    let appbar_apply = appbar_holder.clone();
    let tray_apply = tray_holder.clone();
    let hwnd_apply = hwnd_holder.clone();
    let monitors_apply = monitors.clone();
    settings_ui.on_request_apply(move || {
        if let (Some(sw), Some(ui)) = (settings_weak_apply.upgrade(), ui_weak_apply.upgrade()) {
            let mut cfg = config_apply.borrow_mut();
            let mons = monitors_apply.borrow();
            if let (Ok(mut ab_guard), Ok(mut tray_guard)) =
                (appbar_apply.try_borrow_mut(), tray_apply.try_borrow_mut())
            {
                apply_settings_to_app(
                    &ui,
                    &sw,
                    &mut cfg,
                    &mut ab_guard,
                    &mut tray_guard,
                    *hwnd_apply.borrow(),
                    &mons,
                    &ctrl_apply,
                );
            }
        }
    });

    // SALVAR (aplica em tempo real e fecha a janela)
    let ctrl_save = ctrl_tx.clone();
    let settings_weak_save = settings_ui.as_weak();
    let config_save = config.clone();
    let ui_weak_save = ui.as_weak();
    let appbar_save = appbar_holder.clone();
    let tray_save = tray_holder.clone();
    let hwnd_save = hwnd_holder.clone();
    let monitors_save = monitors.clone();
    settings_ui.on_request_save(move || {
        if let (Some(sw), Some(ui)) = (settings_weak_save.upgrade(), ui_weak_save.upgrade()) {
            let mut cfg = config_save.borrow_mut();
            let mons = monitors_save.borrow();
            if let (Ok(mut ab_guard), Ok(mut tray_guard)) =
                (appbar_save.try_borrow_mut(), tray_save.try_borrow_mut())
            {
                apply_settings_to_app(
                    &ui,
                    &sw,
                    &mut cfg,
                    &mut ab_guard,
                    &mut tray_guard,
                    *hwnd_save.borrow(),
                    &mons,
                    &ctrl_save,
                );
            }
            let _ = sw.hide();
        }
    });

    let config_check = config.clone();
    settings_ui.on_request_check_updates(move || {
        let culture = config_check.borrow().culture.clone();
        std::thread::spawn(move || {
            updater::run_manual_update_check(&culture);
        });
    });

    let config_pawnio_btn = config.clone();
    settings_ui.on_request_setup_pawnio(move || {
        {
            let mut cfg = config_pawnio_btn.borrow_mut();
            if cfg.pawnio_prompt_dismissed {
                cfg.pawnio_prompt_dismissed = false;
                let _ = cfg.save();
            }
        }
        #[cfg(target_os = "windows")]
        {
            let culture = config_pawnio_btn.borrow().culture.clone();
            std::thread::spawn(move || {
                if let Err(e) = telemetry::pawnio_win::launch_elevated_pawnio_setup() {
                    log_msg(&format!("[PawnIO] Falha ao solicitar elevação UAC: {}", e));
                    let s = i18n::load_locale(&culture);
                    unsafe {
                        use windows::core::HSTRING;
                        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
                        let title = HSTRING::from(s.pawnio_prompt_title);
                        let body = HSTRING::from(format!("{} {}", s.pawnio_setup_err, e));
                        let _ = MessageBoxW(None, &body, &title, MB_OK | MB_ICONWARNING);
                    }
                }
            });
        }
    });

    let appbar_ref = appbar_holder.clone();
    let config_ref = config.clone();
    let tray_ref = tray_holder.clone();

    // Timer de recebimento de telemetria periÃ³dico (250ms) e polling de eventos da bandeja
    let ui_weak_timer = ui.as_weak();
    let settings_weak_timer = settings_ui.as_weak();
    let rx_ref = Rc::new(RefCell::new(rx));
    let models = PersistentModels::attach(&ui);
    let config_poll = config.clone();
    let detected_poll = detected_names.clone();
    let is_vis_clone = is_visible.clone();
    let appbar_tray = appbar_holder.clone();
    let monitors_tray_list = monitors.clone();
    let last_cpu_temp: Rc<RefCell<Option<f32>>> = Rc::new(RefCell::new(None));
    let last_cpu_temp_timer = last_cpu_temp.clone();
    #[cfg(target_os = "windows")]
    let pawnio_dismiss_pending = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(target_os = "windows")]
    let pawnio_dismiss_poll = pawnio_dismiss_pending.clone();

    let telemetry_timer = slint::Timer::default();
    telemetry_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            // Poll eventos da bandeja (Tray)
            if let Ok(tray_guard) = tray_ref.try_borrow()
                && let Some(tray) = tray_guard.as_ref() {
                    let events = tray.poll_events();

                    if events.toggle_vis
                        && let Some(ui) = ui_weak_timer.upgrade()
                            && let Ok(mut ab_guard) = appbar_tray.try_borrow_mut() {
                                if *is_vis_clone.borrow() {
                                    appbar_hide(&ui, &mut ab_guard, &is_vis_clone);
                                } else {
                                    appbar_show(&ui, &mut ab_guard, &is_vis_clone);
                                }
                            }

                    if events.open_settings
                        && let Some(sw) = settings_weak_timer.upgrade() {
                            let cfg = config_poll.borrow();
                            let count = monitors_tray_list.borrow().len();
                            populate_settings_ui(&sw, &cfg, count);
                            let _ = sw.show();
                        }

                    if events.toggle_side
                        && let Some(ui) = ui_weak_timer.upgrade() {
                            let mut cfg = config_poll.borrow_mut();
                            let new_right = !cfg.is_dock_right();
                            cfg.set_dock_right(new_right);
                            ui.set_is_docked_right(new_right);
                            let _ = cfg.save();
                            let edge = if new_right { DockEdge::Right } else { DockEdge::Left };
                            if let Ok(mut ab_guard) = appbar_tray.try_borrow_mut()
                                && let Some(ab) = ab_guard.as_mut() {
                                    ab.set_edge(edge);
                                }
                        }

                    if events.cycle_screen {
                        let mut cfg = config_poll.borrow_mut();
                        let mons = monitors_tray_list.borrow();
                        let count = mons.len();
                        if count > 0 {
                            cfg.screen_index = (cfg.screen_index + 1) % count;
                            let _ = cfg.save();
                            if let Some(ui) = ui_weak_timer.upgrade() {
                                sync_monitor_height(&ui, &mons, cfg.screen_index);
                            }
                            if let Ok(mut ab_guard) = appbar_tray.try_borrow_mut()
                                && let Some(ab) = ab_guard.as_mut()
                                    && cfg.screen_index < mons.len() {
                                        ab.set_monitor(mons[cfg.screen_index].clone());
                                    }
                        }
                    }

                    if events.toggle_appbar
                        && let Some(ui) = ui_weak_timer.upgrade() {
                            let mut cfg = config_poll.borrow_mut();
                            cfg.use_app_bar = !cfg.use_app_bar;
                            ui.set_is_appbar(cfg.use_app_bar);
                            let _ = cfg.save();
                            if let Ok(mut ab_guard) = appbar_tray.try_borrow_mut()
                                && let Some(ab) = ab_guard.as_mut() {
                                    ab.set_appbar_mode(cfg.use_app_bar);
                                }
                        }

                    if events.reload_sensors {
                        log_msg("Recarregar sensores solicitado via menu da bandeja.");
                        let _ = ctrl_tx.send(TelemetryControl::ReloadSensors);
                    }

                    if events.update_pos
                        && let Ok(ab_guard) = appbar_tray.try_borrow()
                            && let Some(ab) = ab_guard.as_ref() {
                                ab.update_position();
                            }

                    if events.show_window
                        && let Some(ui) = ui_weak_timer.upgrade()
                            && let Ok(mut ab_guard) = appbar_tray.try_borrow_mut() {
                                appbar_show(&ui, &mut ab_guard, &is_vis_clone);
                            }

                    if events.exit {
                        log_msg("Saída solicitada pelo menu da bandeja.");
                        let _ = slint::quit_event_loop();
                    }
                }

            #[cfg(target_os = "windows")]
            if pawnio_dismiss_poll.swap(false, std::sync::atomic::Ordering::SeqCst)
                && let Ok(mut cfg) = config_poll.try_borrow_mut()
                && !cfg.pawnio_prompt_dismissed
            {
                cfg.pawnio_prompt_dismissed = true;
                let _ = cfg.save();
            }

            // Atualiza telemetria
            if let Some(ui) = ui_weak_timer.upgrade() {
                let now = Local::now();
                let cfg = config_poll.borrow();
                let time_fmt = if cfg.clock_24hr { "%H:%M:%S" } else { "%I:%M:%S %p" };
                ui.set_time_str(now.format(time_fmt).to_string().into());

                let date_str = match cfg.date_setting.as_str() {
                    "Disabled" => String::new(),
                    "Short" => now.format("%d/%m").to_string(),
                    "Long" => now.format("%d de %B de %Y").to_string(),
                    _ => now.format("%A, %d %b").to_string(),
                };
                ui.set_date_str(date_str.into());

                if let Ok(rx_guard) = rx_ref.try_borrow() {
                    let mut latest_snap = None;
                    while let Ok(snap) = rx_guard.try_recv() {
                        latest_snap = Some(snap);
                    }
                    if let Some(snap) = latest_snap {
                        *last_cpu_temp_timer.borrow_mut() = snap.cpu.temperature_c;
                        apply_snapshot(&ui, snap, &cfg, &detected_poll, &models);
                    }
                }
            }
        },
    );

    // Timer de inicializaÃ§Ã£o pÃ³s-mapeamento (150ms)
    let init_timer = slint::Timer::default();
    let ui_weak_init = ui.as_weak();
    let is_vis_init = is_visible.clone();
    let tray_init = tray_holder.clone();
    let hwnd_init = hwnd_holder.clone();
    let monitors_init = monitors.clone();
    init_timer.start(
        slint::TimerMode::SingleShot,
        Duration::from_millis(150),
        move || {
            log_msg("5. Timer pÃ³s-mapeamento: aplicando estilos de janela, DWM, Tray e AppBar...");
            #[cfg(target_os = "windows")]
            {
                let _ = &monitors_init;
                if let Some(hwnd) = find_main_hwnd() {
                    let hwnd_isize = hwnd.0 as isize;
                    *hwnd_init.borrow_mut() = hwnd_isize;
                    log_msg(&format!("Janela HWND encontrada: 0x{:X}", hwnd_isize));

                        // 1. DWM Mica / Acrylic
                        dwm::win32::apply_fluent_effects(hwnd_isize);
                        log_msg("Efeitos DWM aplicados.");

                        // 2. Tray Icon
                        let cfg = config_ref.borrow();
                        if cfg.show_tray_icon {
                            let mut tray = TrayIcon::new(hwnd_isize);
                            if tray.init() {
                                log_msg("Ãcone na bandeja do sistema inicializado com sucesso.");
                                *tray_init.borrow_mut() = Some(tray);
                            } else {
                                log_msg("Aviso: Falha ao inicializar ícone na bandeja.");
                            }
                        }

                        // 3. AppBar com estilos que ocultam da barra de tarefas (WS_EX_TOOLWINDOW)
                        let edge = if cfg.is_dock_right() {
                            DockEdge::Right
                        } else {
                            DockEdge::Left
                        };

                        let mut ab = AppBarManager::new(
                            hwnd_isize,
                            edge,
                            cfg.sidebar_width as i32,
                            target_monitor.clone(),
                            cfg.always_top,
                            cfg.click_through,
                        );
                        ab.attach_window(ui_weak_init.clone());

                        if cfg.use_app_bar && !cfg.initially_hidden {
                            log_msg("Registrando Windows AppBar...");
                            if ab.register() {
                                log_msg("AppBar registrado com sucesso!");
                            } else {
                                log_msg("Aviso: Falha ao registrar AppBar.");
                            }
                        } else if !cfg.initially_hidden {
                            ab.update_position();
                        }

                        if cfg.initially_hidden {
                            log_msg("Modo InitiallyHidden ativo: liberando desktop e recolhendo para a bandeja.");
                            ab.hide_window();
                            *is_vis_init.borrow_mut() = false;
                        }
                        *appbar_ref.borrow_mut() = Some(ab);
                } else {
                    log_msg("Aviso: HWND da janela ZenithBar não encontrado no timer pós-mapeamento.");
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                log_msg("5. [Linux] Inicializando Tray, AppBar e ancoragem de tela...");
                let _ = &hwnd_init;
                let ui_opt = ui_weak_init.upgrade();
                let detected_mons = list_monitors_for_window(ui_opt.as_ref().map(|u| u.window()));
                if !detected_mons.is_empty() {
                    *monitors_init.borrow_mut() = detected_mons;
                    log_msg(&format!(
                        "[Linux] Monitores atualizados pós-mapeamento: {}",
                        monitors_init.borrow().len()
                    ));
                }

                let cfg = config_ref.borrow();
                let active_monitor = {
                    let mons = monitors_init.borrow();
                    if cfg.screen_index < mons.len() {
                        mons[cfg.screen_index].clone()
                    } else {
                        mons.first().cloned().unwrap_or_else(|| target_monitor.clone())
                    }
                };
                if let Some(ui) = ui_opt.as_ref() {
                    sync_monitor_height(ui, &monitors_init.borrow(), cfg.screen_index);
                }

                if cfg.show_tray_icon {
                    let mut tray = TrayIcon::new(0);
                    if tray.init() {
                        log_msg("Ícone na bandeja do sistema inicializado com sucesso.");
                        *tray_init.borrow_mut() = Some(tray);
                    }
                }

                let edge = if cfg.is_dock_right() {
                    DockEdge::Right
                } else {
                    DockEdge::Left
                };

                let mut ab = AppBarManager::new(
                    0,
                    edge,
                    cfg.sidebar_width as i32,
                    active_monitor,
                    cfg.always_top,
                    cfg.click_through,
                );
                ab.attach_window(ui_weak_init.clone());

                if cfg.use_app_bar && !cfg.initially_hidden {
                    let _ = ab.register();
                } else if !cfg.initially_hidden {
                    ab.update_position();
                }

                if cfg.initially_hidden {
                    log_msg("Modo InitiallyHidden ativo no Linux.");
                    ab.hide_window();
                    *is_vis_init.borrow_mut() = false;
                }
                *appbar_ref.borrow_mut() = Some(ab);
            }
        },
    );

    // Prompt único no Windows se temperatura da CPU seguir indisponível após 3.5s
    #[cfg(target_os = "windows")]
    let _pawnio_prompt_timer = {
        let t = slint::Timer::default();
        let config_prompt = config.clone();
        let temp_prompt = last_cpu_temp.clone();
        let dismiss_pending = pawnio_dismiss_pending.clone();
        t.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(3500),
            move || {
                let (enable_cpu, dismissed, culture) = {
                    let cfg = config_prompt.borrow();
                    (cfg.enable_cpu, cfg.pawnio_prompt_dismissed, cfg.culture.clone())
                };
                if !enable_cpu
                    || dismissed
                    || temp_prompt.borrow().is_some()
                    || telemetry::pawnio_win::is_sensor_service_running()
                {
                    return;
                }

                let dismiss_pending_bg = dismiss_pending.clone();
                std::thread::spawn(move || {
                    let s = i18n::load_locale(&culture);
                    let accepted = unsafe {
                        use windows::core::HSTRING;
                        use windows::Win32::UI::WindowsAndMessaging::{
                            MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO,
                        };
                        let title = HSTRING::from(&s.pawnio_prompt_title);
                        let body = HSTRING::from(&s.pawnio_prompt_msg);
                        MessageBoxW(None, &body, &title, MB_YESNO | MB_ICONQUESTION) == IDYES
                    };

                    if accepted {
                        if let Err(e) = telemetry::pawnio_win::launch_elevated_pawnio_setup() {
                            log_msg(&format!("[PawnIO] Setup elevado recusado/falhou: {}", e));
                            dismiss_pending_bg.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    } else {
                        log_msg("[PawnIO] Usuário optou por não instalar o driver PawnIO no momento.");
                        dismiss_pending_bg.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                });
            },
        );
        t
    };

    // Verificação de atualização em segundo plano (se habilitado)
    if config.borrow().auto_update {
        let culture_auto = config.borrow().culture.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            log_msg("[Auto-Update] Verificando novas versões no GitHub em segundo plano...");
            if let Ok(Some(release)) = updater::check_github_release() {
                updater::prompt_and_install_update(&release, &culture_auto);
            }
        });
    }

    if !config.borrow().initially_hidden {
        log_msg("6. Exibindo janela inicial e iniciando loop de eventos...");
        ui.show()?;
    } else {
        log_msg("6. Modo Inicialmente Oculto ativo: minimizado diretamente na bandeja.");
    }

    slint::run_event_loop()?;
    log_msg("Loop de eventos finalizado normalmente.");
    Ok(())
}

#[cfg(target_os = "windows")]
fn find_main_hwnd() -> Option<windows::Win32::Foundation::HWND> {
    use windows::core::{w, BOOL};
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowW, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe {
        if let Ok(hwnd) = FindWindowW(None, w!("ZenithBar")) {
            if !hwnd.0.is_null() {
                return Some(hwnd);
            }
        }

        struct SearchData {
            pid: u32,
            found: Option<HWND>,
        }
        let mut data = SearchData {
            pid: GetCurrentProcessId(),
            found: None,
        };

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            unsafe {
                let data = &mut *(lparam.0 as *mut SearchData);
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid == data.pid {
                    let mut buf = [0u16; 64];
                    let len = GetWindowTextW(hwnd, &mut buf);
                    if len > 0 {
                        let title = String::from_utf16_lossy(&buf[..len as usize]);
                        if title == "ZenithBar" {
                            data.found = Some(hwnd);
                            return BOOL(0);
                        }
                    }
                }
                BOOL(1)
            }
        }

        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut SearchData as isize));
        data.found
    }
}

#[derive(Default)]
struct DetectedNames {
    hostname: String,
    cpu: String,
    gpu: String,
    net: String,
}

fn resolve_display_name(custom: &str, detected: &str) -> String {
    if custom.trim().is_empty() {
        detected.to_string()
    } else {
        custom.trim().to_string()
    }
}

fn format_temp_c(temp: Option<f32>) -> (f32, String) {
    match temp {
        Some(t) => (t, format!("{:.0}°C", t)),
        None => (0.0, "--°C".to_string()),
    }
}

struct PersistentModels {
    cpu_history: Rc<VecModel<f32>>,
    cpu_core_loads: Rc<VecModel<f32>>,
    ram_history: Rc<VecModel<f32>>,
    gpu_history: Rc<VecModel<f32>>,
    disks_list: Rc<VecModel<DiskInfo>>,
    net_history: Rc<VecModel<f32>>,
}

impl PersistentModels {
    fn attach(ui: &AppWindow) -> Self {
        let models = Self {
            cpu_history: Rc::new(VecModel::default()),
            cpu_core_loads: Rc::new(VecModel::default()),
            ram_history: Rc::new(VecModel::default()),
            gpu_history: Rc::new(VecModel::default()),
            disks_list: Rc::new(VecModel::default()),
            net_history: Rc::new(VecModel::default()),
        };
        ui.set_cpu_history(ModelRc::from(models.cpu_history.clone()));
        ui.set_cpu_core_loads(ModelRc::from(models.cpu_core_loads.clone()));
        ui.set_ram_history(ModelRc::from(models.ram_history.clone()));
        ui.set_gpu_history(ModelRc::from(models.gpu_history.clone()));
        ui.set_disks_list(ModelRc::from(models.disks_list.clone()));
        ui.set_net_history(ModelRc::from(models.net_history.clone()));
        models
    }
}

fn sync_vec_model<T: PartialEq + Clone + 'static>(model: &VecModel<T>, next: Vec<T>) {
    let target_len = next.len();
    let common = model.row_count().min(target_len);
    let mut iter = next.into_iter();
    for (idx, item) in iter.by_ref().take(common).enumerate() {
        if model.row_data(idx).as_ref() != Some(&item) {
            model.set_row_data(idx, item);
        }
    }
    for item in iter {
        model.push(item);
    }
    while model.row_count() > target_len {
        if let Some(last) = model.row_count().checked_sub(1) {
            model.remove(last);
        }
    }
}

fn apply_snapshot(
    ui: &AppWindow,
    snap: TelemetrySnapshot,
    cfg: &AppConfig,
    detected_names: &Rc<RefCell<DetectedNames>>,
    models: &PersistentModels,
) {
    {
        let mut det = detected_names.borrow_mut();
        det.hostname = snap.hostname.clone();
        det.cpu = snap.cpu.name.clone();
        det.gpu = snap.gpu.name.clone();
        det.net = snap.net.adapter_name.clone();
    }

    ui.set_hostname(resolve_display_name(&cfg.custom_hostname, &snap.hostname).into());
    ui.set_uptime_str(snap.uptime.into());

    // CPU
    ui.set_cpu_name(resolve_display_name(&cfg.custom_cpu_name, &snap.cpu.name).into());
    ui.set_cpu_usage(snap.cpu.total_usage);
    ui.set_cpu_usage_str(format!("{:.0}%", snap.cpu.total_usage * 100.0).into());
    ui.set_cpu_clock_str(format!("{:.1} GHz", snap.cpu.clock_ghz).into());
    let (cpu_temp_val, cpu_temp_str) = format_temp_c(snap.cpu.temperature_c);
    ui.set_cpu_temp_val(cpu_temp_val);
    ui.set_cpu_temp_str(cpu_temp_str.into());
    sync_vec_model(&models.cpu_history, snap.cpu.history);
    sync_vec_model(&models.cpu_core_loads, snap.cpu.core_usages);

    // RAM
    ui.set_ram_percent(snap.mem.percent);
    ui.set_ram_percent_str(format!("{:.0}%", snap.mem.percent * 100.0).into());
    ui.set_ram_usage_str(format!("{:.1} / {:.1} GB", snap.mem.used_gb, snap.mem.total_gb).into());
    ui.set_ram_available_str(format!("{:.1} GB {}", snap.mem.available_gb, ui.get_i18n_available()).into());
    sync_vec_model(&models.ram_history, snap.mem.history);

    // GPU
    ui.set_gpu_detected(snap.gpu.detected);
    if snap.gpu.detected {
        ui.set_gpu_name(resolve_display_name(&cfg.custom_gpu_name, &snap.gpu.name).into());
        let gpu_usage_val = snap.gpu.usage.unwrap_or(0.0);
        ui.set_gpu_usage(gpu_usage_val);
        ui.set_gpu_usage_str(
            snap.gpu
                .usage
                .map(|u| format!("{:.0}%", u * 100.0))
                .unwrap_or_else(|| "--%".to_string())
                .into(),
        );
        ui.set_gpu_clock_str(
            snap.gpu
                .clock_ghz
                .map(|c| format!("{:.1} GHz", c))
                .unwrap_or_else(|| "-- GHz".to_string())
                .into(),
        );
        let (gpu_temp_val, gpu_temp_str) = format_temp_c(snap.gpu.temperature_c);
        ui.set_gpu_temp_val(gpu_temp_val);
        ui.set_gpu_temp_str(gpu_temp_str.into());
        ui.set_gpu_vram_str(format!("{:.1} / {:.1} GB", snap.gpu.vram_used_gb, snap.gpu.vram_total_gb).into());
        ui.set_gpu_vram_pct_str(
            snap.gpu
                .vram_percent
                .map(|p| format!("{:.0}%", p * 100.0))
                .unwrap_or_else(|| "--%".to_string())
                .into(),
        );
        ui.set_gpu_fan_pct_str(
            snap.gpu
                .fan_percent
                .map(|p| format!("{:.0}%", p * 100.0))
                .unwrap_or_else(|| "--%".to_string())
                .into(),
        );
        sync_vec_model(&models.gpu_history, snap.gpu.history);
    }

    // Disks
    let mut slint_disks = Vec::new();
    for (i, d) in snap.disks.iter().enumerate() {
        let clean_name = if d.name.trim().is_empty()
            || d.name.eq_ignore_ascii_case(&d.mount)
            || d.mount.starts_with(&d.name)
        {
            String::new()
        } else {
            d.name.clone()
        };

        if i == 0 {
            let primary_title = if clean_name.is_empty() {
                d.mount.clone()
            } else {
                format!("{} ({})", d.mount, clean_name)
            };
            ui.set_disk_primary_name(primary_title.into());
            ui.set_disk_primary_usage(format!("{:.0} / {:.0} GB ({:.0}%)", d.used_gb, d.total_gb, d.percent * 100.0).into());
            ui.set_disk_primary_percent(d.percent);
            ui.set_disk_read_str(format!("{:.1} MB/s", d.read_mb_s).into());
            ui.set_disk_write_str(format!("{:.1} MB/s", d.write_mb_s).into());
        }

        slint_disks.push(DiskInfo {
            name: clean_name.into(),
            mount: d.mount.clone().into(),
            used_gb: format!("{:.0} GB", d.used_gb).into(),
            total_gb: format!("{:.0} GB", d.total_gb).into(),
            percent: d.percent,
            read_speed: format!("{:.1} MB/s", d.read_mb_s).into(),
            write_speed: format!("{:.1} MB/s", d.write_mb_s).into(),
        });
    }
    sync_vec_model(&models.disks_list, slint_disks);

    // Network
    ui.set_net_adapter_name(resolve_display_name(&cfg.custom_net_name, &snap.net.adapter_name).into());
    ui.set_net_download_str(format!("{:.1} Mbps", snap.net.download_mbps).into());
    ui.set_net_upload_str(format!("{:.1} Mbps", snap.net.upload_mbps).into());
    ui.set_net_ip_str(snap.net.ip_address.into());
    ui.set_net_ext_ip_str(snap.net.external_ip.into());
    sync_vec_model(&models.net_history, snap.net.history);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_vec_model_reuses_model_across_grow_shrink_and_updates() {
        let model = VecModel::default();

        sync_vec_model(&model, vec![1.0, 2.0, 3.0]);
        assert_eq!(model.row_count(), 3);
        assert_eq!(model.row_data(1), Some(2.0));

        // Simulate hundreds of steady-state ticks
        for tick in 0..500 {
            let v = (tick % 10) as f32;
            sync_vec_model(&model, vec![v, v + 1.0, v + 2.0]);
            assert_eq!(model.row_count(), 3);
        }

        // Grow
        sync_vec_model(&model, vec![9.0, 8.0, 7.0, 6.0, 5.0]);
        assert_eq!(model.row_count(), 5);
        assert_eq!(model.row_data(4), Some(5.0));

        // Shrink
        sync_vec_model(&model, vec![4.0, 3.0]);
        assert_eq!(model.row_count(), 2);
        assert_eq!(model.row_data(0), Some(4.0));
        assert_eq!(model.row_data(1), Some(3.0));
    }

    #[test]
    fn rotate_log_file_caps_oversized_log() {
        let dir = std::env::temp_dir().join(format!("zenithbar_log_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let log_path = dir.join("zenithbar.log");
        let backup_path = dir.join("zenithbar.log.old");

        std::fs::write(&log_path, vec![b'x'; 2048]).unwrap();
        rotate_log_file(&log_path, 1024);

        assert!(!log_path.exists());
        assert!(backup_path.exists());
        assert_eq!(std::fs::metadata(&backup_path).unwrap().len(), 2048);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotate_log_file_keeps_small_or_missing_logs_intact() {
        let dir = std::env::temp_dir().join(format!("zenithbar_log_small_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let log_path = dir.join("zenithbar.log");
        let backup_path = dir.join("zenithbar.log.old");

        // Missing file is a no-op
        rotate_log_file(&log_path, 1024);
        assert!(!backup_path.exists());

        // File below threshold is preserved
        std::fs::write(&log_path, vec![b'a'; 512]).unwrap();
        rotate_log_file(&log_path, 1024);
        assert!(log_path.exists());
        assert!(!backup_path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_display_name_prefers_trimmed_custom_or_detected() {
        assert_eq!(resolve_display_name("", "Ryzen 7 5700G"), "Ryzen 7 5700G");
        assert_eq!(resolve_display_name("   ", "RX 6750 XT"), "RX 6750 XT");
        assert_eq!(resolve_display_name("  My CPU  ", "Ryzen 7 5700G"), "My CPU");
    }

    #[test]
    fn format_temp_c_formats_some_and_none() {
        assert_eq!(format_temp_c(None), (0.0, "--°C".to_string()));
        let (val, text) = format_temp_c(Some(64.6));
        assert!((val - 64.6).abs() < 0.001);
        assert_eq!(text, "65°C");
    }

    #[test]
    fn should_prefer_xwayland_detects_wayland_with_x11_fallback() {
        assert!(should_prefer_xwayland(Some("wayland-0"), Some("wayland"), Some(":0"), None));
        assert!(should_prefer_xwayland(None, Some("Wayland"), Some(":1"), Some("")));
        assert!(!should_prefer_xwayland(Some("wayland-0"), Some("wayland"), None, None));
        assert!(!should_prefer_xwayland(None, Some("x11"), Some(":0"), None));
        assert!(!should_prefer_xwayland(
            Some("wayland-0"),
            Some("wayland"),
            Some(":0"),
            Some("wayland")
        ));
    }
}
