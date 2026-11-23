#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct MonitorDevice {
    pub index: usize,
    pub name: String,
    pub is_primary: bool,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub work_left: i32,
    pub work_top: i32,
    pub work_right: i32,
    pub work_bottom: i32,
    pub work_width: i32,
    pub work_height: i32,
}

#[cfg(target_os = "windows")]
pub mod win32 {
    use super::MonitorDevice;
    use std::mem::size_of;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };

    const MONITORINFOF_PRIMARY: u32 = 1;

    pub fn list_monitors() -> Vec<MonitorDevice> {
        let mut list: Vec<MonitorDevice> = Vec::new();
        let list_ptr = &mut list as *mut Vec<MonitorDevice> as isize;

        unsafe extern "system" fn monitor_enum_proc(
            hmonitor: HMONITOR,
            _hdc: HDC,
            _rect: *mut RECT,
            lparam: LPARAM,
        ) -> BOOL {
            unsafe {
                let list = &mut *(lparam.0 as *mut Vec<MonitorDevice>);
                let mut mi = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    rcMonitor: RECT::default(),
                    rcWork: RECT::default(),
                    dwFlags: 0,
                };

                if GetMonitorInfoW(hmonitor, &mut mi).as_bool() {
                    let is_primary = (mi.dwFlags & MONITORINFOF_PRIMARY) != 0;
                    let index = list.len();
                    let width = mi.rcMonitor.right - mi.rcMonitor.left;
                    let height = mi.rcMonitor.bottom - mi.rcMonitor.top;

                    let work_width = mi.rcWork.right - mi.rcWork.left;
                    let work_height = mi.rcWork.bottom - mi.rcWork.top;

                    list.push(MonitorDevice {
                        index,
                        name: format!("Monitor {} ({})", index + 1, if is_primary { "Principal" } else { "Secundário" }),
                        is_primary,
                        left: mi.rcMonitor.left,
                        top: mi.rcMonitor.top,
                        width,
                        height,
                        work_left: mi.rcWork.left,
                        work_top: mi.rcWork.top,
                        work_right: mi.rcWork.right,
                        work_bottom: mi.rcWork.bottom,
                        work_width,
                        work_height,
                    });
                }

                BOOL(1)
            }
        }

        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(monitor_enum_proc), LPARAM(list_ptr));
        }

        if list.is_empty() {
            list.push(MonitorDevice {
                index: 0,
                name: "Monitor Principal".to_string(),
                is_primary: true,
                left: 0,
                top: 0,
                width: 1920,
                height: 1080,
                work_left: 0,
                work_top: 0,
                work_right: 1920,
                work_bottom: 1040,
                work_width: 1920,
                work_height: 1040,
            });
        }

        list
    }

    #[allow(dead_code)]
    pub fn list_monitors_for_window(_window: Option<&slint::Window>) -> Vec<MonitorDevice> {
        list_monitors()
    }
}

#[cfg(not(target_os = "windows"))]
pub mod win32 {
    use super::MonitorDevice;
    use slint::winit_030::WinitWindowAccessor;

    struct X11DesktopGeometry {
        workarea: Option<(i32, i32, i32, i32)>,
        screen_size: Option<(i32, i32)>,
    }

    pub fn build_monitor_device(
        index: usize,
        name: String,
        is_primary: bool,
        pos: (i32, i32),
        size: (i32, i32),
        workarea: Option<(i32, i32, i32, i32)>,
    ) -> MonitorDevice {
        let (left, top) = pos;
        let (width, height) = size;
        let right = left + width;
        let bottom = top + height;

        let (work_left, work_top, work_right, work_bottom) = match workarea {
            Some((wx, wy, ww, wh)) if ww > 0 && wh > 0 => {
                let wr = wx + ww;
                let wb = wy + wh;
                let il = left.max(wx);
                let it = top.max(wy);
                let ir = right.min(wr);
                let ib = bottom.min(wb);
                if ir > il && ib > it {
                    (il, it, ir, ib)
                } else {
                    (left, top, right, bottom)
                }
            }
            _ => (left, top, right, bottom),
        };

        MonitorDevice {
            index,
            name,
            is_primary,
            left,
            top,
            width,
            height,
            work_left,
            work_top,
            work_right,
            work_bottom,
            work_width: (work_right - work_left).max(1),
            work_height: (work_bottom - work_top).max(1),
        }
    }

    fn query_x11_workarea_and_screen() -> X11DesktopGeometry {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            return X11DesktopGeometry {
                workarea: None,
                screen_size: None,
            };
        };
        let Some(screen) = conn.setup().roots.get(screen_num) else {
            return X11DesktopGeometry {
                workarea: None,
                screen_size: None,
            };
        };
        let screen_dims = Some((
            screen.width_in_pixels as i32,
            screen.height_in_pixels as i32,
        ));
        let root = screen.root;

        let Ok(atom_cookie) = conn.intern_atom(false, b"_NET_WORKAREA") else {
            return X11DesktopGeometry {
                workarea: None,
                screen_size: screen_dims,
            };
        };
        let Ok(atom_reply) = atom_cookie.reply() else {
            return X11DesktopGeometry {
                workarea: None,
                screen_size: screen_dims,
            };
        };
        if atom_reply.atom == 0 {
            return X11DesktopGeometry {
                workarea: None,
                screen_size: screen_dims,
            };
        }

        let Ok(prop_cookie) = conn.get_property(
            false,
            root,
            atom_reply.atom,
            AtomEnum::CARDINAL,
            0,
            4,
        ) else {
            return X11DesktopGeometry {
                workarea: None,
                screen_size: screen_dims,
            };
        };
        let Ok(prop) = prop_cookie.reply() else {
            return X11DesktopGeometry {
                workarea: None,
                screen_size: screen_dims,
            };
        };
        if let Some(mut vals) = prop.value32()
            && let (Some(x), Some(y), Some(w), Some(h)) =
                (vals.next(), vals.next(), vals.next(), vals.next())
            && w > 0
            && h > 0
        {
            return X11DesktopGeometry {
                workarea: Some((x as i32, y as i32, w as i32, h as i32)),
                screen_size: screen_dims,
            };
        }
        X11DesktopGeometry {
            workarea: None,
            screen_size: screen_dims,
        }
    }

    pub fn list_monitors_for_window(window: Option<&slint::Window>) -> Vec<MonitorDevice> {
        let x11_geom = query_x11_workarea_and_screen();
        let mut list = Vec::new();

        if let Some(win) = window {
            win.with_winit_window(|winit_win| {
                let primary = winit_win.primary_monitor();
                let current = winit_win.current_monitor();
                let mut raw_monitors: Vec<_> = winit_win.available_monitors().collect();
                if raw_monitors.is_empty()
                    && let Some(cur) = current.clone()
                {
                    raw_monitors.push(cur);
                }

                // Order primary monitor first, then left-to-right / top-to-bottom
                raw_monitors.sort_by_key(|m| {
                    let is_prim = primary.as_ref().is_some_and(|p| p == m);
                    let pos = m.position();
                    (!is_prim, pos.x, pos.y)
                });

                for (idx, m) in raw_monitors.into_iter().enumerate() {
                    let pos = m.position();
                    let size = m.size();
                    let is_primary = primary.as_ref().is_some_and(|p| p == &m) || idx == 0;
                    let raw_name = m.name().unwrap_or_else(|| format!("Monitor {}", idx + 1));
                    let label = format!(
                        "{} ({})",
                        raw_name,
                        if is_primary { "Principal" } else { "Secundário" }
                    );
                    list.push(build_monitor_device(
                        idx,
                        label,
                        is_primary,
                        (pos.x, pos.y),
                        (size.width as i32, size.height as i32),
                        x11_geom.workarea,
                    ));
                }
            });
        }

        if list.is_empty() {
            let (sw, sh) = x11_geom.screen_size.unwrap_or((1920, 1080));
            list.push(build_monitor_device(
                0,
                "Monitor Padrão".to_string(),
                true,
                (0, 0),
                (sw, sh),
                x11_geom.workarea,
            ));
        }

        list
    }

    pub fn list_monitors() -> Vec<MonitorDevice> {
        list_monitors_for_window(None)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn build_monitor_device_intersects_workarea_and_handles_secondary_monitors() {
            // Primary 2560x1440 monitor with top bar (32px) and bottom dock (48px)
            let primary = build_monitor_device(
                0,
                "DP-1 (Principal)".to_string(),
                true,
                (0, 0),
                (2560, 1440),
                Some((0, 32, 2560, 1360)),
            );
            assert_eq!(primary.work_left, 0);
            assert_eq!(primary.work_top, 32);
            assert_eq!(primary.work_right, 2560);
            assert_eq!(primary.work_bottom, 1392);
            assert_eq!(primary.work_width, 2560);
            assert_eq!(primary.work_height, 1360);

            // Secondary monitor outside primary _NET_WORKAREA keeps its own bounds
            let secondary = build_monitor_device(
                1,
                "HDMI-1 (Secundário)".to_string(),
                false,
                (2560, 0),
                (1920, 1080),
                Some((0, 32, 2560, 1360)),
            );
            assert_eq!(secondary.work_left, 2560);
            assert_eq!(secondary.work_top, 0);
            assert_eq!(secondary.work_right, 4480);
            assert_eq!(secondary.work_width, 1920);
            assert_eq!(secondary.work_height, 1080);
        }
    }
}
