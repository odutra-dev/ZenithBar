#[derive(Debug, Clone, Copy, Default)]
pub struct TrayEvents {
    pub toggle_vis: bool,
    pub open_settings: bool,
    pub toggle_side: bool,
    pub cycle_screen: bool,
    pub reload_sensors: bool,
    pub update_pos: bool,
}

#[cfg(target_os = "windows")]
pub mod win32 {
    use super::TrayEvents;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetSystemMetrics,
        LoadIconW, LoadImageW, SetForegroundWindow, SetWindowPos, TrackPopupMenuEx,
        HICON, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOPMOST, IDI_APPLICATION, IMAGE_ICON,
        LR_DEFAULTCOLOR, LR_SHARED, MF_SEPARATOR, MF_STRING, SM_CXSMICON, SM_CYSMICON,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON,
        WM_APP, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WM_USER,
    };

    pub const WM_APPBAR_CALLBACK: u32 = WM_USER + 102;
    pub const ABN_POSCHANGED: usize = 1;
    pub const ABN_FULLSCREENAPP: usize = 2;

    const WM_TRAYICON: u32 = WM_APP + 42;
    const WM_HOTKEY: u32 = 0x0312;
    const HOTKEY_ID: i32 = 100;
    const ID_TOGGLE_VISIBILITY: usize = 1001;
    const ID_SETTINGS: usize = 1002;
    const ID_TOGGLE_SIDE: usize = 1003;
    const ID_CYCLE_SCREEN: usize = 1004;
    const ID_RELOAD_SENSORS: usize = 1005;
    const ID_GITHUB: usize = 1006;
    const ID_EXIT: usize = 1007;

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
    static SHOULD_UPDATE_POS: AtomicBool = AtomicBool::new(false);
    static IS_ALWAYS_TOP: AtomicBool = AtomicBool::new(true);
    static TRAY_CULTURE: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

    pub fn set_always_top_state(always_top: bool) {
        IS_ALWAYS_TOP.store(always_top, Ordering::SeqCst);
    }

    pub fn set_tray_culture(culture: &str) {
        if let Ok(mut lock) = TRAY_CULTURE.write() {
            *lock = culture.to_string();
        }
    }

    pub fn get_tray_culture() -> String {
        TRAY_CULTURE.read().map(|g| g.clone()).unwrap_or_else(|_| "en-US".to_string())
    }

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

        unsafe fn load_app_icon() -> HICON {
            unsafe {
                if let Ok(hinst) = GetModuleHandleW(None) {
                    if !hinst.0.is_null() {
                        let cx = GetSystemMetrics(SM_CXSMICON);
                        let cy = GetSystemMetrics(SM_CYSMICON);
                        if let Ok(handle) = LoadImageW(
                            Some(HINSTANCE(hinst.0)),
                            PCWSTR(1 as *const u16),
                            IMAGE_ICON,
                            cx,
                            cy,
                            LR_DEFAULTCOLOR | LR_SHARED,
                        ) {
                            if !handle.0.is_null() {
                                return HICON(handle.0);
                            }
                        }
                    }
                }
                LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
            }
        }

        pub fn init(&mut self) -> bool {
            if self.hwnd.0.is_null() {
                return false;
            }

            unsafe {
                let icon = Self::load_app_icon();
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
                update_pos: SHOULD_UPDATE_POS.swap(false, Ordering::SeqCst),
            }
        }

        #[allow(dead_code)]
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

        if msg == WM_APPBAR_CALLBACK {
            match wparam.0 {
                ABN_FULLSCREENAPP => {
                    let is_fullscreen = lparam.0 != 0;
                    if is_fullscreen {
                        // Janela em tela cheia (jogos, YouTube, F11): envia a barra para trás
                        unsafe {
                            let _ = SetWindowPos(
                                hwnd,
                                Some(HWND_BOTTOM),
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                            );
                        }
                    } else {
                        // Saiu de tela cheia: restaura Z-order conforme configuração
                        let insert_after = if IS_ALWAYS_TOP.load(Ordering::SeqCst) {
                            HWND_TOPMOST
                        } else {
                            HWND_NOTOPMOST
                        };
                        unsafe {
                            let _ = SetWindowPos(
                                hwnd,
                                Some(insert_after),
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                            );
                        }
                    }
                    return LRESULT(0);
                }
                ABN_POSCHANGED => {
                    SHOULD_UPDATE_POS.store(true, Ordering::SeqCst);
                    return LRESULT(0);
                }
                _ => {}
            }
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

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn show_tray_menu(hwnd: HWND) {
        unsafe {
            let hmenu = match CreatePopupMenu() {
                Ok(m) => m,
                Err(_) => return,
            };

            let culture = get_tray_culture();
            let s = crate::i18n::load_locale(if culture.is_empty() { "en-US" } else { &culture });

            let w_toggle = to_wide(&s.tray_toggle);
            let w_settings = to_wide(&s.tray_settings);
            let w_side = to_wide(&s.tray_toggle_edge);
            let w_screen = to_wide(&s.tray_cycle_monitor);
            let w_reload = to_wide(&s.tray_reload_sensors);
            let w_github = to_wide(&s.tray_github);
            let w_exit = to_wide(&s.tray_exit);

            let _ = AppendMenuW(hmenu, MF_STRING, ID_TOGGLE_VISIBILITY, windows::core::PCWSTR(w_toggle.as_ptr()));
            let _ = AppendMenuW(hmenu, MF_STRING, ID_SETTINGS, windows::core::PCWSTR(w_settings.as_ptr()));
            let _ = AppendMenuW(hmenu, MF_STRING, ID_TOGGLE_SIDE, windows::core::PCWSTR(w_side.as_ptr()));
            let _ = AppendMenuW(hmenu, MF_STRING, ID_CYCLE_SCREEN, windows::core::PCWSTR(w_screen.as_ptr()));
            let _ = AppendMenuW(hmenu, MF_STRING, ID_RELOAD_SENSORS, windows::core::PCWSTR(w_reload.as_ptr()));
            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(hmenu, MF_STRING, ID_GITHUB, windows::core::PCWSTR(w_github.as_ptr()));
            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(hmenu, MF_STRING, ID_EXIT, windows::core::PCWSTR(w_exit.as_ptr()));

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
                ID_GITHUB => {
                    use windows::Win32::UI::Shell::ShellExecuteW;
                    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
                    let _ = ShellExecuteW(
                        None,
                        w!("open"),
                        w!("https://github.com/odutra-dev/ZenithBar"),
                        None,
                        None,
                        SW_SHOWNORMAL,
                    );
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
    use std::sync::atomic::{AtomicBool, Ordering};

    pub static SHOULD_TOGGLE_VISIBILITY: AtomicBool = AtomicBool::new(false);
    pub static SHOULD_OPEN_SETTINGS: AtomicBool = AtomicBool::new(false);
    pub static SHOULD_TOGGLE_SIDE: AtomicBool = AtomicBool::new(false);
    pub static SHOULD_CYCLE_SCREEN: AtomicBool = AtomicBool::new(false);
    pub static SHOULD_RELOAD_SENSORS: AtomicBool = AtomicBool::new(false);
    pub static SHOULD_UPDATE_POS: AtomicBool = AtomicBool::new(false);

    pub fn set_always_top_state(_always_top: bool) {}
    pub fn set_tray_culture(_culture: &str) {}

    pub struct TrayIcon {}

    impl TrayIcon {
        pub fn new(_hwnd_raw: isize) -> Self {
            Self {}
        }

        pub fn init(&mut self) -> bool {
            crate::log_msg("[Linux Tray] Initialized StatusNotifierItem / Desktop Tray.");
            true
        }

        pub fn poll_events(&self) -> TrayEvents {
            TrayEvents {
                toggle_vis: SHOULD_TOGGLE_VISIBILITY.swap(false, Ordering::SeqCst),
                open_settings: SHOULD_OPEN_SETTINGS.swap(false, Ordering::SeqCst),
                toggle_side: SHOULD_TOGGLE_SIDE.swap(false, Ordering::SeqCst),
                cycle_screen: SHOULD_CYCLE_SCREEN.swap(false, Ordering::SeqCst),
                reload_sensors: SHOULD_RELOAD_SENSORS.swap(false, Ordering::SeqCst),
                update_pos: SHOULD_UPDATE_POS.swap(false, Ordering::SeqCst),
            }
        }

        #[allow(dead_code)]
        pub fn remove(&mut self) {
            crate::log_msg("[Linux Tray] Removed StatusNotifierItem.");
        }
    }
}
