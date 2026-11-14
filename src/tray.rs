#[derive(Debug, Clone, Copy, Default)]
pub struct TrayEvents {
    pub toggle_vis: bool,
    pub open_settings: bool,
    pub toggle_side: bool,
    pub cycle_screen: bool,
    pub reload_sensors: bool,
}

#[cfg(target_os = "windows")]
pub mod win32 {
    use super::TrayEvents;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::UI::Shell::{
        DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW,
        SetForegroundWindow, TrackPopupMenuEx, IDI_APPLICATION,
        MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON,
        WM_APP, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP,
    };

    const WM_TRAYICON: u32 = WM_APP + 42;
    const WM_HOTKEY: u32 = 0x0312;
    const HOTKEY_ID: i32 = 100;
    const ID_TOGGLE_VISIBILITY: usize = 1001;
    const ID_SETTINGS: usize = 1002;
    const ID_TOGGLE_SIDE: usize = 1003;
    const ID_CYCLE_SCREEN: usize = 1004;
    const ID_RELOAD_SENSORS: usize = 1005;
    const ID_EXIT: usize = 1006;

    unsafe extern "system" {
        fn RegisterHotKey(hwnd: HWND, id: i32, fs_modifiers: u32, vk: u32) -> windows::core::BOOL;
        fn UnregisterHotKey(hwnd: HWND, id: i32) -> windows::core::BOOL;
    }

    static TRAY_SUBCLASS_ID: usize = 999;
    static SHOULD_TOGGLE_VISIBILITY: AtomicBool = AtomicBool::new(false);
    static SHOULD_OPEN_SETTINGS: AtomicBool = AtomicBool::new(false);
    static SHOULD_TOGGLE_SIDE: AtomicBool = AtomicBool::new(false);
    static SHOULD_CYCLE_SCREEN: AtomicBool = AtomicBool::new(false);
    static SHOULD_RELOAD_SENSORS: AtomicBool = AtomicBool::new(false);

    pub struct TrayIcon {
        hwnd: HWND,
        is_added: bool,
    }

    impl TrayIcon {
        pub fn new(hwnd_raw: isize) -> Self {
            Self {
                hwnd: HWND(hwnd_raw as *mut _),
                is_added: false,
            }
        }

        pub fn init(&mut self) -> bool {
            if self.hwnd.0.is_null() {
                return false;
            }

            unsafe {
                let icon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
                let mut tip = [0u16; 128];
                let title = "ZenithBar - Hardware Diagnostics\0".encode_utf16().collect::<Vec<u16>>();
                for (i, c) in title.iter().take(127).enumerate() {
                    tip[i] = *c;
                }

                let mut nid = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: self.hwnd,
                    uID: 1,
                    uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                    uCallbackMessage: WM_TRAYICON,
                    hIcon: icon,
                    szTip: tip,
                    ..Default::default()
                };

                let res = Shell_NotifyIconW(NIM_ADD, &mut nid).as_bool();
                self.is_added = res;

                if self.is_added {
                    let _ = SetWindowSubclass(
                        self.hwnd,
                        Some(tray_subclass_proc),
                        TRAY_SUBCLASS_ID,
                        0,
                    );
                    // Register Global Hotkey: Ctrl + Shift + S
                    let _ = RegisterHotKey(self.hwnd, HOTKEY_ID, 0x0006, 0x53);
                }

                self.is_added
            }
        }

        pub fn poll_events(&self) -> TrayEvents {
            TrayEvents {
                toggle_vis: SHOULD_TOGGLE_VISIBILITY.swap(false, Ordering::SeqCst),
                open_settings: SHOULD_OPEN_SETTINGS.swap(false, Ordering::SeqCst),
                toggle_side: SHOULD_TOGGLE_SIDE.swap(false, Ordering::SeqCst),
                cycle_screen: SHOULD_CYCLE_SCREEN.swap(false, Ordering::SeqCst),
                reload_sensors: SHOULD_RELOAD_SENSORS.swap(false, Ordering::SeqCst),
            }
        }

        pub fn remove(&mut self) {
            if self.is_added && !self.hwnd.0.is_null() {
                unsafe {
                    let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID);
                    let mut nid = NOTIFYICONDATAW {
                        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                        hWnd: self.hwnd,
                        uID: 1,
                        ..Default::default()
                    };
                    let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
                    let _ = RemoveWindowSubclass(self.hwnd, Some(tray_subclass_proc), TRAY_SUBCLASS_ID);
                }
                self.is_added = false;
            }
        }
    }

    impl Drop for TrayIcon {
        fn drop(&mut self) {
            self.remove();
        }
    }

    unsafe extern "system" fn tray_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uid_subclass: usize,
        _ref_data: usize,
    ) -> LRESULT {
        if msg == WM_HOTKEY {
            SHOULD_TOGGLE_VISIBILITY.store(true, Ordering::SeqCst);
            return LRESULT(0);
        }

        if msg == WM_TRAYICON {
            let event = lparam.0 as u32;
            match event {
                WM_RBUTTONUP => {
                    unsafe { show_tray_menu(hwnd); }
                    return LRESULT(0);
                }
                WM_LBUTTONDBLCLK => {
                    SHOULD_OPEN_SETTINGS.store(true, Ordering::SeqCst);
                    return LRESULT(0);
                }
                WM_LBUTTONUP => {
                    SHOULD_TOGGLE_VISIBILITY.store(true, Ordering::SeqCst);
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    unsafe fn show_tray_menu(hwnd: HWND) {
        unsafe {
            let hmenu = match CreatePopupMenu() {
                Ok(m) => m,
                Err(_) => return,
            };

            let _ = AppendMenuW(hmenu, MF_STRING, ID_TOGGLE_VISIBILITY, w!("👁  Mostrar / Ocultar Barra"));
            let _ = AppendMenuW(hmenu, MF_STRING, ID_SETTINGS, w!("⚙  Configurações..."));
            let _ = AppendMenuW(hmenu, MF_STRING, ID_TOGGLE_SIDE, w!("↔  Alternar Lado (Esq/Dir)"));
            let _ = AppendMenuW(hmenu, MF_STRING, ID_CYCLE_SCREEN, w!("🖥  Alternar Monitor"));
            let _ = AppendMenuW(hmenu, MF_STRING, ID_RELOAD_SENSORS, w!("🔄  Recarregar Sensores"));
            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(hmenu, MF_STRING, ID_EXIT, w!("✕  Sair do ZenithBar"));

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);

            let _ = SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenuEx(
                hmenu,
                TPM_RIGHTBUTTON.0 | TPM_BOTTOMALIGN.0 | 0x0100, // TPM_RETURNCMD = 0x0100
                pt.x,
                pt.y,
                hwnd,
                None,
            );
            let _ = DestroyMenu(hmenu);

            match cmd.0 as usize {
                ID_TOGGLE_VISIBILITY => {
                    SHOULD_TOGGLE_VISIBILITY.store(true, Ordering::SeqCst);
                }
                ID_SETTINGS => {
                    SHOULD_OPEN_SETTINGS.store(true, Ordering::SeqCst);
                }
                ID_TOGGLE_SIDE => {
                    SHOULD_TOGGLE_SIDE.store(true, Ordering::SeqCst);
                }
                ID_CYCLE_SCREEN => {
                    SHOULD_CYCLE_SCREEN.store(true, Ordering::SeqCst);
                }
                ID_RELOAD_SENSORS => {
                    SHOULD_RELOAD_SENSORS.store(true, Ordering::SeqCst);
                }
                ID_EXIT => {
                    std::process::exit(0);
                }
                _ => {}
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub mod win32 {
    use super::TrayEvents;
    pub struct TrayIcon {}
    impl TrayIcon {
        pub fn new(_hwnd_raw: isize) -> Self { Self {} }
        pub fn init(&mut self) -> bool { true }
        pub fn poll_events(&self) -> TrayEvents { TrayEvents::default() }
        pub fn remove(&mut self) {}
    }
}
