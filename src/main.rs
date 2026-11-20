#![windows_subsystem = "windows"]

mod appbar;
mod config;
mod dwm;
mod i18n;
mod monitor;
mod telemetry;
mod tray;
mod updater;

#[allow(unused_imports)]
use appbar::{win32::AppBarManager, DockEdge};
use chrono::Local;
use config::AppConfig;
use monitor::win32::list_monitors;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use telemetry::{TelemetryEngine, TelemetrySnapshot};
use tray::win32::TrayIcon;

slint::include_modules!();

fn log_msg(msg: &str) {
    use std::io::Write;
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("zenithbar.log")
        .and_then(|mut f| writeln!(f, "[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg));
}

fn appbar_hide(ui: &AppWindow, ab: &mut Option<AppBarManager>, is_vis: &RefCell<bool>) {
    if let Some(ab_mgr) = ab.as_mut() {
        ab_mgr.unregister();
        ab_mgr.hide_window();
    } else {
        let _ = ui.hide();
    }
    #[cfg(not(target_os = "windows"))]
    let _ = ui.hide();
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
    #[cfg(not(target_os = "windows"))]
    let _ = ui.show();
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

fn apply_settings_to_app(
    ui: &AppWindow,
    sw: &SettingsWindow,
    cfg: &mut AppConfig,
    ab: &mut Option<AppBarManager>,
    monitors: &[monitor::MonitorDevice],
    interval_tx: &std::sync::mpsc::Sender<u64>,
) {
    let edge_changed = cfg.is_dock_right() != sw.get_dock_right();
    let monitor_changed = cfg.screen_index != (sw.get_monitor_idx().max(0) as usize);
    let width_changed = cfg.sidebar_width != (sw.get_sidebar_width().clamp(180, 360) as u32);
    let appbar_changed = cfg.use_app_bar != sw.get_use_app_bar();
    let always_top_changed = cfg.always_top != sw.get_always_top();
    let click_through_changed = cfg.click_through != sw.get_click_through();

    cfg.set_dock_right(sw.get_dock_right());
    cfg.screen_index = sw.get_monitor_idx().max(0) as usize;
    cfg.use_app_bar = sw.get_use_app_bar();
    cfg.always_top = sw.get_always_top();
    cfg.run_at_startup = sw.get_run_at_startup();
    cfg.initially_hidden = sw.get_initially_hidden();

    cfg.polling_interval = sw.get_polling_interval().clamp(100, 10000) as u64;
    let _ = interval_tx.send(cfg.polling_interval);
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

    let target_h = if cfg.screen_index < monitors.len() {
        monitors[cfg.screen_index].work_height
    } else {
        monitors.first().map(|m| m.work_height).unwrap_or(1080)
    };
    let scale = ui.window().scale_factor();
    let logical_h = if scale > 0.0 { target_h as f32 / scale } else { target_h as f32 };
    ui.set_sidebar_height_px(logical_h);
    ui.window().request_redraw();

    tray::win32::set_always_top_state(cfg.always_top);
    tray::win32::set_tray_culture(&cfg.culture);

    // Atualiza o AppBar nativo Win32
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
    // Setup panic hook with log file and native Windows Message Box
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("FATAL ERROR:\n{:?}\n", info);
        log_msg(&msg);
        let _ = std::fs::write("zenithbar_crash.log", &msg);

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
    }

    log_msg("4. Inicializando thread de telemetria isolada em background (MTA)...");
    let (tx, rx) = std::sync::mpsc::channel::<TelemetrySnapshot>();
    let (interval_tx, interval_rx) = std::sync::mpsc::channel::<u64>();
    let initial_polling_interval = config.borrow().polling_interval.clamp(100, 10000);
    std::thread::spawn(move || {
        log_msg("[Telemetry Thread] Iniciada com sucesso.");
        let mut engine = TelemetryEngine::new();
        let mut interval = initial_polling_interval;
        loop {
            let snap = engine.snapshot();
            if tx.send(snap).is_err() {
                break;
            }
            match interval_rx.recv_timeout(Duration::from_millis(interval)) {
                Ok(mut next) => {
                    while let Ok(newer) = interval_rx.try_recv() {
                        next = newer;
                    }
                    interval = next.clamp(100, 10000);
                    log_msg(&format!("[Telemetry Thread] Novo intervalo ativo: {} ms", interval));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {},
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    // Detectar monitores
    let monitors = list_monitors();
    log_msg(&format!("Monitores detectados: {}", monitors.len()));
    let target_monitor = {
        let cfg = config.borrow();
        if cfg.screen_index < monitors.len() {
            monitors[cfg.screen_index].clone()
        } else {
            monitors[0].clone()
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
        let physical_w = if scale > 0.0 { (cfg.sidebar_width as f32 * scale).round() as i32 } else { cfg.sidebar_width as i32 };
        let init_x = if cfg.is_dock_right() {
            target_monitor.work_right - physical_w
        } else {
            target_monitor.work_left
        };
        let init_y = target_monitor.work_top;
        ui.window().set_position(slint::PhysicalPosition::new(init_x, init_y));
        log_msg(&format!("Posição inicial ancorada pré-configurada: ({}, {})", init_x, init_y));
    }

    // Gerenciador do AppBar e Ãcone da Bandeja mantidos vivos
    let appbar_holder: Rc<RefCell<Option<AppBarManager>>> = Rc::new(RefCell::new(None));
    let tray_holder: Rc<RefCell<Option<TrayIcon>>> = Rc::new(RefCell::new(None));

    // Estado de visibilidade
    let is_visible = Rc::new(RefCell::new(!config.borrow().initially_hidden));

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
            if let Ok(mut ab_guard) = appbar_toggle.try_borrow_mut() {
                if let Some(ab) = ab_guard.as_mut() {
                    ab.set_edge(edge);
                }
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
            if let Ok(mut ab_guard) = appbar_mode.try_borrow_mut() {
                if let Some(ab) = ab_guard.as_mut() {
                    ab.set_appbar_mode(cfg.use_app_bar);
                }
            }
        }
    });

    let settings_weak = settings_ui.as_weak();
    let config_for_settings = config.clone();
    let monitors_len = monitors.len();
    ui.on_request_open_settings(move || {
        if let Some(sw) = settings_weak.upgrade() {
            let cfg = config_for_settings.borrow();
            populate_settings_ui(&sw, &cfg, monitors_len);
            let _ = sw.show();
        }
    });

    // O botÃ£o 'âœ•' no rodapÃ© oculta para a bandeja (AppBarHide) liberando o desktop
    let ui_weak_quit = ui.as_weak();
    let appbar_quit = appbar_holder.clone();
    let is_vis_quit = is_visible.clone();
    ui.on_request_quit(move || {
        if let Some(ui) = ui_weak_quit.upgrade() {
            if let Ok(mut ab_guard) = appbar_quit.try_borrow_mut() {
                appbar_hide(&ui, &mut ab_guard, &is_vis_quit);
            }
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
    let interval_apply = interval_tx.clone();
    let settings_weak_apply = settings_ui.as_weak();
    let config_apply = config.clone();
    let ui_weak_apply = ui.as_weak();
    let appbar_apply = appbar_holder.clone();
    let monitors_apply = monitors.clone();
    settings_ui.on_request_apply(move || {
        if let (Some(sw), Some(ui)) = (settings_weak_apply.upgrade(), ui_weak_apply.upgrade()) {
            let mut cfg = config_apply.borrow_mut();
            if let Ok(mut ab_guard) = appbar_apply.try_borrow_mut() {
                apply_settings_to_app(&ui, &sw, &mut cfg, &mut ab_guard, &monitors_apply, &interval_apply);
            }
        }
    });

    // SALVAR (aplica em tempo real e fecha a janela)
    let interval_save = interval_tx.clone();
    let settings_weak_save = settings_ui.as_weak();
    let config_save = config.clone();
    let ui_weak_save = ui.as_weak();
    let appbar_save = appbar_holder.clone();
    let monitors_save = monitors.clone();
    settings_ui.on_request_save(move || {
        if let (Some(sw), Some(ui)) = (settings_weak_save.upgrade(), ui_weak_save.upgrade()) {
            let mut cfg = config_save.borrow_mut();
            if let Ok(mut ab_guard) = appbar_save.try_borrow_mut() {
                apply_settings_to_app(&ui, &sw, &mut cfg, &mut ab_guard, &monitors_save, &interval_save);
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

    let appbar_ref = appbar_holder.clone();
    let config_ref = config.clone();
    let tray_ref = tray_holder.clone();

    // Timer de recebimento de telemetria periÃ³dico (250ms) e polling de eventos da bandeja
    let ui_weak_timer = ui.as_weak();
    let settings_weak_timer = settings_ui.as_weak();
    let rx_ref = Rc::new(RefCell::new(rx));
    let config_poll = config.clone();
    let is_vis_clone = is_visible.clone();
    let appbar_tray = appbar_holder.clone();
    let monitors_tray_count = monitors.len();
    let monitors_tray_list = monitors.clone();

    let telemetry_timer = slint::Timer::default();
    telemetry_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            // Poll eventos da bandeja (Tray)
            if let Ok(tray_guard) = tray_ref.try_borrow() {
                if let Some(tray) = tray_guard.as_ref() {
                    let events = tray.poll_events();

                    if events.toggle_vis {
                        if let Some(ui) = ui_weak_timer.upgrade() {
                            if let Ok(mut ab_guard) = appbar_tray.try_borrow_mut() {
                                if *is_vis_clone.borrow() {
                                    appbar_hide(&ui, &mut ab_guard, &is_vis_clone);
                                } else {
                                    appbar_show(&ui, &mut ab_guard, &is_vis_clone);
                                }
                            }
                        }
                    }

                    if events.open_settings {
                        if let Some(sw) = settings_weak_timer.upgrade() {
                            let cfg = config_poll.borrow();
                            populate_settings_ui(&sw, &cfg, monitors_tray_count);
                            let _ = sw.show();
                        }
                    }

                    if events.toggle_side {
                        if let Some(ui) = ui_weak_timer.upgrade() {
                            let mut cfg = config_poll.borrow_mut();
                            let new_right = !cfg.is_dock_right();
                            cfg.set_dock_right(new_right);
                            ui.set_is_docked_right(new_right);
                            let _ = cfg.save();
                            let edge = if new_right { DockEdge::Right } else { DockEdge::Left };
                            if let Ok(mut ab_guard) = appbar_tray.try_borrow_mut() {
                                if let Some(ab) = ab_guard.as_mut() {
                                    ab.set_edge(edge);
                                }
                            }
                        }
                    }

                    if events.cycle_screen {
                        let mut cfg = config_poll.borrow_mut();
                        if monitors_tray_count > 0 {
                            cfg.screen_index = (cfg.screen_index + 1) % monitors_tray_count;
                            let _ = cfg.save();
                            if let Ok(mut ab_guard) = appbar_tray.try_borrow_mut() {
                                if let Some(ab) = ab_guard.as_mut() {
                                    if cfg.screen_index < monitors_tray_list.len() {
                                        ab.set_monitor(monitors_tray_list[cfg.screen_index].clone());
                                    }
                                }
                            }
                        }
                    }

                    if events.reload_sensors {
                        log_msg("Recarregar sensores solicitado via menu da bandeja.");
                    }

                    if events.update_pos {
                        if let Ok(ab_guard) = appbar_tray.try_borrow() {
                            if let Some(ab) = ab_guard.as_ref() {
                                ab.update_position();
                            }
                        }
                    }
                }
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
                    while let Ok(snap) = rx_guard.try_recv() {
                        apply_snapshot(&ui, snap);
                    }
                }
            }
        },
    );

    // Timer de inicializaÃ§Ã£o pÃ³s-mapeamento (150ms)
    let init_timer = slint::Timer::default();
    #[allow(unused_variables)]
    let ui_weak_init = ui.as_weak();
    let is_vis_init = is_visible.clone();
    init_timer.start(
        slint::TimerMode::SingleShot,
        Duration::from_millis(150),
        move || {
            log_msg("5. Timer pÃ³s-mapeamento: aplicando estilos de janela, DWM, Tray e AppBar...");
            #[cfg(target_os = "windows")]
            {
                if let Some(hwnd) = find_main_hwnd() {
                    let hwnd_isize = hwnd.0 as isize;
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
                                *tray_holder.borrow_mut() = Some(tray);
                            } else {
                                log_msg("Aviso: Falha ao inicializar Ã­cone na bandeja.");
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
                let cfg = config_ref.borrow();
                if cfg.show_tray_icon {
                    let mut tray = TrayIcon::new(0);
                    if tray.init() {
                        log_msg("Ãcone na bandeja do sistema inicializado com sucesso.");
                        *tray_holder.borrow_mut() = Some(tray);
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
                    target_monitor.clone(),
                    cfg.always_top,
                    cfg.click_through,
                );

                if cfg.use_app_bar && !cfg.initially_hidden {
                    let _ = ab.register();
                } else if !cfg.initially_hidden {
                    ab.update_position();
                }
                *appbar_ref.borrow_mut() = Some(ab);

                if cfg.initially_hidden {
                    log_msg("Modo InitiallyHidden ativo no Linux.");
                    if let Some(ui) = ui_weak_init.upgrade() {
                        let _ = ui.hide();
                    }
                    *is_vis_init.borrow_mut() = false;
                }
            }
        },
    );

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

fn format_temp_c(temp: Option<f32>) -> (f32, String) {
    match temp {
        Some(t) => (t, format!("{:.0}°C", t)),
        None => (0.0, "--°C".to_string()),
    }
}

fn apply_snapshot(ui: &AppWindow, snap: TelemetrySnapshot) {
    ui.set_hostname(snap.hostname.into());
    ui.set_uptime_str(snap.uptime.into());

    // CPU
    ui.set_cpu_name(snap.cpu.name.into());
    ui.set_cpu_usage(snap.cpu.total_usage);
    ui.set_cpu_usage_str(format!("{:.0}%", snap.cpu.total_usage * 100.0).into());
    ui.set_cpu_clock_str(format!("{:.1} GHz", snap.cpu.clock_ghz).into());
    let (cpu_temp_val, cpu_temp_str) = format_temp_c(snap.cpu.temperature_c);
    ui.set_cpu_temp_val(cpu_temp_val);
    ui.set_cpu_temp_str(cpu_temp_str.into());
    let cpu_hist = Rc::new(VecModel::from(snap.cpu.history));
    ui.set_cpu_history(ModelRc::from(cpu_hist));

    let core_loads = Rc::new(VecModel::from(snap.cpu.core_usages));
    ui.set_cpu_core_loads(ModelRc::from(core_loads));

    // RAM
    ui.set_ram_percent(snap.mem.percent);
    ui.set_ram_percent_str(format!("{:.0}%", snap.mem.percent * 100.0).into());
    ui.set_ram_usage_str(format!("{:.1} / {:.1} GB", snap.mem.used_gb, snap.mem.total_gb).into());
    ui.set_ram_available_str(format!("{:.1} GB Disp.", snap.mem.available_gb).into());
    let ram_hist = Rc::new(VecModel::from(snap.mem.history));
    ui.set_ram_history(ModelRc::from(ram_hist));

    // GPU
    ui.set_gpu_detected(snap.gpu.detected);
    if snap.gpu.detected {
        ui.set_gpu_name(snap.gpu.name.into());
        ui.set_gpu_usage(snap.gpu.usage);
        ui.set_gpu_usage_str(format!("{:.0}%", snap.gpu.usage * 100.0).into());
        let (gpu_temp_val, gpu_temp_str) = format_temp_c(snap.gpu.temperature_c);
        ui.set_gpu_temp_val(gpu_temp_val);
        ui.set_gpu_temp_str(gpu_temp_str.into());
        ui.set_gpu_vram_str(format!("{:.1} / {:.1} GB", snap.gpu.vram_used_gb, snap.gpu.vram_total_gb).into());
        if let Some(fan) = snap.gpu.fan_rpm {
            ui.set_gpu_fan_str(format!("{} RPM", fan).into());
        } else {
            ui.set_gpu_fan_str(String::new().into());
        }
        let gpu_hist = Rc::new(VecModel::from(snap.gpu.history));
        ui.set_gpu_history(ModelRc::from(gpu_hist));
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
    let disks_model = Rc::new(VecModel::from(slint_disks));
    ui.set_disks_list(ModelRc::from(disks_model));

    // Network
    ui.set_net_adapter_name(snap.net.adapter_name.into());
    ui.set_net_download_str(format!("{:.1} Mbps", snap.net.download_mbps).into());
    ui.set_net_upload_str(format!("{:.1} Mbps", snap.net.upload_mbps).into());
    ui.set_net_ip_str(snap.net.ip_address.into());
    ui.set_net_ext_ip_str(snap.net.external_ip.into());
    let net_hist = Rc::new(VecModel::from(snap.net.history));
    ui.set_net_history(ModelRc::from(net_hist));
}
