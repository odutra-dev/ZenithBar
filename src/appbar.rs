use crate::monitor::MonitorDevice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockEdge {
    Left,
    Right,
}

pub fn compute_dock_geometry(
    edge: DockEdge,
    logical_width: i32,
    scale: f32,
    monitor: &MonitorDevice,
) -> (i32, i32, i32, i32) {
    let s = if scale > 0.0 && scale.is_finite() { scale } else { 1.0 };
    let physical_w = (logical_width.clamp(180, 360) as f32 * s).round() as i32;
    let x = match edge {
        DockEdge::Left => monitor.work_left,
        DockEdge::Right => monitor.work_right - physical_w,
    };
    let y = monitor.work_top;
    let h = monitor.work_height.max(1);
    (x, y, physical_w, h)
}

#[cfg(any(not(target_os = "windows"), test))]
pub fn compute_x11_strut_partial(
    edge: DockEdge,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    monitor: &MonitorDevice,
    total_screen_w: i32,
) -> Option<[u32; 12]> {
    let mut strut = [0u32; 12];
    let start_y = y.max(0) as u32;
    let end_y = (y + h - 1).max(0) as u32;

    match edge {
        DockEdge::Left => {
            if monitor.left > 0 {
                return None;
            }
            strut[0] = (x.max(0) + w).max(0) as u32;
            strut[4] = start_y;
            strut[5] = end_y;
        }
        DockEdge::Right => {
            let mon_right = monitor.left + monitor.width;
            if total_screen_w > 0 && mon_right < total_screen_w {
                return None;
            }
            strut[1] = (total_screen_w - x).max(w).max(0) as u32;
            strut[6] = start_y;
            strut[7] = end_y;
        }
    }
    Some(strut)
}

#[cfg(target_os = "windows")]
pub mod win32 {
    use super::DockEdge;
    use crate::monitor::MonitorDevice;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_EXCLUDED_FROM_PEEK};
    use windows::Win32::UI::Shell::{
        SHAppBarMessage, APPBARDATA, ABE_LEFT, ABE_RIGHT, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE,
        ABM_SETPOS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        GWL_EXSTYLE, GWL_STYLE, HWND_NOTOPMOST, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_SHOWWINDOW,
        SW_HIDE, SW_SHOW, WS_POPUP, WS_VISIBLE,
    };

    const WS_EX_TOOLWINDOW: isize = 0x00000080;
    const WS_EX_APPWINDOW: isize = 0x00040000;
    const WS_EX_TRANSPARENT: isize = 0x00000020;

    pub struct AppBarManager {
        hwnd: HWND,
        is_registered: bool,
        edge: DockEdge,
        width: i32,
        monitor: MonitorDevice,
        always_top: bool,
        click_through: bool,
    }

    impl AppBarManager {
        pub fn new(
            hwnd_raw: isize,
            edge: DockEdge,
            width: i32,
            monitor: MonitorDevice,
            always_top: bool,
            click_through: bool,
        ) -> Self {
            let mgr = Self {
                hwnd: HWND(hwnd_raw as *mut _),
                is_registered: false,
                edge,
                width,
                monitor,
                always_top,
                click_through,
            };
            mgr.apply_window_styles();
            mgr
        }

        pub fn apply_window_styles(&self) {
            if self.hwnd.0.is_null() {
                return;
            }

            unsafe {
                // 1. Remover borda/título da janela e garantir popup
                let current_style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
                let new_style = (current_style & !0x00CF0000) | (WS_POPUP.0 as isize) | (WS_VISIBLE.0 as isize);
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, new_style);

                // 2. Ocultar COMPLETAMENTE da Barra de Tarefas e Alt+Tab:
                //    Define WS_EX_TOOLWINDOW e remove WS_EX_APPWINDOW
                let current_exstyle = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
                let mut new_exstyle = (current_exstyle & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW;
                if self.click_through {
                    new_exstyle |= WS_EX_TRANSPARENT;
                } else {
                    new_exstyle &= !WS_EX_TRANSPARENT;
                }
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, new_exstyle);

                // 3. Excluir do Aero Peek
                let exclude: i32 = 1;
                let _ = DwmSetWindowAttribute(
                    self.hwnd,
                    DWMWA_EXCLUDED_FROM_PEEK,
                    &exclude as *const _ as *const _,
                    std::mem::size_of::<i32>() as u32,
                );

                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                        | SWP_FRAMECHANGED,
                );
            }
        }

        pub fn attach_window(&mut self, _window: slint::Weak<crate::AppWindow>) {}

        pub fn register(&mut self) -> bool {
            if self.hwnd.0.is_null() {
                return false;
            }

            self.apply_window_styles();

            let mut abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: self.hwnd,
                uCallbackMessage: windows::Win32::UI::WindowsAndMessaging::WM_USER + 102,
                uEdge: match self.edge {
                    DockEdge::Left => ABE_LEFT,
                    DockEdge::Right => ABE_RIGHT,
                },
                rc: RECT::default(),
                lParam: LPARAM(0),
            };

            unsafe {
                let res = SHAppBarMessage(ABM_NEW, &mut abd);
                self.is_registered = res != 0;
            }

            self.update_position();
            self.is_registered
        }

        pub fn set_edge(&mut self, edge: DockEdge) {
            let was_registered = self.is_registered;
            if was_registered {
                self.unregister();
            }
            self.edge = edge;
            if was_registered {
                self.register();
            } else {
                self.update_position();
            }
        }

        pub fn set_monitor(&mut self, monitor: MonitorDevice) {
            let was_registered = self.is_registered;
            if was_registered {
                self.unregister();
            }
            self.monitor = monitor;
            if was_registered {
                self.register();
            } else {
                self.update_position();
            }
        }

        pub fn set_width(&mut self, width: i32) {
            self.width = width.clamp(180, 360);
            self.update_position();
        }

        pub fn set_always_top(&mut self, always_top: bool) {
            self.always_top = always_top;
            self.update_position();
        }

        pub fn set_click_through(&mut self, click_through: bool) {
            self.click_through = click_through;
            self.apply_window_styles();
        }

        pub fn set_appbar_mode(&mut self, use_appbar: bool) {
            if use_appbar && !self.is_registered {
                self.register();
            } else if !use_appbar && self.is_registered {
                self.unregister();
                self.update_position();
            }
        }

        pub fn hide_window(&self) {
            if !self.hwnd.0.is_null() {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
            }
        }

        pub fn show_window(&self) {
            if !self.hwnd.0.is_null() {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_SHOW);
                    let _ = SetForegroundWindow(self.hwnd);
                }
            }
        }

        pub fn update_position(&self) {
            if self.hwnd.0.is_null() {
                return;
            }

            unsafe {
                let u_edge = match self.edge {
                    DockEdge::Left => ABE_LEFT,
                    DockEdge::Right => ABE_RIGHT,
                };

                let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(self.hwnd);
                let scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
                let (x, y, physical_w, h) =
                    super::compute_dock_geometry(self.edge, self.width, scale, &self.monitor);
                let w = physical_w;

                let rect = RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                };

                let mut abd = APPBARDATA {
                    cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                    hWnd: self.hwnd,
                    uCallbackMessage: 0,
                    uEdge: u_edge,
                    rc: rect,
                    lParam: LPARAM(0),
                };

                if self.is_registered {
                    SHAppBarMessage(ABM_QUERYPOS, &mut abd);
                    // The shell may move the requested edge to avoid another
                    // appbar. Use the negotiated rectangle for both reservation
                    // and actual window geometry.
                    match self.edge {
                        DockEdge::Left => abd.rc.right = abd.rc.left + physical_w,
                        DockEdge::Right => abd.rc.left = abd.rc.right - physical_w,
                    }
                    SHAppBarMessage(ABM_SETPOS, &mut abd);
                }

                let insert_after = if self.always_top {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                };

                let _ = SetWindowPos(
                    self.hwnd,
                    Some(insert_after),
                    abd.rc.left,
                    abd.rc.top,
                    abd.rc.right - abd.rc.left,
                    abd.rc.bottom - abd.rc.top,
                    SWP_SHOWWINDOW | SWP_FRAMECHANGED | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
                );

                use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow};
                let _ = InvalidateRect(Some(self.hwnd), None, true);
                let _ = UpdateWindow(self.hwnd);
            }
        }

        pub fn unregister(&mut self) {
            if self.is_registered && !self.hwnd.0.is_null() {
                let mut abd = APPBARDATA {
                    cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                    hWnd: self.hwnd,
                    uCallbackMessage: 0,
                    uEdge: match self.edge {
                        DockEdge::Left => ABE_LEFT,
                        DockEdge::Right => ABE_RIGHT,
                    },
                    rc: RECT::default(),
                    lParam: LPARAM(0),
                };
                unsafe {
                    SHAppBarMessage(ABM_REMOVE, &mut abd);
                }
                self.is_registered = false;
            }
        }
    }

    impl Drop for AppBarManager {
        fn drop(&mut self) {
            self.unregister();
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub mod win32 {
    use super::{compute_dock_geometry, compute_x11_strut_partial, DockEdge};
    use crate::monitor::MonitorDevice;
    use slint::winit_030::winit::dpi::{PhysicalPosition, PhysicalSize};
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::winit_030::winit::window::WindowLevel;
    use slint::winit_030::WinitWindowAccessor;
    use slint::ComponentHandle;

    pub struct AppBarManager {
        edge: DockEdge,
        width: i32,
        monitor: MonitorDevice,
        always_top: bool,
        click_through: bool,
        is_registered: bool,
        window_weak: Option<slint::Weak<crate::AppWindow>>,
    }

    impl AppBarManager {
        pub fn new(
            _hwnd_raw: isize,
            edge: DockEdge,
            width: i32,
            monitor: MonitorDevice,
            always_top: bool,
            click_through: bool,
        ) -> Self {
            let session_type = std::env::var("XDG_SESSION_TYPE")
                .unwrap_or_else(|_| "unknown".to_string());
            crate::log_msg(&format!(
                "[Linux Desktop] Initialized AppBar manager (Session: {}, Edge: {:?}, Width: {}px)",
                session_type, edge, width
            ));
            Self {
                edge,
                width: width.clamp(180, 360),
                monitor,
                always_top,
                click_through,
                is_registered: false,
                window_weak: None,
            }
        }

        pub fn attach_window(&mut self, window: slint::Weak<crate::AppWindow>) {
            self.window_weak = Some(window);
        }

        pub fn register(&mut self) -> bool {
            self.is_registered = true;
            self.update_position();
            crate::log_msg("[Linux Desktop] Registered screen reservation / docked edge.");
            true
        }

        pub fn set_edge(&mut self, edge: DockEdge) {
            self.edge = edge;
            self.update_position();
        }

        pub fn set_monitor(&mut self, monitor: MonitorDevice) {
            self.monitor = monitor;
            self.update_position();
        }

        pub fn set_width(&mut self, width: i32) {
            self.width = width.clamp(180, 360);
            self.update_position();
        }

        pub fn set_always_top(&mut self, always_top: bool) {
            self.always_top = always_top;
            self.update_position();
            crate::log_msg(&format!("[Linux Desktop] Always on top: {}", always_top));
        }

        pub fn set_click_through(&mut self, click_through: bool) {
            self.click_through = click_through;
            self.update_position();
        }

        pub fn set_appbar_mode(&mut self, use_appbar: bool) {
            if use_appbar {
                self.register();
            } else {
                self.unregister();
            }
        }

        pub fn hide_window(&self) {
            if let Some(weak) = self.window_weak.as_ref()
                && let Some(ui) = weak.upgrade()
            {
                if self.is_registered {
                    apply_x11_clear_struts(&ui);
                }
                let _ = ui.hide();
            }
        }

        pub fn show_window(&self) {
            if let Some(weak) = self.window_weak.as_ref()
                && let Some(ui) = weak.upgrade()
            {
                let _ = ui.show();
                self.update_position();
            }
        }

        pub fn update_position(&self) {
            let Some(weak) = self.window_weak.as_ref() else {
                return;
            };
            let Some(ui) = weak.upgrade() else {
                return;
            };

            let win = ui.window();
            let scale = win.scale_factor();
            let (x, y, phys_w, phys_h) =
                compute_dock_geometry(self.edge, self.width, scale, &self.monitor);

            let logical_h = if scale > 0.0 {
                phys_h as f32 / scale
            } else {
                phys_h as f32
            };
            ui.set_sidebar_width_px(self.width as f32);
            ui.set_sidebar_height_px(logical_h);

            win.set_position(slint::PhysicalPosition::new(x, y));

            win.with_winit_window(|winit_win| {
                winit_win.set_outer_position(PhysicalPosition::new(x, y));
                let _ = winit_win.request_inner_size(PhysicalSize::new(
                    phys_w.max(1) as u32,
                    phys_h.max(1) as u32,
                ));
                winit_win.set_window_level(if self.always_top {
                    WindowLevel::AlwaysOnTop
                } else {
                    WindowLevel::Normal
                });
                let _ = winit_win.set_cursor_hittest(!self.click_through);

                if let Ok(handle) = winit_win.window_handle()
                    && let Some(xwin) = extract_x11_window_id(handle.as_raw())
                {
                    apply_x11_dock_hints(
                        xwin,
                        self.edge,
                        x,
                        y,
                        phys_w,
                        phys_h,
                        &self.monitor,
                        self.always_top,
                        self.is_registered,
                    );
                }
            });

            crate::log_msg(&format!(
                "[Linux Desktop] Positioned ({}, {}), Size {}x{}, Edge: {:?}, Dock: {}, Monitor: {}",
                x, y, phys_w, phys_h, self.edge, self.is_registered, self.monitor.name
            ));
        }

        pub fn unregister(&mut self) {
            self.is_registered = false;
            if let Some(weak) = self.window_weak.as_ref()
                && let Some(ui) = weak.upgrade()
            {
                apply_x11_clear_struts(&ui);
            }
            self.update_position();
            crate::log_msg("[Linux Desktop] Unregistered screen reservation.");
        }
    }

    impl Drop for AppBarManager {
        fn drop(&mut self) {
            self.unregister();
        }
    }

    fn extract_x11_window_id(raw: RawWindowHandle) -> Option<u32> {
        match raw {
            RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok(),
            RawWindowHandle::Xcb(h) => Some(h.window.get()),
            _ => None,
        }
    }

    fn intern_atom(conn: &x11rb::rust_connection::RustConnection, name: &[u8]) -> Option<u32> {
        use x11rb::protocol::xproto::ConnectionExt;
        conn.intern_atom(false, name)
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
            .filter(|&a| a != 0)
    }

    fn apply_x11_clear_struts(ui: &crate::AppWindow) {
        ui.window().with_winit_window(|winit_win| {
            if let Ok(handle) = winit_win.window_handle()
                && let Some(xwin) = extract_x11_window_id(handle.as_raw())
                && let Ok((conn, _)) = x11rb::connect(None)
            {
                use x11rb::connection::Connection;
                use x11rb::protocol::xproto::ConnectionExt;
                for name in [&b"_NET_WM_STRUT_PARTIAL"[..], &b"_NET_WM_STRUT"[..]] {
                    if let Some(atom) = intern_atom(&conn, name) {
                        let _ = conn.delete_property(xwin, atom);
                    }
                }
                let _ = conn.flush();
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_x11_dock_hints(
        xwin: u32,
        edge: DockEdge,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        monitor: &MonitorDevice,
        always_top: bool,
        reserve_space: bool,
    ) {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt, PropMode};
        use x11rb::wrapper::ConnectionExt as _;

        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            return;
        };
        let total_screen_w = conn
            .setup()
            .roots
            .get(screen_num)
            .map(|s| s.width_in_pixels as i32)
            .unwrap_or(monitor.left + monitor.width);

        // Window Type: Dock when reserving space, Utility otherwise (keeps frameless panel clean)
        if let Some(wm_type) = intern_atom(&conn, b"_NET_WM_WINDOW_TYPE") {
            let target_type = if reserve_space {
                intern_atom(&conn, b"_NET_WM_WINDOW_TYPE_DOCK")
            } else {
                intern_atom(&conn, b"_NET_WM_WINDOW_TYPE_UTILITY")
            };
            if let Some(atom_val) = target_type {
                let _ = conn.change_property32(
                    PropMode::REPLACE,
                    xwin,
                    wm_type,
                    AtomEnum::ATOM,
                    &[atom_val],
                );
            }
        }

        // Window State: Skip Taskbar/Pager, Sticky, and Above when always_top
        if let Some(wm_state) = intern_atom(&conn, b"_NET_WM_STATE") {
            let mut states = Vec::new();
            if let Some(a) = intern_atom(&conn, b"_NET_WM_STATE_SKIP_TASKBAR") {
                states.push(a);
            }
            if let Some(a) = intern_atom(&conn, b"_NET_WM_STATE_SKIP_PAGER") {
                states.push(a);
            }
            if let Some(a) = intern_atom(&conn, b"_NET_WM_STATE_STICKY") {
                states.push(a);
            }
            if always_top
                && let Some(a) = intern_atom(&conn, b"_NET_WM_STATE_ABOVE")
            {
                states.push(a);
            }
            if !states.is_empty() {
                let _ = conn.change_property32(
                    PropMode::REPLACE,
                    xwin,
                    wm_state,
                    AtomEnum::ATOM,
                    &states,
                );
            }
        }

        // Struts (reserve desktop edge on X11)
        let strut_partial_atom = intern_atom(&conn, b"_NET_WM_STRUT_PARTIAL");
        let strut_atom = intern_atom(&conn, b"_NET_WM_STRUT");
        if reserve_space {
            if let Some(strut) =
                compute_x11_strut_partial(edge, x, y, w, h, monitor, total_screen_w)
            {
                if let Some(atom) = strut_partial_atom {
                    let _ = conn.change_property32(
                        PropMode::REPLACE,
                        xwin,
                        atom,
                        AtomEnum::CARDINAL,
                        &strut,
                    );
                }
                if let Some(atom) = strut_atom {
                    let _ = conn.change_property32(
                        PropMode::REPLACE,
                        xwin,
                        atom,
                        AtomEnum::CARDINAL,
                        &strut[..4],
                    );
                }
            }
        } else {
            if let Some(atom) = strut_partial_atom {
                let _ = conn.delete_property(xwin, atom);
            }
            if let Some(atom) = strut_atom {
                let _ = conn.delete_property(xwin, atom);
            }
        }

        let aux = ConfigureWindowAux::new()
            .x(x)
            .y(y)
            .width(w.max(1) as u32)
            .height(h.max(1) as u32);
        let _ = conn.configure_window(xwin, &aux);
        let _ = conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::MonitorDevice;

    fn sample_monitor(left: i32, width: i32, work_top: i32, work_height: i32) -> MonitorDevice {
        MonitorDevice {
            index: 0,
            name: "Test".to_string(),
            is_primary: true,
            left,
            top: 0,
            width,
            height: work_top + work_height,
            work_left: left,
            work_top,
            work_right: left + width,
            work_bottom: work_top + work_height,
            work_width: width,
            work_height,
        }
    }

    #[test]
    fn compute_dock_geometry_anchors_to_right_and_left_with_dpi_scaling() {
        let mon = sample_monitor(0, 2560, 32, 1360);

        let (rx, ry, rw, rh) = compute_dock_geometry(DockEdge::Right, 300, 1.0, &mon);
        assert_eq!((rx, ry, rw, rh), (2260, 32, 300, 1360));

        let (lx, ly, lw, lh) = compute_dock_geometry(DockEdge::Left, 300, 1.5, &mon);
        assert_eq!((lx, ly, lw, lh), (0, 32, 450, 1360));
    }

    #[test]
    fn compute_x11_strut_partial_reserves_outer_edges_and_skips_interior_monitors() {
        let primary = sample_monitor(0, 2560, 32, 1360);
        let strut_right =
            compute_x11_strut_partial(DockEdge::Right, 2260, 32, 300, 1360, &primary, 2560)
                .expect("right edge on outer screen boundary should emit strut");
        assert_eq!(strut_right[1], 300);
        assert_eq!(strut_right[6], 32);
        assert_eq!(strut_right[7], 1391);

        // Dual monitor setup (2560 + 1920 = 4480): right edge of left monitor is interior -> None
        let interior =
            compute_x11_strut_partial(DockEdge::Right, 2260, 32, 300, 1360, &primary, 4480);
        assert!(interior.is_none());
    }
}
