#[derive(Debug, Clone)]
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
}

#[cfg(not(target_os = "windows"))]
pub mod win32 {
    use super::MonitorDevice;
    pub fn list_monitors() -> Vec<MonitorDevice> {
        vec![MonitorDevice {
            index: 0,
            name: "Monitor Padrão".to_string(),
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
        }]
    }
}
