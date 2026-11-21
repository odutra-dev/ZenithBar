use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
use std::mem::size_of;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HMODULE, LUID};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIAdapter3, IDXGIFactory1, DXGI_ADAPTER_DESC1,
    DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA,
};

pub struct WinGpuSample {
    pub name: String,
    pub usage: Option<f32>,
    pub clock_ghz: Option<f32>,
    pub vram_used_gb: f64,
    pub vram_total_gb: f64,
    pub vram_percent: Option<f32>,
    pub temperature_c: Option<f32>,
    pub fan_percent: Option<f32>,
}

struct SelectedAdapter {
    name: String,
    vendor_id: u32,
    luid: LUID,
    vram_used_gb: f64,
    vram_total_gb: f64,
    vram_percent: Option<f32>,
}

pub struct WindowsGpuMonitor {
    pdh: Option<PdhGpuQuery>,
    nvml: Option<NvmlApi>,
    nvml_checked: bool,
    nvapi: Option<NvApi>,
    nvapi_checked: bool,
    adl: Option<AdlApi>,
    adl_checked: bool,
    logged_dxgi: bool,
    logged_nv_sample: bool,
    logged_adl_sample: bool,
}

impl WindowsGpuMonitor {
    pub fn new() -> Self {
        let pdh = PdhGpuQuery::new();
        if pdh.is_none() {
            crate::log_msg("[GPU Win] Aviso: falha ao inicializar contadores PDH.");
        }
        Self {
            pdh,
            nvml: None,
            nvml_checked: false,
            nvapi: None,
            nvapi_checked: false,
            adl: None,
            adl_checked: false,
            logged_dxgi: false,
            logged_nv_sample: false,
            logged_adl_sample: false,
        }
    }

    pub fn sample(&mut self) -> Option<WinGpuSample> {
        let adapter = query_dxgi_primary_adapter()?;
        if !self.logged_dxgi {
            self.logged_dxgi = true;
            crate::log_msg(&format!(
                "[GPU Win] DXGI selecionou '{}' (VendorID=0x{:04X}, VRAM={:.2} GB)",
                adapter.name, adapter.vendor_id, adapter.vram_total_gb
            ));
        }

        let mut usage: Option<f32> = None;
        let mut clock_ghz: Option<f32> = None;
        let mut vram_used_gb = adapter.vram_used_gb;
        let mut vram_total_gb = adapter.vram_total_gb;
        let mut vram_percent = adapter.vram_percent;
        let mut temperature_c: Option<f32> = None;
        let mut fan_percent: Option<f32> = None;

        // 1. Universal Windows 10/11 PDH counters (NVIDIA / AMD / Intel)
        if let Some(pdh) = self.pdh.as_mut() {
            let (pdh_usage, pdh_vram_bytes) = pdh.sample_for_luid(adapter.luid);
            if pdh_usage.is_some() {
                usage = pdh_usage;
            }
            if vram_percent.is_none() && vram_total_gb > 0.0 {
                if let Some(used_bytes) = pdh_vram_bytes {
                    let used_gb = used_bytes / (1024.0 * 1024.0 * 1024.0);
                    vram_used_gb = used_gb;
                    vram_percent = Some((used_gb / vram_total_gb).clamp(0.0, 1.0) as f32);
                }
            }
        }

        // 2. Vendor-specific dynamic libraries (zero external processes, no admin required)
        if adapter.vendor_id == 0x10DE {
            if !self.nvml_checked {
                self.nvml_checked = true;
                self.nvml = NvmlApi::load();
                crate::log_msg(&format!(
                    "[GPU Win] NVML (nvml.dll): {}",
                    if self.nvml.is_some() { "carregada" } else { "indisponível" }
                ));
            }
            let mut nvml_diag = String::new();
            if let Some(nvml) = self.nvml.as_ref() {
                let nv = nvml.sample(&adapter.name);
                nvml_diag = nv.diag;
                if nv.usage.is_some() {
                    usage = nv.usage;
                }
                if nv.clock_ghz.is_some() {
                    clock_ghz = nv.clock_ghz;
                }
                if nv.vram_percent.is_some() {
                    vram_used_gb = nv.vram_used_gb;
                    vram_total_gb = nv.vram_total_gb;
                    vram_percent = nv.vram_percent;
                }
                if nv.temperature_c.is_some() {
                    temperature_c = nv.temperature_c;
                }
                if nv.fan_percent.is_some() {
                    fan_percent = nv.fan_percent;
                }
            }

            if temperature_c.is_none() || clock_ghz.is_none() || usage.is_none() {
                if !self.nvapi_checked {
                    self.nvapi_checked = true;
                    self.nvapi = NvApi::load();
                    crate::log_msg(&format!(
                        "[GPU Win] NvAPI (nvapi64.dll): {}",
                        if self.nvapi.is_some() { "carregada" } else { "indisponível" }
                    ));
                }
                if let Some(nvapi) = self.nvapi.as_ref() {
                    let nv2 = nvapi.sample(&adapter.name);
                    if usage.is_none() {
                        usage = nv2.usage;
                    }
                    if clock_ghz.is_none() {
                        clock_ghz = nv2.clock_ghz;
                    }
                    if temperature_c.is_none() {
                        temperature_c = nv2.temperature_c;
                    }
                    if !self.logged_nv_sample {
                        nvml_diag = format!("{} | {}", nvml_diag, nv2.diag);
                    }
                }
            }

            if !self.logged_nv_sample {
                self.logged_nv_sample = true;
                crate::log_msg(&format!("[GPU Win] NVIDIA amostra inicial: {}", nvml_diag));
            }
        } else if adapter.vendor_id == 0x1002 {
            if !self.adl_checked {
                self.adl_checked = true;
                self.adl = AdlApi::load();
                crate::log_msg(&format!(
                    "[GPU Win] AMD ADL (atiadlxx.dll): {}",
                    if self.adl.is_some() { "carregada" } else { "indisponível" }
                ));
            }
            if let Some(adl) = self.adl.as_ref() {
                let amd = adl.sample(&adapter.name);
                if !self.logged_adl_sample {
                    self.logged_adl_sample = true;
                    crate::log_msg(&format!("[GPU Win] ADL amostra inicial: {}", amd.diag));
                }
                if amd.usage.is_some() {
                    usage = amd.usage;
                }
                if amd.clock_ghz.is_some() {
                    clock_ghz = amd.clock_ghz;
                }
                if amd.temperature_c.is_some() {
                    temperature_c = amd.temperature_c;
                }
                if amd.fan_percent.is_some() {
                    fan_percent = amd.fan_percent;
                }
            }
        }

        Some(WinGpuSample {
            name: adapter.name,
            usage,
            clock_ghz,
            vram_used_gb,
            vram_total_gb,
            vram_percent,
            temperature_c,
            fan_percent,
        })
    }
}

fn utf16_array_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len]).trim().to_string()
}

fn query_dxgi_primary_adapter() -> Option<SelectedAdapter> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut best: Option<(bool, usize, SelectedAdapter)> = None;

        for idx in 0..16u32 {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(idx) {
                Ok(a) => a,
                Err(_) => break,
            };
            let desc: DXGI_ADAPTER_DESC1 = match adapter.GetDesc1() {
                Ok(d) => d,
                Err(_) => continue,
            };

            let name = utf16_array_to_string(&desc.Description);
            if name.is_empty() {
                continue;
            }

            let is_software = (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0
                || desc.VendorId == 0x1414;

            let mut mem_info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            let has_mem_info = adapter
                .cast::<IDXGIAdapter3>()
                .ok()
                .and_then(|a3| {
                    a3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut mem_info)
                        .ok()
                })
                .is_some();

            let dedicated_bytes = desc.DedicatedVideoMemory as f64;
            let (vram_used_gb, vram_total_gb, vram_percent) = if dedicated_bytes > 0.0 {
                let total = dedicated_bytes / (1024.0 * 1024.0 * 1024.0);
                if has_mem_info {
                    let used = mem_info.CurrentUsage as f64 / (1024.0 * 1024.0 * 1024.0);
                    let pct = (mem_info.CurrentUsage as f64 / dedicated_bytes).clamp(0.0, 1.0) as f32;
                    (used, total, Some(pct))
                } else {
                    (0.0, total, None)
                }
            } else if has_mem_info && mem_info.Budget > 0 {
                let total = mem_info.Budget as f64 / (1024.0 * 1024.0 * 1024.0);
                let used = mem_info.CurrentUsage as f64 / (1024.0 * 1024.0 * 1024.0);
                let pct = (mem_info.CurrentUsage as f64 / mem_info.Budget as f64).clamp(0.0, 1.0) as f32;
                (used, total, Some(pct))
            } else {
                (0.0, 0.0, None)
            };

            let candidate = SelectedAdapter {
                name,
                vendor_id: desc.VendorId,
                luid: desc.AdapterLuid,
                vram_used_gb,
                vram_total_gb,
                vram_percent,
            };

            let replace = match &best {
                None => true,
                Some((best_is_hw, best_vram, _)) => {
                    let cand_is_hw = !is_software;
                    (cand_is_hw && !*best_is_hw)
                        || (cand_is_hw == *best_is_hw && desc.DedicatedVideoMemory > *best_vram)
                }
            };

            if replace {
                best = Some((!is_software, desc.DedicatedVideoMemory, candidate));
            }
        }

        best.map(|(_, _, a)| a)
    }
}

struct PdhGpuQuery {
    hquery: PDH_HQUERY,
    hcounter_engine: PDH_HCOUNTER,
    hcounter_mem: PDH_HCOUNTER,
    initialized: bool,
}

impl PdhGpuQuery {
    fn new() -> Option<Self> {
        unsafe {
            let mut hquery = PDH_HQUERY::default();
            if PdhOpenQueryW(None, 0, &mut hquery) != 0 || hquery.is_invalid() {
                return None;
            }

            let mut hcounter_engine = PDH_HCOUNTER::default();
            let eng_ok = PdhAddEnglishCounterW(
                hquery,
                w!("\\GPU Engine(*)\\Utilization Percentage"),
                0,
                &mut hcounter_engine,
            ) == 0;

            let mut hcounter_mem = PDH_HCOUNTER::default();
            let mem_ok = PdhAddEnglishCounterW(
                hquery,
                w!("\\GPU Adapter Memory(*)\\Dedicated Usage"),
                0,
                &mut hcounter_mem,
            ) == 0;

            if !eng_ok && !mem_ok {
                let _ = PdhCloseQuery(hquery);
                return None;
            }

            let _ = PdhCollectQueryData(hquery);
            Some(Self {
                hquery,
                hcounter_engine: if eng_ok { hcounter_engine } else { PDH_HCOUNTER::default() },
                hcounter_mem: if mem_ok { hcounter_mem } else { PDH_HCOUNTER::default() },
                initialized: false,
            })
        }
    }

    fn sample_for_luid(&mut self, luid: LUID) -> (Option<f32>, Option<f64>) {
        unsafe {
            if PdhCollectQueryData(self.hquery) != 0 {
                return (None, None);
            }

            // Rate counters need at least two collections to produce meaningful deltas.
            if !self.initialized {
                self.initialized = true;
                return (None, None);
            }

            let luid_tag = format!("luid_0x{:08x}_0x{:08x}", luid.HighPart as u32, luid.LowPart);
            let mut usage_pct: Option<f32> = None;
            let mut vram_used_bytes: Option<f64> = None;

            if !self.hcounter_engine.is_invalid() {
                if let Some(items) = read_pdh_double_array(self.hcounter_engine) {
                    let mut sum_3d = 0.0f64;
                    let mut max_any = 0.0f64;
                    let mut matched = false;
                    for (name, val) in items {
                        let lower = name.to_ascii_lowercase();
                        if lower.contains(&luid_tag) {
                            matched = true;
                            if lower.contains("engtype_3d") {
                                sum_3d += val.max(0.0);
                            }
                            if val > max_any {
                                max_any = val;
                            }
                        }
                    }
                    if matched {
                        let pct = sum_3d.max(max_any).clamp(0.0, 100.0) / 100.0;
                        usage_pct = Some(pct as f32);
                    }
                }
            }

            if !self.hcounter_mem.is_invalid() {
                if let Some(items) = read_pdh_double_array(self.hcounter_mem) {
                    for (name, val) in items {
                        if name.to_ascii_lowercase().contains(&luid_tag) && val >= 0.0 {
                            vram_used_bytes = Some(val);
                            break;
                        }
                    }
                }
            }

            (usage_pct, vram_used_bytes)
        }
    }
}

impl Drop for PdhGpuQuery {
    fn drop(&mut self) {
        unsafe {
            if !self.hquery.is_invalid() {
                let _ = PdhCloseQuery(self.hquery);
            }
        }
    }
}

unsafe fn read_pdh_double_array(hcounter: PDH_HCOUNTER) -> Option<Vec<(String, f64)>> {
    unsafe {
        let mut buf_size = 0u32;
        let mut item_count = 0u32;
        let status = PdhGetFormattedCounterArrayW(
            hcounter,
            PDH_FMT_DOUBLE,
            &mut buf_size,
            &mut item_count,
            None,
        );
        if status != PDH_MORE_DATA && status != 0 {
            return None;
        }
        if buf_size == 0 {
            return None;
        }

        let elem_count = (buf_size as usize).div_ceil(size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
        let mut buf = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); elem_count];
        let status = PdhGetFormattedCounterArrayW(
            hcounter,
            PDH_FMT_DOUBLE,
            &mut buf_size,
            &mut item_count,
            Some(buf.as_mut_ptr()),
        );
        if status != 0 {
            return None;
        }

        let count = (item_count as usize).min(buf.len());
        let mut results = Vec::with_capacity(count);
        for item in &buf[..count] {
            if item.szName.is_null() || item.FmtValue.CStatus > 1 {
                continue;
            }
            let name = item.szName.to_string().unwrap_or_default();
            let val = item.FmtValue.Anonymous.doubleValue;
            results.push((name, val));
        }
        Some(results)
    }
}

// ============================================================================
// NVIDIA NVML Dynamic Loader (nvml.dll)
// ============================================================================

#[repr(C)]
struct NvmlUtilization {
    gpu: c_uint,
    memory: c_uint,
}

#[repr(C)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

struct NvmlSample {
    usage: Option<f32>,
    clock_ghz: Option<f32>,
    vram_used_gb: f64,
    vram_total_gb: f64,
    vram_percent: Option<f32>,
    temperature_c: Option<f32>,
    fan_percent: Option<f32>,
    diag: String,
}

type NvmlDevice = *mut c_void;

struct NvmlApi {
    module: HMODULE,
    shutdown: unsafe extern "C" fn() -> c_int,
    device_get_count: unsafe extern "C" fn(*mut c_uint) -> c_int,
    device_get_handle: unsafe extern "C" fn(c_uint, *mut NvmlDevice) -> c_int,
    device_get_name: Option<unsafe extern "C" fn(NvmlDevice, *mut c_char, c_uint) -> c_int>,
    device_get_utilization: Option<unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> c_int>,
    device_get_clock: Option<unsafe extern "C" fn(NvmlDevice, c_int, *mut c_uint) -> c_int>,
    device_get_memory: Option<unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> c_int>,
    device_get_temp: Option<unsafe extern "C" fn(NvmlDevice, c_int, *mut c_uint) -> c_int>,
    device_get_fan: Option<unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_int>,
    device_get_fan_v2: Option<unsafe extern "C" fn(NvmlDevice, c_uint, *mut c_uint) -> c_int>,
}

pub fn find_nvidia_driverstore_file(filename: &str) -> Option<String> {
    let repo = std::path::Path::new("C:\\Windows\\System32\\DriverStore\\FileRepository");
    let entries = std::fs::read_dir(repo).ok()?;
    for entry in entries.flatten() {
        let dir_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if dir_name.starts_with("nv") {
            let candidate = entry.path().join(filename);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

unsafe fn load_library_path_str(path: &str) -> Option<HMODULE> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let module = LoadLibraryW(PCWSTR(wide.as_ptr())).ok()?;
        if module.is_invalid() {
            None
        } else {
            Some(module)
        }
    }
}

unsafe fn load_library_candidates(paths: &[PCWSTR]) -> Option<HMODULE> {
    unsafe {
        for &path in paths {
            if let Ok(module) = LoadLibraryW(path) {
                if !module.is_invalid() {
                    return Some(module);
                }
            }
        }
        None
    }
}

unsafe fn resolve_sym<T>(module: HMODULE, name: &[u8]) -> Option<T> {
    unsafe {
        let proc = GetProcAddress(module, windows::core::PCSTR(name.as_ptr()))?;
        Some(std::mem::transmute_copy(&proc))
    }
}

impl NvmlApi {
    fn load() -> Option<Self> {
        unsafe {
            let module = load_library_candidates(&[
                w!("nvml.dll"),
                w!("C:\\Windows\\System32\\nvml.dll"),
                w!("C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvml.dll"),
            ])
            .or_else(|| {
                find_nvidia_driverstore_file("nvml.dll")
                    .and_then(|p| load_library_path_str(&p))
            })?;

            let init: unsafe extern "C" fn() -> c_int =
                match resolve_sym(module, b"nvmlInit_v2\0")
                    .or_else(|| resolve_sym(module, b"nvmlInit\0"))
                {
                    Some(f) => f,
                    None => {
                        let _ = FreeLibrary(module);
                        return None;
                    }
                };

            let shutdown: unsafe extern "C" fn() -> c_int =
                match resolve_sym(module, b"nvmlShutdown\0") {
                    Some(f) => f,
                    None => {
                        let _ = FreeLibrary(module);
                        return None;
                    }
                };

            let init_rc = init();
            if init_rc != 0 {
                crate::log_msg(&format!("[GPU Win] nvmlInit falhou: rc={}", init_rc));
                let _ = FreeLibrary(module);
                return None;
            }

            let Some(device_get_count) = resolve_sym(module, b"nvmlDeviceGetCount_v2\0")
                .or_else(|| resolve_sym(module, b"nvmlDeviceGetCount\0"))
            else {
                shutdown();
                let _ = FreeLibrary(module);
                return None;
            };
            let Some(device_get_handle) = resolve_sym(module, b"nvmlDeviceGetHandleByIndex_v2\0")
                .or_else(|| resolve_sym(module, b"nvmlDeviceGetHandleByIndex\0"))
            else {
                shutdown();
                let _ = FreeLibrary(module);
                return None;
            };

            let device_get_name = resolve_sym(module, b"nvmlDeviceGetName\0");
            let device_get_utilization = resolve_sym(module, b"nvmlDeviceGetUtilizationRates\0");
            let device_get_clock = resolve_sym(module, b"nvmlDeviceGetClockInfo\0");
            let device_get_memory = resolve_sym(module, b"nvmlDeviceGetMemoryInfo\0");
            let device_get_temp = resolve_sym(module, b"nvmlDeviceGetTemperature\0");
            let device_get_fan = resolve_sym(module, b"nvmlDeviceGetFanSpeed\0");
            let device_get_fan_v2 = resolve_sym(module, b"nvmlDeviceGetFanSpeed_v2\0");

            Some(Self {
                module,
                shutdown,
                device_get_count,
                device_get_handle,
                device_get_name,
                device_get_utilization,
                device_get_clock,
                device_get_memory,
                device_get_temp,
                device_get_fan,
                device_get_fan_v2,
            })
        }
    }

    fn sample(&self, expected_name: &str) -> NvmlSample {
        unsafe {
            let mut count = 0u32;
            let count_rc = (self.device_get_count)(&mut count);
            if count_rc != 0 || count == 0 {
                return NvmlSample {
                    usage: None,
                    clock_ghz: None,
                    vram_used_gb: 0.0,
                    vram_total_gb: 0.0,
                    vram_percent: None,
                    temperature_c: None,
                    fan_percent: None,
                    diag: format!("nvml count_rc={} count={}", count_rc, count),
                };
            }

            let mut chosen: Option<(u32, NvmlDevice)> = None;
            let expected_lower = expected_name.to_ascii_lowercase();
            for i in 0..count {
                let mut dev: NvmlDevice = std::ptr::null_mut();
                if (self.device_get_handle)(i, &mut dev) != 0 || dev.is_null() {
                    continue;
                }
                if chosen.is_none() {
                    chosen = Some((i, dev));
                }
                if let Some(get_name) = self.device_get_name {
                    let mut name_buf = [0 as c_char; 96];
                    if get_name(dev, name_buf.as_mut_ptr(), name_buf.len() as u32) == 0 {
                        let nv_name = CStr::from_ptr(name_buf.as_ptr())
                            .to_string_lossy()
                            .to_ascii_lowercase();
                        if expected_lower.contains(&nv_name) || nv_name.contains(&expected_lower) {
                            chosen = Some((i, dev));
                            break;
                        }
                    }
                }
            }

            let Some((dev_idx, dev)) = chosen else {
                return NvmlSample {
                    usage: None,
                    clock_ghz: None,
                    vram_used_gb: 0.0,
                    vram_total_gb: 0.0,
                    vram_percent: None,
                    temperature_c: None,
                    fan_percent: None,
                    diag: format!("nvml sem handle valido entre {} GPUs", count),
                };
            };

            let mut util = NvmlUtilization { gpu: 0, memory: 0 };
            let mut util_rc = -1;
            let usage = self.device_get_utilization.and_then(|get_util| {
                util_rc = get_util(dev, &mut util);
                if util_rc == 0 {
                    Some((util.gpu as f32 / 100.0).clamp(0.0, 1.0))
                } else {
                    None
                }
            });

            let mut clock_rc = -1;
            let clock_ghz = self.device_get_clock.and_then(|get_clk| {
                // 0 = NVML_CLOCK_GRAPHICS, 1 = NVML_CLOCK_SM
                for clk_type in [0, 1] {
                    let mut clock_mhz = 0u32;
                    let rc = get_clk(dev, clk_type, &mut clock_mhz);
                    if clk_type == 0 {
                        clock_rc = rc;
                    }
                    if rc == 0 && clock_mhz > 0 {
                        return Some(clock_mhz as f32 / 1000.0);
                    }
                }
                None
            });

            let mut mem = NvmlMemory {
                total: 0,
                free: 0,
                used: 0,
            };
            let (vram_used_gb, vram_total_gb, vram_percent) = self
                .device_get_memory
                .and_then(|get_mem| {
                    if get_mem(dev, &mut mem) == 0 && mem.total > 0 {
                        let gib = 1024.0 * 1024.0 * 1024.0;
                        Some((
                            mem.used as f64 / gib,
                            mem.total as f64 / gib,
                            Some((mem.used as f64 / mem.total as f64).clamp(0.0, 1.0) as f32),
                        ))
                    } else {
                        None
                    }
                })
                .unwrap_or((0.0, 0.0, None));

            let mut temp_rc = -1;
            let temperature_c = self.device_get_temp.and_then(|get_temp| {
                let mut temp_c = 0u32;
                temp_rc = get_temp(dev, 0, &mut temp_c);
                if temp_rc == 0 && temp_c > 0 && temp_c < 130 {
                    Some(temp_c as f32)
                } else {
                    None
                }
            });

            let mut fan_rc = -1;
            let fan_percent = self
                .device_get_fan_v2
                .and_then(|get_fan_v2| {
                    let mut fan = 0u32;
                    let rc = get_fan_v2(dev, 0, &mut fan);
                    fan_rc = rc;
                    if rc == 0 && fan <= 100 {
                        Some(fan as f32 / 100.0)
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    self.device_get_fan.and_then(|get_fan| {
                        let mut fan = 0u32;
                        let rc = get_fan(dev, &mut fan);
                        fan_rc = rc;
                        if rc == 0 && fan <= 100 {
                            Some(fan as f32 / 100.0)
                        } else {
                            None
                        }
                    })
                });

            NvmlSample {
                usage,
                clock_ghz,
                vram_used_gb,
                vram_total_gb,
                vram_percent,
                temperature_c,
                fan_percent,
                diag: format!(
                    "nvml[idx{}] util_rc={} clk_rc={} temp_rc={} fan_rc={}",
                    dev_idx, util_rc, clock_rc, temp_rc, fan_rc
                ),
            }
        }
    }
}

impl Drop for NvmlApi {
    fn drop(&mut self) {
        unsafe {
            (self.shutdown)();
            let _ = FreeLibrary(self.module);
        }
    }
}

// ============================================================================
// NVIDIA NvAPI Fallback Loader (nvapi64.dll)
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvThermalSensor {
    controller: c_int,
    default_min_temp: c_int,
    default_max_temp: c_int,
    current_temp: c_int,
    target: c_int,
}

#[repr(C)]
#[derive(Default)]
struct NvThermalSettings {
    version: c_uint,
    count: c_uint,
    sensor: [NvThermalSensor; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvDynamicPstateUtil {
    b_is_present: c_uint,
    percentage: c_uint,
}

#[repr(C)]
#[derive(Default)]
struct NvDynamicPstatesInfoEx {
    version: c_uint,
    flags: c_uint,
    utilization: [NvDynamicPstateUtil; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvClockFrequencyDomain {
    b_is_present: c_uint,
    frequency: c_uint, // kHz
}

#[repr(C)]
struct NvClockFrequencies {
    version: c_uint,
    clock_type: c_uint,
    domain: [NvClockFrequencyDomain; 32],
}

impl Default for NvClockFrequencies {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

struct NvApiSample {
    usage: Option<f32>,
    clock_ghz: Option<f32>,
    temperature_c: Option<f32>,
    diag: String,
}

struct NvApi {
    module: HMODULE,
    unload: Option<unsafe extern "C" fn() -> c_int>,
    enum_gpus: unsafe extern "C" fn(*mut [*mut c_void; 64], *mut c_uint) -> c_int,
    get_full_name: Option<unsafe extern "C" fn(*mut c_void, *mut c_char) -> c_int>,
    get_thermal: Option<unsafe extern "C" fn(*mut c_void, c_uint, *mut NvThermalSettings) -> c_int>,
    get_pstates: Option<unsafe extern "C" fn(*mut c_void, *mut NvDynamicPstatesInfoEx) -> c_int>,
    get_clocks: Option<unsafe extern "C" fn(*mut c_void, *mut NvClockFrequencies) -> c_int>,
}

impl NvApi {
    fn load() -> Option<Self> {
        unsafe {
            let module = load_library_candidates(&[
                w!("nvapi64.dll"),
                w!("C:\\Windows\\System32\\nvapi64.dll"),
            ])?;

            let Some(query_iface): Option<unsafe extern "C" fn(c_uint) -> *const c_void> =
                resolve_sym(module, b"nvapi_QueryInterface\0")
            else {
                let _ = FreeLibrary(module);
                return None;
            };

            unsafe fn q<T>(
                qi: unsafe extern "C" fn(c_uint) -> *const c_void,
                id: c_uint,
            ) -> Option<T> {
                let ptr = unsafe { qi(id) };
                if ptr.is_null() {
                    None
                } else {
                    Some(unsafe { std::mem::transmute_copy(&ptr) })
                }
            }

            let Some(init): Option<unsafe extern "C" fn() -> c_int> = q(query_iface, 0x0150E828)
            else {
                let _ = FreeLibrary(module);
                return None;
            };
            if init() != 0 {
                let _ = FreeLibrary(module);
                return None;
            }

            let unload: Option<unsafe extern "C" fn() -> c_int> = q(query_iface, 0xD22BDD7E);
            let Some(enum_gpus) = q(query_iface, 0xE5AC921F) else {
                if let Some(u) = unload {
                    u();
                }
                let _ = FreeLibrary(module);
                return None;
            };

            Some(Self {
                module,
                unload,
                enum_gpus,
                get_full_name: q(query_iface, 0xCEEE8E9F),
                get_thermal: q(query_iface, 0xE3640A56),
                get_pstates: q(query_iface, 0x60DED2ED),
                get_clocks: q(query_iface, 0xDCB616C3),
            })
        }
    }

    fn sample(&self, expected_name: &str) -> NvApiSample {
        unsafe {
            let mut handles = [std::ptr::null_mut(); 64];
            let mut count = 0u32;
            let enum_rc = (self.enum_gpus)(&mut handles, &mut count);
            if enum_rc != 0 || count == 0 {
                return NvApiSample {
                    usage: None,
                    clock_ghz: None,
                    temperature_c: None,
                    diag: format!("nvapi enum_rc={} count={}", enum_rc, count),
                };
            }

            let expected_lower = expected_name.to_ascii_lowercase();
            let mut chosen: Option<*mut c_void> = None;
            for &h in handles.iter().take((count as usize).min(64)) {
                if h.is_null() {
                    continue;
                }
                if chosen.is_none() {
                    chosen = Some(h);
                }
                if let Some(get_name) = self.get_full_name {
                    let mut buf = [0 as c_char; 64];
                    if get_name(h, buf.as_mut_ptr()) == 0 {
                        let name = CStr::from_ptr(buf.as_ptr())
                            .to_string_lossy()
                            .to_ascii_lowercase();
                        if expected_lower.contains(&name) || name.contains(&expected_lower) {
                            chosen = Some(h);
                            break;
                        }
                    }
                }
            }

            let Some(gpu) = chosen else {
                return NvApiSample {
                    usage: None,
                    clock_ghz: None,
                    temperature_c: None,
                    diag: "nvapi sem handle".to_string(),
                };
            };

            let mut therm_rc = -1;
            let temperature_c = self.get_thermal.and_then(|get_therm| {
                let mut ts = NvThermalSettings {
                    version: (size_of::<NvThermalSettings>() as c_uint) | (1 << 16),
                    ..Default::default()
                };
                therm_rc = get_therm(gpu, 0, &mut ts);
                if therm_rc == 0 && ts.count > 0 {
                    let t = ts.sensor[0].current_temp;
                    if t > 0 && t < 130 {
                        return Some(t as f32);
                    }
                }
                None
            });

            let mut pstate_rc = -1;
            let usage = self.get_pstates.and_then(|get_ps| {
                let mut ps = NvDynamicPstatesInfoEx {
                    version: (size_of::<NvDynamicPstatesInfoEx>() as c_uint) | (1 << 16),
                    ..Default::default()
                };
                pstate_rc = get_ps(gpu, &mut ps);
                if pstate_rc == 0 && ps.utilization[0].b_is_present != 0 {
                    return Some((ps.utilization[0].percentage as f32 / 100.0).clamp(0.0, 1.0));
                }
                None
            });

            let mut clk_rc = -1;
            let clock_ghz = self.get_clocks.and_then(|get_clk| {
                let mut cf = NvClockFrequencies {
                    version: (size_of::<NvClockFrequencies>() as c_uint) | (2 << 16),
                    clock_type: 0, // current
                    ..Default::default()
                };
                clk_rc = get_clk(gpu, &mut cf);
                if clk_rc == 0 && cf.domain[0].b_is_present != 0 && cf.domain[0].frequency > 0 {
                    return Some(cf.domain[0].frequency as f32 / 1_000_000.0);
                }
                None
            });

            NvApiSample {
                usage,
                clock_ghz,
                temperature_c,
                diag: format!(
                    "nvapi therm_rc={} pstate_rc={} clk_rc={}",
                    therm_rc, pstate_rc, clk_rc
                ),
            }
        }
    }
}

impl Drop for NvApi {
    fn drop(&mut self) {
        unsafe {
            if let Some(u) = self.unload {
                u();
            }
            let _ = FreeLibrary(self.module);
        }
    }
}

// ============================================================================
// AMD ADL Dynamic Loader (atiadlxx.dll)
// ============================================================================

const ADL_MAX_PATH: usize = 256;
const ADL_PMLOG_MAX_SENSORS: usize = 256;

#[repr(C)]
#[derive(Clone, Copy)]
struct AdlAdapterInfo {
    size: c_int,
    adapter_index: c_int,
    udid: [c_char; ADL_MAX_PATH],
    bus_number: c_int,
    device_number: c_int,
    function_number: c_int,
    vendor_id: c_int,
    adapter_name: [c_char; ADL_MAX_PATH],
    display_name: [c_char; ADL_MAX_PATH],
    present: c_int,
    exist: c_int,
    driver_path: [c_char; ADL_MAX_PATH],
    driver_path_ext: [c_char; ADL_MAX_PATH],
    pnp_string: [c_char; ADL_MAX_PATH],
    os_display_index: c_int,
}

impl Default for AdlAdapterInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Default)]
struct AdlTemperature {
    size: c_int,
    temperature: c_int, // millidegrees Celsius
}

#[repr(C)]
#[derive(Default)]
struct AdlPmActivity {
    size: c_int,
    engine_clock: c_int, // 10 kHz units
    memory_clock: c_int,
    vddc: c_int,
    activity_percent: c_int,
    current_performance_level: c_int,
    current_bus_speed: c_int,
    current_bus_lanes: c_int,
    maximum_bus_lanes: c_int,
    reserved: c_int,
}

#[repr(C)]
#[derive(Default)]
struct AdlFanSpeedValue {
    size: c_int,
    speed_type: c_int, // 1 = percent, 2 = RPM
    fan_speed: c_int,
    flags: c_int,
}

#[repr(C)]
#[derive(Default)]
struct AdlOdNPerformanceStatus {
    core_clock: c_int, // 10 kHz units
    memory_clock: c_int,
    dcef_clock: c_int,
    gfx_clock: c_int,
    uvd_clock: c_int,
    vce_clock: c_int,
    gpu_activity_percent: c_int,
    current_core_perf_level: c_int,
    current_mem_perf_level: c_int,
    current_dcef_perf_level: c_int,
    current_gfx_perf_level: c_int,
    uvd_perf_level: c_int,
    vce_perf_level: c_int,
    current_bus_speed: c_int,
    current_bus_lanes: c_int,
    max_bus_lanes: c_int,
    vddc: c_int,
    vddci: c_int,
}

#[repr(C)]
#[derive(Default)]
struct AdlOdNFanControl {
    mode: c_int,
    fan_control_mode: c_int,
    current_fan_speed_mode: c_int, // 1 = percent, 2 = RPM
    current_fan_speed: c_int,
    target_fan_speed: c_int,
    target_temp: c_int,
    min_performance_clock: c_int,
    min_fan_limit: c_int,
}

#[repr(C)]
#[derive(Default)]
struct AdlOd6CurrentStatus {
    engine_clock: c_int, // 10 kHz units
    memory_clock: c_int,
    activity_percent: c_int,
    current_perf_level: c_int,
    current_bus_speed: c_int,
    current_bus_lanes: c_int,
    max_bus_lanes: c_int,
    reserved: c_int,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AdlSingleSensorData {
    supported: c_int,
    value: c_int,
}

#[repr(C)]
struct AdlPmLogDataOutput {
    size: c_int,
    sensors: [AdlSingleSensorData; ADL_PMLOG_MAX_SENSORS],
}

impl Default for AdlPmLogDataOutput {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

struct AdlSample {
    usage: Option<f32>,
    clock_ghz: Option<f32>,
    temperature_c: Option<f32>,
    fan_percent: Option<f32>,
    diag: String,
}

type AdlMallocFn = unsafe extern "C" fn(c_int) -> *mut c_void;

unsafe extern "C" fn adl_malloc(size: c_int) -> *mut c_void {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    let layout = match std::alloc::Layout::from_size_align(size as usize, 8) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    unsafe { std::alloc::alloc_zeroed(layout) as *mut c_void }
}

struct AdlApi {
    module: HMODULE,
    context: *mut c_void,
    destroy: unsafe extern "C" fn(*mut c_void) -> c_int,
    num_adapters: unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int,
    adapter_info: unsafe extern "C" fn(*mut c_void, *mut AdlAdapterInfo, c_int) -> c_int,
    odn_temp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *mut c_int) -> c_int>,
    odn_perf: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut AdlOdNPerformanceStatus) -> c_int>,
    odn_fan: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut AdlOdNFanControl) -> c_int>,
    od6_status: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut AdlOd6CurrentStatus) -> c_int>,
    od5_temp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *mut AdlTemperature) -> c_int>,
    od5_activity: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut AdlPmActivity) -> c_int>,
    od5_fan: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *mut AdlFanSpeedValue) -> c_int>,
    pm_log_data: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut AdlPmLogDataOutput) -> c_int>,
}

impl AdlApi {
    fn load() -> Option<Self> {
        unsafe {
            let module = load_library_candidates(&[
                w!("atiadlxx.dll"),
                w!("C:\\Windows\\System32\\atiadlxx.dll"),
            ])?;

            let Some(create): Option<
                unsafe extern "C" fn(AdlMallocFn, c_int, *mut *mut c_void) -> c_int,
            > = resolve_sym(module, b"ADL2_Main_Control_Create\0")
            else {
                let _ = FreeLibrary(module);
                return None;
            };
            let Some(destroy): Option<unsafe extern "C" fn(*mut c_void) -> c_int> =
                resolve_sym(module, b"ADL2_Main_Control_Destroy\0")
            else {
                let _ = FreeLibrary(module);
                return None;
            };
            let Some(num_adapters) = resolve_sym(module, b"ADL2_Adapter_NumberOfAdapters_Get\0")
            else {
                let _ = FreeLibrary(module);
                return None;
            };
            let Some(adapter_info) = resolve_sym(module, b"ADL2_Adapter_AdapterInfo_Get\0") else {
                let _ = FreeLibrary(module);
                return None;
            };

            let mut context: *mut c_void = std::ptr::null_mut();
            let mut rc = create(adl_malloc, 0, &mut context);
            if rc != 0 || context.is_null() {
                rc = create(adl_malloc, 1, &mut context);
            }
            if rc != 0 || context.is_null() {
                crate::log_msg(&format!("[GPU Win] ADL2_Main_Control_Create falhou: rc={}", rc));
                let _ = FreeLibrary(module);
                return None;
            }

            let odn_temp = resolve_sym(module, b"ADL2_OverdriveN_Temperature_Get\0");
            let odn_perf = resolve_sym(module, b"ADL2_OverdriveN_PerformanceStatus_Get\0");
            let odn_fan = resolve_sym(module, b"ADL2_OverdriveN_FanControl_Get\0");
            let od6_status = resolve_sym(module, b"ADL2_Overdrive6_CurrentStatus_Get\0");
            let od5_temp = resolve_sym(module, b"ADL2_Overdrive5_Temperature_Get\0");
            let od5_activity = resolve_sym(module, b"ADL2_Overdrive5_CurrentActivity_Get\0");
            let od5_fan = resolve_sym(module, b"ADL2_Overdrive5_FanSpeed_Get\0");
            let pm_log_data = resolve_sym(module, b"ADL2_New_QueryPMLogData_Get\0");

            Some(Self {
                module,
                context,
                destroy,
                num_adapters,
                adapter_info,
                odn_temp,
                odn_perf,
                odn_fan,
                od6_status,
                od5_temp,
                od5_activity,
                od5_fan,
                pm_log_data,
            })
        }
    }

    fn sample(&self, expected_name: &str) -> AdlSample {
        unsafe {
            let mut count: c_int = 0;
            let num_rc = (self.num_adapters)(self.context, &mut count);
            if num_rc != 0 || count <= 0 {
                return AdlSample {
                    usage: None,
                    clock_ghz: None,
                    temperature_c: None,
                    fan_percent: None,
                    diag: format!("num_adapters rc={} count={}", num_rc, count),
                };
            }

            let mut infos = vec![AdlAdapterInfo::default(); count as usize];
            let buf_size = (infos.len() * size_of::<AdlAdapterInfo>()) as c_int;
            let info_rc = (self.adapter_info)(self.context, infos.as_mut_ptr(), buf_size);
            if info_rc != 0 {
                return AdlSample {
                    usage: None,
                    clock_ghz: None,
                    temperature_c: None,
                    fan_percent: None,
                    diag: format!("adapter_info rc={}", info_rc),
                };
            }

            let expected_lower = expected_name.to_ascii_lowercase();
            let mut candidates: Vec<(u8, c_int)> = Vec::new();
            for info in &infos {
                let adl_name = CStr::from_ptr(info.adapter_name.as_ptr())
                    .to_string_lossy()
                    .to_ascii_lowercase();
                let is_amd_vendor = info.vendor_id == 1002
                    || info.vendor_id == 0x1002
                    || adl_name.contains("amd")
                    || adl_name.contains("radeon");
                if !is_amd_vendor {
                    continue;
                }
                let name_match = !adl_name.is_empty()
                    && (expected_lower.contains(&adl_name) || adl_name.contains(&expected_lower));
                let priority = (if name_match { 0 } else { 4 })
                    + (if info.present != 0 { 0 } else { 2 })
                    + (if info.exist != 0 { 0 } else { 1 });
                if !candidates.iter().any(|(_, idx)| *idx == info.adapter_index) {
                    candidates.push((priority, info.adapter_index));
                }
            }
            candidates.sort_by_key(|(prio, _)| *prio);

            if candidates.is_empty() {
                return AdlSample {
                    usage: None,
                    clock_ghz: None,
                    temperature_c: None,
                    fan_percent: None,
                    diag: format!("nenhum adapter AMD entre {} entradas", infos.len()),
                };
            }

            let mut usage: Option<f32> = None;
            let mut clock_ghz: Option<f32> = None;
            let mut temperature_c: Option<f32> = None;
            let mut fan_percent: Option<f32> = None;
            let mut diag_parts: Vec<String> = Vec::new();

            for (_, adapter_index) in &candidates {
                let idx = *adapter_index;

                // 1. PMLog (RDNA1/RDNA2/RDNA3): indices from official ADL_PMLOG_SENSORS enum
                if let Some(query_pm) = self.pm_log_data {
                    let mut pm = AdlPmLogDataOutput {
                        size: size_of::<AdlPmLogDataOutput>() as c_int,
                        ..Default::default()
                    };
                    let rc = query_pm(self.context, idx, &mut pm);
                    diag_parts.push(format!("idx{}:pmlog={}", idx, rc));
                    if rc == 0 {
                        // ADL_PMLOG_CLK_GFXCLK = 1 (MHz)
                        let gfx_clk = pm.sensors[1];
                        if gfx_clk.supported != 0 && gfx_clk.value > 0 && gfx_clk.value < 6000 {
                            clock_ghz = Some(gfx_clk.value as f32 / 1000.0);
                        }
                        // ADL_PMLOG_TEMPERATURE_EDGE = 8 (°C), HOTSPOT = 27 (°C)
                        let temp_edge = pm.sensors[8];
                        let temp_hot = pm.sensors[27];
                        if temp_edge.supported != 0 && temp_edge.value > 0 && temp_edge.value < 130 {
                            temperature_c = Some(temp_edge.value as f32);
                        } else if temp_hot.supported != 0 && temp_hot.value > 0 && temp_hot.value < 130 {
                            temperature_c = Some(temp_hot.value as f32);
                        }
                        // ADL_PMLOG_FAN_PERCENTAGE = 15 (%)
                        let fan = pm.sensors[15];
                        if fan.supported != 0 && (0..=100).contains(&fan.value) {
                            fan_percent = Some(fan.value as f32 / 100.0);
                        }
                        // ADL_PMLOG_INFO_ACTIVITY_GFX = 19 (%)
                        let act = pm.sensors[19];
                        if act.supported != 0 && (0..=100).contains(&act.value) {
                            usage = Some(act.value as f32 / 100.0);
                        }
                    }
                }

                // 2. OverdriveN (Polaris/Vega/RDNA1/RDNA2)
                if temperature_c.is_none() {
                    if let Some(get_temp) = self.odn_temp {
                        for temp_type in [1, 7] {
                            let mut t_mc: c_int = 0;
                            let rc = get_temp(self.context, idx, temp_type, &mut t_mc);
                            if rc == 0 && t_mc > 0 && t_mc < 150_000 {
                                temperature_c = Some(t_mc as f32 / 1000.0);
                                break;
                            }
                            if temp_type == 1 {
                                diag_parts.push(format!("idx{}:odn_temp={}", idx, rc));
                            }
                        }
                    }
                }

                if clock_ghz.is_none() || usage.is_none() {
                    if let Some(get_perf) = self.odn_perf {
                        let mut perf = AdlOdNPerformanceStatus::default();
                        let rc = get_perf(self.context, idx, &mut perf);
                        if rc == 0 {
                            if clock_ghz.is_none() && perf.core_clock > 0 {
                                clock_ghz = Some(perf.core_clock as f32 / 100_000.0);
                            }
                            if usage.is_none() && (0..=100).contains(&perf.gpu_activity_percent) {
                                usage = Some(perf.gpu_activity_percent as f32 / 100.0);
                            }
                        } else {
                            diag_parts.push(format!("idx{}:odn_perf={}", idx, rc));
                        }
                    }
                }

                if fan_percent.is_none() {
                    if let Some(get_fan) = self.odn_fan {
                        let mut fan = AdlOdNFanControl::default();
                        let rc = get_fan(self.context, idx, &mut fan);
                        if rc == 0
                            && fan.current_fan_speed_mode == 1
                            && (0..=100).contains(&fan.current_fan_speed)
                        {
                            fan_percent = Some(fan.current_fan_speed as f32 / 100.0);
                        } else if rc != 0 {
                            diag_parts.push(format!("idx{}:odn_fan={}", idx, rc));
                        }
                    }
                }

                // 3. Overdrive6 fallback
                if clock_ghz.is_none() || usage.is_none() {
                    if let Some(get_od6) = self.od6_status {
                        let mut st = AdlOd6CurrentStatus::default();
                        if get_od6(self.context, idx, &mut st) == 0 {
                            if clock_ghz.is_none() && st.engine_clock > 0 {
                                clock_ghz = Some(st.engine_clock as f32 / 100_000.0);
                            }
                            if usage.is_none() && (0..=100).contains(&st.activity_percent) {
                                usage = Some(st.activity_percent as f32 / 100.0);
                            }
                        }
                    }
                }

                // 4. Overdrive5 legacy fallback
                if temperature_c.is_none() {
                    if let Some(get_temp) = self.od5_temp {
                        let mut t = AdlTemperature {
                            size: size_of::<AdlTemperature>() as c_int,
                            temperature: 0,
                        };
                        if get_temp(self.context, idx, 0, &mut t) == 0 && t.temperature > 0 {
                            temperature_c = Some(t.temperature as f32 / 1000.0);
                        }
                    }
                }

                if usage.is_none() || clock_ghz.is_none() {
                    if let Some(get_act) = self.od5_activity {
                        let mut act = AdlPmActivity {
                            size: size_of::<AdlPmActivity>() as c_int,
                            ..Default::default()
                        };
                        if get_act(self.context, idx, &mut act) == 0 {
                            if usage.is_none() && (0..=100).contains(&act.activity_percent) {
                                usage = Some(act.activity_percent as f32 / 100.0);
                            }
                            if clock_ghz.is_none() && act.engine_clock > 0 {
                                clock_ghz = Some(act.engine_clock as f32 / 100_000.0);
                            }
                        }
                    }
                }

                if fan_percent.is_none() {
                    if let Some(get_fan) = self.od5_fan {
                        let mut fan = AdlFanSpeedValue {
                            size: size_of::<AdlFanSpeedValue>() as c_int,
                            speed_type: 1,
                            fan_speed: 0,
                            flags: 0,
                        };
                        if get_fan(self.context, idx, 0, &mut fan) == 0
                            && (0..=100).contains(&fan.fan_speed)
                        {
                            fan_percent = Some(fan.fan_speed as f32 / 100.0);
                        }
                    }
                }

                if temperature_c.is_some() || clock_ghz.is_some() || fan_percent.is_some() {
                    break;
                }
            }

            AdlSample {
                usage,
                clock_ghz,
                temperature_c,
                fan_percent,
                diag: format!(
                    "candidates={:?} temp={:?} clk={:?} fan={:?} [{}]",
                    candidates,
                    temperature_c,
                    clock_ghz,
                    fan_percent,
                    diag_parts.join(", ")
                ),
            }
        }
    }
}

impl Drop for AdlApi {
    fn drop(&mut self) {
        unsafe {
            if !self.context.is_null() {
                (self.destroy)(self.context);
            }
            let _ = FreeLibrary(self.module);
        }
    }
}
