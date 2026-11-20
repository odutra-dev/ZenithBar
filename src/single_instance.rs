use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
pub use win32::SingleInstanceGuard;

#[cfg(not(target_os = "windows"))]
pub use unix::SingleInstanceGuard;

#[cfg(target_os = "windows")]
mod win32 {
    use super::*;
    use windows::core::{w, BOOL};
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, WPARAM,
    };
    use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
        SetForegroundWindow, ShowWindow, SW_SHOW,
    };

    pub struct SingleInstanceGuard {
        handle: HANDLE,
    }

    impl SingleInstanceGuard {
        pub fn acquire(wait_timeout: Duration) -> Option<Self> {
            let deadline = Instant::now() + wait_timeout;
            loop {
                unsafe {
                    let handle = match CreateMutexW(
                        None,
                        false,
                        w!("Local\\ZenithBar_SingleInstance_Mutex"),
                    ) {
                        Ok(h) => h,
                        Err(_) => return None,
                    };

                    if GetLastError() == ERROR_ALREADY_EXISTS {
                        let _ = CloseHandle(handle);
                        if Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(50));
                            continue;
                        }
                        notify_existing_instance();
                        return None;
                    }

                    return Some(Self { handle });
                }
            }
        }
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            unsafe {
                if !self.handle.is_invalid() {
                    let _ = CloseHandle(self.handle);
                }
            }
        }
    }

    fn notify_existing_instance() {
        if let Some(hwnd) = find_existing_main_hwnd() {
            unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    crate::tray::win32::WM_SHOW_EXISTING_INSTANCE,
                    WPARAM(0),
                    LPARAM(0),
                );
                if !IsWindowVisible(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                }
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }

    fn find_existing_main_hwnd() -> Option<HWND> {
        struct SearchData {
            current_pid: u32,
            found: Option<HWND>,
        }

        let mut data = SearchData {
            current_pid: unsafe { GetCurrentProcessId() },
            found: None,
        };

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            unsafe {
                let data = &mut *(lparam.0 as *mut SearchData);
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != 0 && pid != data.current_pid {
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

        unsafe {
            let _ = EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut SearchData as isize));
        }
        data.found
    }
}

#[cfg(not(target_os = "windows"))]
mod unix {
    use super::*;
    use std::fs::{File, OpenOptions, TryLockError};
    use std::path::{Path, PathBuf};

    pub struct SingleInstanceGuard {
        _file: File,
    }

    impl SingleInstanceGuard {
        pub fn acquire(wait_timeout: Duration) -> Option<Self> {
            let path = default_lock_path();
            Self::acquire_at(&path, wait_timeout)
        }

        pub fn acquire_at(path: &Path, wait_timeout: Duration) -> Option<Self> {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(path)
                .ok()?;

            let deadline = Instant::now() + wait_timeout;
            loop {
                match file.try_lock() {
                    Ok(()) => return Some(Self { _file: file }),
                    Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => return None,
                }
            }
        }
    }

    fn default_lock_path() -> PathBuf {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            if !runtime_dir.is_empty() {
                return PathBuf::from(runtime_dir).join("zenithbar.lock");
            }
        }
        std::env::temp_dir().join("zenithbar.lock")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn prevents_second_instance_until_first_drops() {
            let path = std::env::temp_dir()
                .join(format!("zenithbar_single_instance_test_{}.lock", std::process::id()));
            let _ = std::fs::remove_file(&path);

            let first = SingleInstanceGuard::acquire_at(&path, Duration::ZERO);
            assert!(first.is_some(), "first instance must acquire lock");

            let second = SingleInstanceGuard::acquire_at(&path, Duration::ZERO);
            assert!(second.is_none(), "second instance must be rejected while first is active");

            drop(first);

            let after_drop = SingleInstanceGuard::acquire_at(&path, Duration::ZERO);
            assert!(after_drop.is_some(), "lock must be acquirable after first instance exits");
            drop(after_drop);

            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn restarted_instance_waits_for_previous_exit() {
            let path = std::env::temp_dir()
                .join(format!("zenithbar_restart_wait_test_{}.lock", std::process::id()));
            let _ = std::fs::remove_file(&path);

            let first = SingleInstanceGuard::acquire_at(&path, Duration::ZERO).unwrap();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                drop(first);
            });

            let restarted = SingleInstanceGuard::acquire_at(&path, Duration::from_secs(2));
            assert!(restarted.is_some(), "restarted instance should acquire lock once old process exits");
            drop(restarted);

            let _ = std::fs::remove_file(&path);
        }
    }
}
