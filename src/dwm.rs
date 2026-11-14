#[cfg(target_os = "windows")]
pub mod win32 {
    use std::ffi::c_void;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DWMWA_WINDOW_CORNER_PREFERENCE,
    };

    pub fn apply_fluent_effects(hwnd_raw: isize) {
        if hwnd_raw == 0 {
            return;
        }
        let hwnd = HWND(hwnd_raw as *mut _);

        unsafe {
            // Enable Immersive Dark Mode for window title and borders
            let dark_mode: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode as *const _ as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );

            // Apply Acrylic / Transient Backdrop (Value = 3 for Acrylic, 2 for Mica)
            let backdrop: i32 = 3; // DWMSBT_TRANSIENTWINDOW (Acrylic)
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop as *const _ as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );

            // Corner preference: Slightly rounded
            let corner: i32 = 1; // DWMWCP_DONOTROUND = 1, DWMWCP_ROUND = 2, DWMWCP_ROUNDSMALL = 3
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub mod win32 {
    #[allow(dead_code)]
    pub fn apply_fluent_effects(_hwnd_raw: isize) {}
}
