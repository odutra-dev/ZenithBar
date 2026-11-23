use std::mem::size_of;

pub const ZENITH_SHM_MAGIC: u32 = 0x5A42_5348; // "ZBSH"
pub const ZENITH_SHM_VERSION: u32 = 1;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const ZENITH_SHM_MAX_AGE_MS: u64 = 15_000;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) const PAWNIO_SETUP_ARGS: [&str; 2] = ["-install", "-silent"];

#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct ZenithSensorShm {
    pub magic: u32,
    pub version: u32,
    pub sequence: u64,
    pub updated_unix_ms: u64,
    pub cpu_temp_c: f32,
    pub cpu_temp_valid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum PawnCpuKind {
    AmdFamily17,
    IntelMsr,
    Unsupported,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn parse_zenith_shm_bytes(bytes: &[u8], now_unix_ms: u64) -> Option<f32> {
    if bytes.len() < size_of::<ZenithSensorShm>() {
        return None;
    }
    let hdr = unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const ZenithSensorShm) };
    if hdr.magic != ZENITH_SHM_MAGIC || hdr.version != ZENITH_SHM_VERSION {
        return None;
    }
    if hdr.sequence == 0 || !hdr.sequence.is_multiple_of(2) {
        return None;
    }
    if now_unix_ms.saturating_sub(hdr.updated_unix_ms) > ZENITH_SHM_MAX_AGE_MS {
        return None;
    }
    if hdr.cpu_temp_valid == 1
        && hdr.cpu_temp_c.is_finite()
        && hdr.cpu_temp_c > 0.0
        && hdr.cpu_temp_c < 130.0
    {
        Some(hdr.cpu_temp_c)
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn amd_tctl_offset(cpu_name: &str) -> f32 {
    if cpu_name.contains("1600X")
        || cpu_name.contains("1700X")
        || cpu_name.contains("1800X")
    {
        -20.0
    } else if cpu_name.contains("Threadripper 19") || cpu_name.contains("Threadripper 29") {
        -27.0
    } else if cpu_name.contains("2700X") {
        -10.0
    } else {
        0.0
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn decode_amd_zen_cur_tmp(raw: u32, cpu_name: &str) -> Option<f32> {
    const F17H_TEMP_RANGE_SEL_MASK: u32 = 0x80000;
    const F17H_TEMP_TJ_SEL_MASK: u32 = 0x30000;

    let temp_offset_flag = (raw & F17H_TEMP_RANGE_SEL_MASK) != 0
        || (raw & F17H_TEMP_TJ_SEL_MASK) == F17H_TEMP_TJ_SEL_MASK;
    let millideg = (raw >> 21) * 125;
    let mut temp = millideg as f32 * 0.001;
    if temp_offset_flag {
        temp -= 49.0;
    }
    temp += amd_tctl_offset(cpu_name);

    if temp.is_finite() && temp > 0.0 && temp < 130.0 {
        Some(temp)
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn decode_intel_msr_temp(therm_status: u64, tj_max_msr: Option<u64>) -> Option<f32> {
    if (therm_status & 0x8000_0000) == 0 {
        return None;
    }
    let tj_max = tj_max_msr
        .map(|v| ((v >> 16) & 0xFF) as f32)
        .filter(|&v| v > 50.0 && v < 130.0)
        .unwrap_or(100.0);
    let delta = ((therm_status >> 16) & 0x7F) as f32;
    let temp = tj_max - delta;
    if temp.is_finite() && temp > 0.0 && temp < 130.0 {
        Some(temp)
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn classify_cpu_arch_kind(vendor: &str, family: u32) -> PawnCpuKind {
    match vendor {
        "AuthenticAMD" if (0x17..=0x1A).contains(&family) => PawnCpuKind::AmdFamily17,
        "GenuineIntel" => PawnCpuKind::IntelMsr,
        _ => PawnCpuKind::Unsupported,
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn select_release_asset_url(
    release_json: &serde_json::Value,
    required_suffix: &str,
) -> Option<String> {
    let assets = release_json["assets"].as_array()?;
    let suffix_lower = required_suffix.to_ascii_lowercase();
    for asset in assets {
        let name = asset["name"].as_str().unwrap_or("");
        if name.to_ascii_lowercase().ends_with(&suffix_lower)
            && let Some(url) = asset["browser_download_url"].as_str()
        {
            return Some(url.to_string());
        }
    }
    None
}

#[cfg(target_os = "windows")]
mod win_impl {
    use super::*;
    use std::ffi::{c_char, c_void, CString};
    use std::fs;
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use windows::core::{w, PCSTR, PCWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, FreeLibrary, GetLastError, LocalFree, ERROR_SERVICE_ALREADY_RUNNING,
        ERROR_SERVICE_DOES_NOT_EXIST, HANDLE, HLOCAL, HMODULE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0,
    };
    use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Memory::{
        CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
        MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
    };
    use windows::Win32::System::Registry::{
        RegOpenKeyExW, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY,
    };
    use windows::Win32::System::Services::{
        ChangeServiceConfigW, CloseServiceHandle, ControlService, CreateServiceW, OpenSCManagerW,
        OpenServiceW, QueryServiceStatus, RegisterServiceCtrlHandlerExW, SetServiceStatus,
        StartServiceCtrlDispatcherW, StartServiceW, SC_MANAGER_ALL_ACCESS, SC_MANAGER_CONNECT,
        SERVICE_ALL_ACCESS, SERVICE_AUTO_START, SERVICE_CONTROL_INTERROGATE,
        SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_ERROR_NORMAL,
        SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS,
        SERVICE_STATUS_HANDLE, SERVICE_STOPPED, SERVICE_STOP_PENDING, SERVICE_TABLE_ENTRYW,
        SERVICE_WIN32_OWN_PROCESS,
    };
    use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, SW_SHOWNORMAL,
    };

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const GENERIC_READ_ACCESS: u32 = 0x80000000;
    const GENERIC_WRITE_ACCESS: u32 = 0x40000000;
    const IOCTL_PIO_LOAD_BINARY: u32 = (41394u32 << 16) | (0x821u32 << 2);
    const IOCTL_PIO_EXECUTE_FN: u32 = (41394u32 << 16) | (0x841u32 << 2);
    const FN_NAME_LENGTH: usize = 32;

    const F17H_M01H_THM_TCON_CUR_TMP: u64 = 0x0005_9800;
    const MSR_IA32_THERM_STATUS: u64 = 0x019C;
    const MSR_IA32_TEMPERATURE_TARGET: u64 = 0x01A2;
    const MSR_IA32_PACKAGE_THERM_STATUS: u64 = 0x01B1;

    static SERVICE_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
    static mut SERVICE_STATUS_HANDLE_GLOBAL: SERVICE_STATUS_HANDLE =
        SERVICE_STATUS_HANDLE(std::ptr::null_mut());

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn program_data_dir() -> PathBuf {
        let base = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string());
        PathBuf::from(base).join("ZenithBar")
    }

    fn pawnio_modules_dir() -> PathBuf {
        program_data_dir().join("pawnio")
    }

    fn program_files_pawnio_dll() -> PathBuf {
        let pf = std::env::var("ProgramFiles")
            .unwrap_or_else(|_| "C:\\Program Files".to_string());
        PathBuf::from(pf).join("PawnIO").join("PawnIOLib.dll")
    }

    pub fn is_pawnio_installed() -> bool {
        if program_files_pawnio_dll().is_file() {
            return true;
        }
        unsafe {
            let mut hkey = windows::Win32::System::Registry::HKEY::default();
            let subkey = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\PawnIO");
            if RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                subkey,
                Some(0),
                KEY_READ | KEY_WOW64_64KEY,
                &mut hkey,
            )
            .is_ok()
            {
                let _ = windows::Win32::System::Registry::RegCloseKey(hkey);
                return true;
            }
        }
        false
    }

    pub fn is_sensor_service_running() -> bool {
        unsafe {
            let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else {
                return false;
            };
            let res = if let Ok(svc) = OpenServiceW(scm, w!("ZenithBarSensor"), SERVICE_QUERY_STATUS) {
                let mut status = SERVICE_STATUS::default();
                let running = QueryServiceStatus(svc, &mut status).is_ok()
                    && status.dwCurrentState == SERVICE_RUNNING;
                let _ = CloseServiceHandle(svc);
                running
            } else {
                false
            };
            let _ = CloseServiceHandle(scm);
            res
        }
    }

    pub fn launch_elevated_pawnio_setup() -> Result<(), String> {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to locate current executable: {}", e))?;
        let exe_w = to_wide(&exe.to_string_lossy());
        let args_w = to_wide("--setup-pawnio");
        unsafe {
            let hinst = ShellExecuteW(
                None,
                w!("runas"),
                PCWSTR(exe_w.as_ptr()),
                PCWSTR(args_w.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
            if (hinst.0 as isize) <= 32 {
                return Err(format!(
                    "Elevation declined or failed (code={})",
                    hinst.0 as isize
                ));
            }
        }
        Ok(())
    }

    #[cfg(target_arch = "x86_64")]
    fn detect_host_cpu() -> (PawnCpuKind, String) {
        use std::arch::x86_64::__cpuid;
        let cpuid0 = __cpuid(0);
        let mut vendor_bytes = [0u8; 12];
        vendor_bytes[0..4].copy_from_slice(&cpuid0.ebx.to_le_bytes());
        vendor_bytes[4..8].copy_from_slice(&cpuid0.edx.to_le_bytes());
        vendor_bytes[8..12].copy_from_slice(&cpuid0.ecx.to_le_bytes());
        let vendor = String::from_utf8_lossy(&vendor_bytes).to_string();

        let cpuid1 = __cpuid(1);
        let base_family = (cpuid1.eax >> 8) & 0xF;
        let ext_family = (cpuid1.eax >> 20) & 0xFF;
        let family = if base_family == 0xF {
            base_family + ext_family
        } else {
            base_family
        };

        let max_ext = __cpuid(0x8000_0000).eax;
        let mut brand = String::new();
        if max_ext >= 0x8000_0004 {
            let mut buf = Vec::with_capacity(48);
            for leaf in 0x8000_0002..=0x8000_0004 {
                let r = __cpuid(leaf);
                buf.extend_from_slice(&r.eax.to_le_bytes());
                buf.extend_from_slice(&r.ebx.to_le_bytes());
                buf.extend_from_slice(&r.ecx.to_le_bytes());
                buf.extend_from_slice(&r.edx.to_le_bytes());
            }
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            brand = String::from_utf8_lossy(&buf[..end]).trim().to_string();
        }

        (classify_cpu_arch_kind(&vendor, family), brand)
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn detect_host_cpu() -> (PawnCpuKind, String) {
        (PawnCpuKind::Unsupported, String::new())
    }

    type FnPawnOpen = unsafe extern "system" fn(*mut HANDLE) -> i32;
    type FnPawnLoad = unsafe extern "system" fn(HANDLE, *const u8, usize) -> i32;
    type FnPawnExecute = unsafe extern "system" fn(
        HANDLE,
        *const c_char,
        *const u64,
        usize,
        *mut u64,
        usize,
        *mut usize,
    ) -> i32;
    type FnPawnClose = unsafe extern "system" fn(HANDLE) -> i32;

    enum PawnBackend {
        Dll {
            module: HMODULE,
            handle: HANDLE,
            execute: FnPawnExecute,
            close: FnPawnClose,
        },
        Direct {
            handle: HANDLE,
        },
    }

    impl Drop for PawnBackend {
        fn drop(&mut self) {
            unsafe {
                match self {
                    PawnBackend::Dll {
                        module,
                        handle,
                        close,
                        ..
                    } => {
                        if !handle.is_invalid() && !handle.0.is_null() {
                            let _ = close(*handle);
                        }
                        if !module.is_invalid() {
                            let _ = FreeLibrary(*module);
                        }
                    }
                    PawnBackend::Direct { handle } => {
                        if !handle.is_invalid() {
                            let _ = CloseHandle(*handle);
                        }
                    }
                }
            }
        }
    }

    impl PawnBackend {
        fn open_and_load(blob: &[u8]) -> Option<Self> {
            if let Some(dll_backend) = Self::open_via_dll(blob) {
                return Some(dll_backend);
            }
            Self::open_via_direct(blob)
        }

        fn open_via_dll(blob: &[u8]) -> Option<Self> {
            let candidates = [
                program_files_pawnio_dll(),
                pawnio_modules_dir().join("PawnIOLib.dll"),
            ];
            unsafe {
                let mut loaded_mod = None;
                for path in candidates {
                    if path.is_file() {
                        let wpath = to_wide(&path.to_string_lossy());
                        if let Ok(m) = LoadLibraryW(PCWSTR(wpath.as_ptr()))
                            && !m.is_invalid()
                        {
                            loaded_mod = Some(m);
                            break;
                        }
                    }
                }
                let module = loaded_mod?;

                let open: FnPawnOpen = std::mem::transmute_copy(
                    &GetProcAddress(module, PCSTR(c"pawnio_open".as_ptr().cast()))?,
                );
                let load: FnPawnLoad = std::mem::transmute_copy(
                    &GetProcAddress(module, PCSTR(c"pawnio_load".as_ptr().cast()))?,
                );
                let execute: FnPawnExecute = std::mem::transmute_copy(
                    &GetProcAddress(module, PCSTR(c"pawnio_execute".as_ptr().cast()))?,
                );
                let close: FnPawnClose = std::mem::transmute_copy(
                    &GetProcAddress(module, PCSTR(c"pawnio_close".as_ptr().cast()))?,
                );

                let mut handle = HANDLE::default();
                if open(&mut handle) < 0 || handle.is_invalid() || handle.0.is_null() {
                    let _ = FreeLibrary(module);
                    return None;
                }

                if load(handle, blob.as_ptr(), blob.len()) < 0 {
                    let _ = close(handle);
                    let _ = FreeLibrary(module);
                    return None;
                }

                Some(Self::Dll {
                    module,
                    handle,
                    execute,
                    close,
                })
            }
        }

        fn open_via_direct(blob: &[u8]) -> Option<Self> {
            unsafe {
                let handle = CreateFileW(
                    w!("\\\\?\\GLOBALROOT\\Device\\PawnIO"),
                    GENERIC_READ_ACCESS | GENERIC_WRITE_ACCESS,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
                .ok()?;
                if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
                    return None;
                }

                let mut bytes_returned = 0u32;
                let ok = DeviceIoControl(
                    handle,
                    IOCTL_PIO_LOAD_BINARY,
                    Some(blob.as_ptr() as *const c_void),
                    blob.len() as u32,
                    None,
                    0,
                    Some(&mut bytes_returned),
                    None,
                );
                if ok.is_err() {
                    let _ = CloseHandle(handle);
                    return None;
                }
                Some(Self::Direct { handle })
            }
        }

        fn execute(&self, fn_name: &str, input: &[u64], output: &mut [u64]) -> Option<usize> {
            match self {
                PawnBackend::Dll {
                    handle, execute, ..
                } => unsafe {
                    let cname = CString::new(fn_name).ok()?;
                    let mut ret_size = 0usize;
                    let hr = execute(
                        *handle,
                        cname.as_ptr(),
                        input.as_ptr(),
                        input.len(),
                        output.as_mut_ptr(),
                        output.len(),
                        &mut ret_size,
                    );
                    if hr >= 0 {
                        Some(ret_size)
                    } else {
                        None
                    }
                },
                PawnBackend::Direct { handle } => unsafe {
                    let name_bytes = fn_name.as_bytes();
                    if name_bytes.len() >= FN_NAME_LENGTH {
                        return None;
                    }
                    let mut in_buf = vec![0u8; FN_NAME_LENGTH + std::mem::size_of_val(input)];
                    in_buf[..name_bytes.len()].copy_from_slice(name_bytes);
                    for (idx, val) in input.iter().enumerate() {
                        let start = FN_NAME_LENGTH + idx * size_of::<u64>();
                        in_buf[start..start + 8].copy_from_slice(&val.to_le_bytes());
                    }

                    let mut bytes_returned = 0u32;
                    let ok = DeviceIoControl(
                        *handle,
                        IOCTL_PIO_EXECUTE_FN,
                        Some(in_buf.as_ptr() as *const c_void),
                        in_buf.len() as u32,
                        Some(output.as_mut_ptr() as *mut c_void),
                        std::mem::size_of_val(output) as u32,
                        Some(&mut bytes_returned),
                        None,
                    );
                    if ok.is_ok() {
                        Some(bytes_returned as usize / size_of::<u64>())
                    } else {
                        None
                    }
                },
            }
        }
    }

    pub struct PawnIoCpuReader {
        kind: PawnCpuKind,
        cpu_name: String,
        backend: Option<PawnBackend>,
        pci_mutex: Option<HANDLE>,
        tj_max_msr: Option<u64>,
    }

    impl PawnIoCpuReader {
        pub fn new() -> Self {
            let (kind, cpu_name) = detect_host_cpu();
            let pci_mutex = unsafe {
                CreateMutexW(None, false, w!("Global\\Access_PCI"))
                    .ok()
                    .filter(|h| !h.is_invalid())
            };
            let mut reader = Self {
                kind,
                cpu_name,
                backend: None,
                pci_mutex,
                tj_max_msr: None,
            };
            reader.try_connect();
            reader
        }

        fn module_filename(&self) -> Option<&'static str> {
            match self.kind {
                PawnCpuKind::AmdFamily17 => Some("AMDFamily17.bin"),
                PawnCpuKind::IntelMsr => Some("IntelMSR.bin"),
                PawnCpuKind::Unsupported => None,
            }
        }

        fn find_module_bytes(&self) -> Option<Vec<u8>> {
            let fname = self.module_filename()?;
            let mut candidates = vec![pawnio_modules_dir().join(fname)];
            if let Ok(exe) = std::env::current_exe()
                && let Some(dir) = exe.parent()
            {
                candidates.push(dir.join("pawnio").join(fname));
                candidates.push(dir.join(fname));
            }
            for p in candidates {
                if p.is_file()
                    && let Ok(bytes) = fs::read(&p)
                {
                    return Some(bytes);
                }
            }
            None
        }

        fn try_connect(&mut self) -> bool {
            if self.backend.is_some() {
                return true;
            }
            let Some(blob) = self.find_module_bytes() else {
                return false;
            };
            let Some(backend) = PawnBackend::open_and_load(&blob) else {
                return false;
            };
            if self.kind == PawnCpuKind::IntelMsr {
                let mut out = [0u64; 1];
                if let Some(n) = backend.execute("ioctl_read_msr", &[MSR_IA32_TEMPERATURE_TARGET], &mut out)
                    && n >= 1
                {
                    self.tj_max_msr = Some(out[0]);
                }
            }
            self.backend = Some(backend);
            true
        }

        pub fn sample_cpu_temp(&mut self) -> Option<f32> {
            if !self.try_connect() {
                return None;
            }
            let backend = self.backend.as_ref()?;
            match self.kind {
                PawnCpuKind::AmdFamily17 => {
                    let mut locked = false;
                    if let Some(mtx) = self.pci_mutex {
                        let wait = unsafe { WaitForSingleObject(mtx, 10) };
                        locked = wait == WAIT_OBJECT_0;
                    }
                    let mut out = [0u64; 1];
                    let res = backend.execute("ioctl_read_smn", &[F17H_M01H_THM_TCON_CUR_TMP], &mut out);
                    if locked && let Some(mtx) = self.pci_mutex {
                        unsafe {
                            let _ = ReleaseMutex(mtx);
                        }
                    }
                    let count = res?;
                    if count < 1 {
                        return None;
                    }
                    decode_amd_zen_cur_tmp(out[0] as u32, &self.cpu_name)
                }
                PawnCpuKind::IntelMsr => {
                    let mut out = [0u64; 1];
                    if let Some(n) = backend.execute("ioctl_read_msr", &[MSR_IA32_PACKAGE_THERM_STATUS], &mut out)
                        && n >= 1
                        && let Some(t) = decode_intel_msr_temp(out[0], self.tj_max_msr)
                    {
                        return Some(t);
                    }
                    if let Some(n) = backend.execute("ioctl_read_msr", &[MSR_IA32_THERM_STATUS], &mut out)
                        && n >= 1
                    {
                        return decode_intel_msr_temp(out[0], self.tj_max_msr);
                    }
                    None
                }
                PawnCpuKind::Unsupported => None,
            }
        }
    }

    impl Drop for PawnIoCpuReader {
        fn drop(&mut self) {
            if let Some(mtx) = self.pci_mutex.take() {
                unsafe {
                    let _ = CloseHandle(mtx);
                }
            }
        }
    }

    fn download_to_file(url: &str, dest: &Path) -> Result<(), String> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .build();
        let resp = agent
            .get(url)
            .set("User-Agent", "ZenithBar-PawnIO-Setup")
            .call()
            .map_err(|e| format!("HTTP download error ({}): {}", url, e))?;
        let mut reader = resp.into_reader();
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut file = fs::File::create(dest)
            .map_err(|e| format!("Failed creating {}: {}", dest.display(), e))?;
        let mut buf = [0u8; 16384];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("Download stream error: {}", e))?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut file, &buf[..n])
                .map_err(|e| format!("Write error: {}", e))?;
        }
        Ok(())
    }

    fn fetch_latest_github_asset_url(repo: &str, suffix: &str) -> Result<String, String> {
        let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build();
        let resp = agent
            .get(&url)
            .set("User-Agent", "ZenithBar-PawnIO-Setup")
            .set("Accept", "application/vnd.github.v3+json")
            .call()
            .map_err(|e| format!("GitHub API error for {}: {}", repo, e))?;
        let json: serde_json::Value = resp
            .into_json()
            .map_err(|e| format!("Invalid GitHub JSON for {}: {}", repo, e))?;
        select_release_asset_url(&json, suffix)
            .ok_or_else(|| format!("Asset ending in '{}' not found in {}", suffix, repo))
    }

    fn extract_zip_archive(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
        let _ = fs::create_dir_all(dest_dir);
        let tar_status = std::process::Command::new("C:\\Windows\\System32\\tar.exe")
            .arg("-xf")
            .arg(zip_path)
            .arg("-C")
            .arg(dest_dir)
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        if let Ok(st) = tar_status
            && st.success()
        {
            return Ok(());
        }

        let ps_cmd = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            zip_path.to_string_lossy().replace('\'', "''"),
            dest_dir.to_string_lossy().replace('\'', "''")
        );
        let ps_status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Failed extracting zip archive: {}", e))?;
        if ps_status.success() {
            Ok(())
        } else {
            Err("Archive extraction returned non-zero status".to_string())
        }
    }

    fn stop_and_install_sensor_service(service_exe: &Path) -> Result<(), String> {
        unsafe {
            let scm = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS)
                .map_err(|e| format!("OpenSCManagerW failed (Admin required): {:?}", e))?;
            let bin_cmd = format!("\"{}\" --sensor-service", service_exe.to_string_lossy());
            let bin_w = to_wide(&bin_cmd);
            let display_w = to_wide("ZenithBar Hardware Sensor Service");

            let svc = match OpenServiceW(scm, w!("ZenithBarSensor"), SERVICE_ALL_ACCESS) {
                Ok(existing) => {
                    let mut status = SERVICE_STATUS::default();
                    let _ = ControlService(existing, SERVICE_CONTROL_STOP, &mut status);
                    for _ in 0..20 {
                        if QueryServiceStatus(existing, &mut status).is_ok()
                            && status.dwCurrentState == SERVICE_STOPPED
                        {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(150));
                    }
                    existing
                }
                Err(_) => {
                    if GetLastError() != ERROR_SERVICE_DOES_NOT_EXIST {
                        // Still attempt CreateServiceW if OpenServiceW failed unexpectedly
                    }
                    CreateServiceW(
                        scm,
                        w!("ZenithBarSensor"),
                        PCWSTR(display_w.as_ptr()),
                        SERVICE_ALL_ACCESS,
                        SERVICE_WIN32_OWN_PROCESS,
                        SERVICE_AUTO_START,
                        SERVICE_ERROR_NORMAL,
                        PCWSTR(bin_w.as_ptr()),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .map_err(|e| {
                        let _ = CloseServiceHandle(scm);
                        format!("CreateServiceW failed: {:?}", e)
                    })?
                }
            };

            let cur_exe = std::env::current_exe()
                .map_err(|e| format!("Failed locating current executable: {}", e))?;
            if let Some(parent) = service_exe.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if cur_exe != service_exe {
                fs::copy(&cur_exe, service_exe).map_err(|e| {
                    let _ = CloseServiceHandle(svc);
                    let _ = CloseServiceHandle(scm);
                    format!("Failed copying service binary: {}", e)
                })?;
            }

            let _ = ChangeServiceConfigW(
                svc,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_AUTO_START,
                SERVICE_ERROR_NORMAL,
                PCWSTR(bin_w.as_ptr()),
                None,
                None,
                None,
                None,
                None,
                PCWSTR(display_w.as_ptr()),
            );

            if let Err(err) = StartServiceW(svc, None)
                && GetLastError() != ERROR_SERVICE_ALREADY_RUNNING
            {
                let _ = CloseServiceHandle(svc);
                let _ = CloseServiceHandle(scm);
                return Err(format!("StartServiceW failed: {:?}", err));
            }

            let _ = CloseServiceHandle(svc);
            let _ = CloseServiceHandle(scm);
            Ok(())
        }
    }

    pub fn install_pawnio_and_service() -> Result<(), String> {
        if !is_pawnio_installed() {
            let setup_url = fetch_latest_github_asset_url("namazso/PawnIO.Setup", ".exe")?;
            let setup_exe = std::env::temp_dir().join("ZenithBar_PawnIO_setup.exe");
            download_to_file(&setup_url, &setup_exe)?;
            let status = std::process::Command::new(&setup_exe)
                .args(PAWNIO_SETUP_ARGS)
                .creation_flags(CREATE_NO_WINDOW)
                .status()
                .map_err(|e| format!("Failed running PawnIO_setup.exe: {}", e))?;
            let _ = fs::remove_file(&setup_exe);
            if !status.success() {
                return Err(format!(
                    "PawnIO_setup.exe exited with status {:?}",
                    status.code()
                ));
            }
        }

        let mod_dir = pawnio_modules_dir();
        if !mod_dir.join("AMDFamily17.bin").is_file() || !mod_dir.join("IntelMSR.bin").is_file() {
            let mod_zip_url = fetch_latest_github_asset_url("namazso/PawnIO.Modules", ".zip")?;
            let zip_path = std::env::temp_dir().join("ZenithBar_PawnIO_Modules.zip");
            download_to_file(&mod_zip_url, &zip_path)?;
            extract_zip_archive(&zip_path, &mod_dir)?;
            let _ = fs::remove_file(&zip_path);
        }

        let service_exe = program_data_dir().join("ZenithBarSensor.exe");
        stop_and_install_sensor_service(&service_exe)?;
        Ok(())
    }

    pub fn run_setup_pawnio_cli(culture: &str) -> i32 {
        let s = crate::i18n::load_locale(culture);
        match install_pawnio_and_service() {
            Ok(()) => {
                let title = to_wide("ZenithBar");
                let body = to_wide(&s.pawnio_setup_ok);
                unsafe {
                    let _ = MessageBoxW(
                        None,
                        PCWSTR(body.as_ptr()),
                        PCWSTR(title.as_ptr()),
                        MB_OK | MB_ICONINFORMATION,
                    );
                }
                0
            }
            Err(err) => {
                crate::log_msg(&format!("[PawnIO Setup] Erro: {}", err));
                let title = to_wide("ZenithBar - Erro");
                let body = to_wide(&format!("{} {}", s.pawnio_setup_err, err));
                unsafe {
                    let _ = MessageBoxW(
                        None,
                        PCWSTR(body.as_ptr()),
                        PCWSTR(title.as_ptr()),
                        MB_OK | MB_ICONERROR,
                    );
                }
                1
            }
        }
    }

    struct GlobalSensorShmWriter {
        handle: HANDLE,
        view: MEMORY_MAPPED_VIEW_ADDRESS,
        seq: u64,
    }

    impl GlobalSensorShmWriter {
        fn create() -> Option<Self> {
            unsafe {
                let mut sd = PSECURITY_DESCRIPTOR::default();
                let sddl = w!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;AU)");
                if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl,
                    1, // SDDL_REVISION_1
                    &mut sd,
                    None,
                )
                .is_err()
                {
                    return None;
                }

                let sa = SECURITY_ATTRIBUTES {
                    nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: sd.0,
                    bInheritHandle: false.into(),
                };

                let handle_res = CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    Some(&sa),
                    PAGE_READWRITE,
                    0,
                    size_of::<ZenithSensorShm>() as u32,
                    w!("Global\\ZenithBar_SensorShm"),
                );

                if !sd.0.is_null() {
                    let _ = LocalFree(Some(HLOCAL(sd.0)));
                }

                let handle = handle_res.ok()?;
                if handle.is_invalid() {
                    return None;
                }

                let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, size_of::<ZenithSensorShm>());
                if view.Value.is_null() {
                    let _ = CloseHandle(handle);
                    return None;
                }

                Some(Self {
                    handle,
                    view,
                    seq: 0,
                })
            }
        }

        fn write(&mut self, cpu_temp: Option<f32>) {
            let ptr = self.view.Value as *mut ZenithSensorShm;
            if ptr.is_null() {
                return;
            }
            self.seq = self.seq.wrapping_add(1);
            let odd_seq = self.seq | 1;
            let even_seq = odd_seq.wrapping_add(1);
            self.seq = even_seq;

            let (val, valid) = match cpu_temp {
                Some(t) => (t, 1),
                None => (0.0, 0),
            };

            unsafe {
                std::ptr::write_volatile(&mut (*ptr).magic, ZENITH_SHM_MAGIC);
                std::ptr::write_volatile(&mut (*ptr).version, ZENITH_SHM_VERSION);
                std::ptr::write_volatile(&mut (*ptr).sequence, odd_seq);
                std::sync::atomic::fence(Ordering::Release);
                std::ptr::write_volatile(&mut (*ptr).updated_unix_ms, now_unix_ms());
                std::ptr::write_volatile(&mut (*ptr).cpu_temp_c, val);
                std::ptr::write_volatile(&mut (*ptr).cpu_temp_valid, valid);
                std::sync::atomic::fence(Ordering::Release);
                std::ptr::write_volatile(&mut (*ptr).sequence, even_seq);
            }
        }
    }

    impl Drop for GlobalSensorShmWriter {
        fn drop(&mut self) {
            unsafe {
                if !self.view.Value.is_null() {
                    let _ = UnmapViewOfFile(self.view);
                }
                if !self.handle.is_invalid() {
                    let _ = CloseHandle(self.handle);
                }
            }
        }
    }

    fn report_service_status(current_state: u32, exit_code: u32, wait_hint: u32) {
        unsafe {
            let handle = SERVICE_STATUS_HANDLE_GLOBAL;
            if handle.0.is_null() {
                return;
            }
            let controls_accepted = if current_state == SERVICE_START_PENDING.0 {
                0
            } else {
                0x00000001 | 0x00000004 // SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
            };
            let status = SERVICE_STATUS {
                dwServiceType: SERVICE_WIN32_OWN_PROCESS,
                dwCurrentState: windows::Win32::System::Services::SERVICE_STATUS_CURRENT_STATE(
                    current_state,
                ),
                dwControlsAccepted: controls_accepted,
                dwWin32ExitCode: exit_code,
                dwServiceSpecificExitCode: 0,
                dwCheckPoint: 0,
                dwWaitHint: wait_hint,
            };
            let _ = SetServiceStatus(handle, &status);
        }
    }

    unsafe extern "system" fn service_ctrl_handler(
        control: u32,
        _event_type: u32,
        _event_data: *mut c_void,
        _context: *mut c_void,
    ) -> u32 {
        match control {
            SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
                report_service_status(SERVICE_STOP_PENDING.0, 0, 2000);
                SERVICE_STOP_REQUESTED.store(true, Ordering::SeqCst);
                0
            }
            SERVICE_CONTROL_INTERROGATE => 0,
            _ => 0,
        }
    }

    unsafe extern "system" fn service_main(_argc: u32, _argv: *mut windows::core::PWSTR) {
        unsafe {
            let Ok(status_handle) = RegisterServiceCtrlHandlerExW(
                w!("ZenithBarSensor"),
                Some(service_ctrl_handler),
                None,
            ) else {
                return;
            };
            SERVICE_STATUS_HANDLE_GLOBAL = status_handle;
            report_service_status(SERVICE_START_PENDING.0, 0, 3000);

            let mut reader = PawnIoCpuReader::new();
            let mut shm = GlobalSensorShmWriter::create();

            report_service_status(SERVICE_RUNNING.0, 0, 0);

            while !SERVICE_STOP_REQUESTED.load(Ordering::SeqCst) {
                let temp = reader.sample_cpu_temp();
                if let Some(writer) = shm.as_mut() {
                    writer.write(temp);
                }
                for _ in 0..10 {
                    if SERVICE_STOP_REQUESTED.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }

            drop(shm);
            report_service_status(SERVICE_STOPPED.0, 0, 0);
        }
    }

    pub fn run_sensor_service_entry() -> i32 {
        unsafe {
            let mut svc_name = to_wide("ZenithBarSensor");
            let table = [
                SERVICE_TABLE_ENTRYW {
                    lpServiceName: windows::core::PWSTR(svc_name.as_mut_ptr()),
                    lpServiceProc: Some(service_main),
                },
                SERVICE_TABLE_ENTRYW::default(),
            ];
            if StartServiceCtrlDispatcherW(table.as_ptr()).is_ok() {
                0
            } else {
                1
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use win_impl::{
    is_sensor_service_running, launch_elevated_pawnio_setup, now_unix_ms, run_sensor_service_entry,
    run_setup_pawnio_cli, PawnIoCpuReader,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn shm_as_bytes(s: &ZenithSensorShm) -> &[u8] {
        unsafe { std::slice::from_raw_parts(s as *const ZenithSensorShm as *const u8, size_of::<ZenithSensorShm>()) }
    }

    #[test]
    fn parse_zenith_shm_bytes_validates_magic_sequence_staleness_and_range() {
        let now = 1_700_000_000_000u64;
        let valid = ZenithSensorShm {
            magic: ZENITH_SHM_MAGIC,
            version: ZENITH_SHM_VERSION,
            sequence: 4,
            updated_unix_ms: now - 500,
            cpu_temp_c: 54.25,
            cpu_temp_valid: 1,
        };
        assert_eq!(parse_zenith_shm_bytes(shm_as_bytes(&valid), now), Some(54.25));

        // Odd sequence means write in progress
        let mut writing = valid;
        writing.sequence = 5;
        assert_eq!(parse_zenith_shm_bytes(shm_as_bytes(&writing), now), None);

        // Stale sample (> 15s) is rejected
        let mut stale = valid;
        stale.updated_unix_ms = now - 20_000;
        assert_eq!(parse_zenith_shm_bytes(shm_as_bytes(&stale), now), None);

        // Wrong magic is rejected
        let mut bad = valid;
        bad.magic = 0x1234_5678;
        assert_eq!(parse_zenith_shm_bytes(shm_as_bytes(&bad), now), None);
    }

    #[test]
    fn decode_amd_zen_cur_tmp_handles_normal_and_offset_ranges() {
        // Normal range (temp_offset_flag = 0): 52.5 C -> raw bits [31:21] = 420 (420 * 125 = 52500 mC)
        let raw_normal = 420u32 << 21;
        let t = decode_amd_zen_cur_tmp(raw_normal, "AMD Ryzen 7 5700G with Radeon Graphics").unwrap();
        assert!((t - 52.5).abs() < 0.01);

        // Extended range (bit 19 set -> -49 C): raw temp = 100.0 C (800 << 21) -> 51.0 C
        let raw_ext = (800u32 << 21) | 0x80000;
        let t_ext = decode_amd_zen_cur_tmp(raw_ext, "AMD Ryzen 7 5700G").unwrap();
        assert!((t_ext - 51.0).abs() < 0.01);

        // Ryzen 1700X -20 C Tctl offset: 75.0 C raw -> 55.0 C Tdie
        let raw_1700x = 600u32 << 21;
        let t_1700x = decode_amd_zen_cur_tmp(raw_1700x, "AMD Ryzen 7 1700X").unwrap();
        assert!((t_1700x - 55.0).abs() < 0.01);
    }

    #[test]
    fn decode_intel_msr_temp_reads_valid_bit_and_tjmax() {
        // TjMax = 100 C (0x64 << 16), delta = 38 C (0x26 << 16), valid bit 31 = 1 -> 62 C
        let tj_msr = 0x64u64 << 16;
        let therm_status = (1u64 << 31) | (38u64 << 16);
        assert_eq!(decode_intel_msr_temp(therm_status, Some(tj_msr)), Some(62.0));

        // Invalid bit 31 = 0 -> None
        assert_eq!(decode_intel_msr_temp(38u64 << 16, Some(tj_msr)), None);
    }

    #[test]
    fn classify_cpu_arch_and_release_asset_matching() {
        assert_eq!(classify_cpu_arch_kind("AuthenticAMD", 0x19), PawnCpuKind::AmdFamily17);
        assert_eq!(classify_cpu_arch_kind("GenuineIntel", 0x06), PawnCpuKind::IntelMsr);
        assert_eq!(classify_cpu_arch_kind("AuthenticAMD", 0x15), PawnCpuKind::Unsupported);

        let payload = serde_json::json!({
            "assets": [
                { "name": "checksums.txt", "browser_download_url": "https://example.com/checksums.txt" },
                { "name": "release_0_2_12.zip", "browser_download_url": "https://example.com/release.zip" }
            ]
        });
        assert_eq!(
            select_release_asset_url(&payload, ".zip"),
            Some("https://example.com/release.zip".to_string())
        );
    }

    #[test]
    fn pawnio_setup_uses_documented_dash_install_silent_flags() {
        assert_eq!(PAWNIO_SETUP_ARGS, ["-install", "-silent"]);
        assert!(PAWNIO_SETUP_ARGS.iter().all(|arg| arg.starts_with('-')));
    }
}
