#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockEdge {
    Left,
    Right,
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
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE,
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_SHOWWINDOW, WS_POPUP, WS_VISIBLE,
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

        pub fn register(&mut self) -> bool {
            if self.hwnd.0.is_null() {
                return false;
            }

            self.apply_window_styles();

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
            self.width = width;
            if self.is_registered {
                self.unregister();
                self.register();
            } else {
                self.update_position();
            }
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

        pub fn update_position(&self) {
            if self.hwnd.0.is_null() {
                return;
            }

            unsafe {
                let u_edge = match self.edge {
                    DockEdge::Left => ABE_LEFT,
                    DockEdge::Right => ABE_RIGHT,
                };

                let x = match self.edge {
                    DockEdge::Left => self.monitor.work_left,
                    DockEdge::Right => self.monitor.work_right - self.width,
                };
                let y = self.monitor.work_top;
                let w = self.width;
                let h = self.monitor.work_height;

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
                    x,
                    y,
                    w,
                    h,
                    SWP_SHOWWINDOW | SWP_FRAMECHANGED,
                );
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
    use super::DockEdge;
    use crate::monitor::MonitorDevice;

    pub struct AppBarManager {
        edge: DockEdge,
        width: i32,
        monitor: MonitorDevice,
        always_top: bool,
        click_through: bool,
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
            Self { edge, width, monitor, always_top, click_through }
        }
        pub fn register(&mut self) -> bool { true }
        pub fn set_edge(&mut self, edge: DockEdge) { self.edge = edge; }
        pub fn set_monitor(&mut self, monitor: MonitorDevice) { self.monitor = monitor; }
        pub fn set_width(&mut self, width: i32) { self.width = width; }
        pub fn set_always_top(&mut self, always_top: bool) { self.always_top = always_top; }
        pub fn set_click_through(&mut self, click_through: bool) { self.click_through = click_through; }
        pub fn set_appbar_mode(&mut self, _use_appbar: bool) {}
        pub fn update_position(&self) {}
        pub fn unregister(&mut self) {}
    }
}
