#![windows_subsystem = "windows"]

mod appbar;
mod config;
mod dwm;
mod monitor;
mod telemetry;
mod tray;

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
    }
    let _ = ui.hide();
    *is_vis.borrow_mut() = false;
    log_msg("Barra ocultada para a bandeja (AppBar liberado).");
}

fn appbar_show(ui: &AppWindow, ab: &mut Option<AppBarManager>, is_vis: &RefCell<bool>) {
    let _ = ui.show();
    if let Some(ab_mgr) = ab.as_mut() {
        ab_mgr.register();
    }
    *is_vis.borrow_mut() = true;
    log_msg("Barra reexibida na tela (AppBar registrado).");
}

fn populate_settings_ui(sw: &SettingsWindow, cfg: &AppConfig, monitor_count: usize) {
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
) {
    let edge_changed = cfg.is_dock_right() != sw.get_dock_right();
    let monitor_changed = cfg.screen_index != (sw.get_monitor_idx().max(0) as usize);
    let width_changed = cfg.sidebar_width != (sw.get_sidebar_width().max(200) as u32);
    let appbar_changed = cfg.use_app_bar != sw.get_use_app_bar();
    let always_top_changed = cfg.always_top != sw.get_always_top();
    let click_through_changed = cfg.click_through != sw.get_click_through();

    cfg.set_dock_right(sw.get_dock_right());
    cfg.screen_index = sw.get_monitor_idx().max(0) as usize;
    cfg.use_app_bar = sw.get_use_app_bar();
    cfg.always_top = sw.get_always_top();
    cfg.run_at_startup = sw.get_run_at_startup();
    cfg.initially_hidden = sw.get_initially_hidden();

    cfg.polling_interval = sw.get_polling_interval().max(100) as u64;
    cfg.ui_scale = sw.get_ui_scale();
    cfg.show_tray_icon = sw.get_show_tray_icon();
    cfg.click_through = sw.get_click_through();
    cfg.collapse_menu_bar = sw.get_collapse_menu_bar();
    cfg.auto_update = sw.get_auto_update();

    cfg.sidebar_width = sw.get_sidebar_width().max(200) as u32;
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

    log_msg("1. Carregando configurações 1:1...");
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
                let title = HSTRING::from("ZenithBar - Erro de Renderização");
                let body = HSTRING::from(format!(
                    "Falha ao inicializar a interface gráfica Slint:\n{}\n\nVerifique se os drivers de vídeo estão atualizados.",
                    err_msg
                ));
                let _ = MessageBoxW(None, &body, &title, MB_OK | MB_ICONERROR);
            }
            return Err(Box::new(e));
        }
    };

    log_msg("3. Criando janela separada de Configurações de 5 Abas 1:1...");
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
    }

    log_msg("4. Inicializando thread de telemetria isolada em background (MTA)...");
    let (tx, rx) = std::sync::mpsc::channel::<TelemetrySnapshot>();
    std::thread::spawn(move || {
        log_msg("[Telemetry Thread] Iniciada com sucesso.");
        let mut engine = TelemetryEngine::new();
        loop {
            let snap = engine.snapshot();
            if tx.send(snap).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1000));
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

    // Gerenciador do AppBar e Ícone da Bandeja mantidos vivos
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

    // O botão '✕' no rodapé oculta para a bandeja (AppBarHide) liberando o desktop
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

    // Callbacks da Janela de Configurações
    let settings_weak_close = settings_ui.as_weak();
    settings_ui.on_request_close(move || {
        if let Some(sw) = settings_weak_close.upgrade() {
            let _ = sw.hide();
        }
    });

    // APLICAR (em tempo real)
    let settings_weak_apply = settings_ui.as_weak();
    let config_apply = config.clone();
    let ui_weak_apply = ui.as_weak();
    let appbar_apply = appbar_holder.clone();
    let monitors_apply = monitors.clone();
    settings_ui.on_request_apply(move || {
        if let (Some(sw), Some(ui)) = (settings_weak_apply.upgrade(), ui_weak_apply.upgrade()) {
            let mut cfg = config_apply.borrow_mut();
            if let Ok(mut ab_guard) = appbar_apply.try_borrow_mut() {
                apply_settings_to_app(&ui, &sw, &mut cfg, &mut ab_guard, &monitors_apply);
            }
        }
    });

    // SALVAR (aplica em tempo real e fecha a janela)
    let settings_weak_save = settings_ui.as_weak();
    let config_save = config.clone();
    let ui_weak_save = ui.as_weak();
    let appbar_save = appbar_holder.clone();
    let monitors_save = monitors.clone();
    settings_ui.on_request_save(move || {
        if let (Some(sw), Some(ui)) = (settings_weak_save.upgrade(), ui_weak_save.upgrade()) {
            let mut cfg = config_save.borrow_mut();
            if let Ok(mut ab_guard) = appbar_save.try_borrow_mut() {
                apply_settings_to_app(&ui, &sw, &mut cfg, &mut ab_guard, &monitors_save);
            }
            let _ = sw.hide();
        }
    });

    let appbar_ref = appbar_holder.clone();
    let config_ref = config.clone();
    let tray_ref = tray_holder.clone();

    // Timer de recebimento de telemetria periódico (250ms) e polling de eventos da bandeja
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
        Duration::from_millis(250),
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

    // Timer de inicialização pós-mapeamento (150ms)
    let init_timer = slint::Timer::default();
    let ui_weak_init = ui.as_weak();
    let is_vis_init = is_visible.clone();
    init_timer.start(
        slint::TimerMode::SingleShot,
        Duration::from_millis(150),
        move || {
            log_msg("5. Timer pós-mapeamento: aplicando estilos de janela, DWM, Tray e AppBar...");
            #[cfg(target_os = "windows")]
            unsafe {
                use windows::core::w;
                use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

                if let Ok(hwnd) = FindWindowW(None, w!("ZenithBar")) {
                    if !hwnd.0.is_null() {
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
                                log_msg("Ícone na bandeja do sistema inicializado com sucesso.");
                                *tray_holder.borrow_mut() = Some(tray);
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
                        *appbar_ref.borrow_mut() = Some(ab);

                        if cfg.initially_hidden {
                            log_msg("Modo InitiallyHidden ativo: liberando desktop e recolhendo para a bandeja.");
                            if let Some(ui) = ui_weak_init.upgrade() {
                                let _ = ui.hide();
                            }
                            *is_vis_init.borrow_mut() = false;
                        }
                    } else {
                        log_msg("Aviso: HWND nulo retornado por FindWindowW.");
                    }
                } else {
                    log_msg("Aviso: FindWindowW não encontrou a janela ZenithBar.");
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                log_msg("5. [Linux] Inicializando Tray, AppBar e ancoragem de tela...");
                let cfg = config_ref.borrow();
                if cfg.show_tray_icon {
                    let mut tray = TrayIcon::new(0);
                    if tray.init() {
                        log_msg("Ícone na bandeja do sistema inicializado com sucesso.");
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

    log_msg("6. Exibindo janela inicial e iniciando loop de eventos...");
    ui.show()?;

    slint::run_event_loop()?;
    log_msg("Loop de eventos finalizado normalmente.");
    Ok(())
}

fn apply_snapshot(ui: &AppWindow, snap: TelemetrySnapshot) {
    ui.set_hostname(snap.hostname.into());
    ui.set_uptime_str(snap.uptime.into());

    // CPU
    ui.set_cpu_name(snap.cpu.name.into());
    ui.set_cpu_usage(snap.cpu.total_usage);
    ui.set_cpu_usage_str(format!("{:.0}%", snap.cpu.total_usage * 100.0).into());
    ui.set_cpu_clock_str(format!("{:.2} GHz", snap.cpu.clock_ghz).into());
    if let Some(temp) = snap.cpu.temperature_c {
        ui.set_cpu_temp_val(temp);
        ui.set_cpu_temp_str(format!("{:.0} °C", temp).into());
    } else {
        ui.set_cpu_temp_val(0.0);
        ui.set_cpu_temp_str("-- °C".into());
    }
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
        if let Some(temp) = snap.gpu.temperature_c {
            ui.set_gpu_temp_val(temp);
            ui.set_gpu_temp_str(format!("{:.0} °C", temp).into());
        } else {
            ui.set_gpu_temp_val(0.0);
            ui.set_gpu_temp_str("-- °C".into());
        }
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
        if i == 0 {
            ui.set_disk_primary_name(format!("{} ({})", d.mount, d.name).into());
            ui.set_disk_primary_usage(format!("{:.0} / {:.0} GB ({:.0}%)", d.used_gb, d.total_gb, d.percent * 100.0).into());
            ui.set_disk_primary_percent(d.percent);
            ui.set_disk_read_str(format!("{:.1} MB/s", d.read_mb_s).into());
            ui.set_disk_write_str(format!("{:.1} MB/s", d.write_mb_s).into());
        }

        slint_disks.push(DiskInfo {
            name: d.name.clone().into(),
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
