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
    adl: Option<AdlApi>,
    adl_checked: bool,
}

impl WindowsGpuMonitor {
    pub fn new() -> Self {
        Self {
            pdh: PdhGpuQuery::new(),
            nvml: None,
            nvml_checked: false,
            adl: None,
            adl_checked: false,
        }
    }

    pub fn sample(&mut self) -> Option<WinGpuSample> {
        let adapter = query_dxgi_primary_adapter()?;

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
            }
            if let Some(nvml) = self.nvml.as_ref() {
                if let Some(nv) = nvml.sample(&adapter.name) {
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
            }
        } else if adapter.vendor_id == 0x1002 {
            if !self.adl_checked {
                self.adl_checked = true;
                self.adl = AdlApi::load();
            }
            if let Some(adl) = self.adl.as_ref() {
                if let Some(amd) = adl.sample(&adapter.name) {
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
}

type NvmlDevice = *mut c_void;

struct NvmlApi {
    module: HMODULE,
    shutdown: unsafe extern "C" fn() -> c_int,
    device_get_count: unsafe extern "C" fn(*mut c_uint) -> c_int,
    device_get_handle: unsafe extern "C" fn(c_uint, *mut NvmlDevice) -> c_int,
    device_get_name: unsafe extern "C" fn(NvmlDevice, *mut c_char, c_uint) -> c_int,
    device_get_utilization: unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> c_int,
    device_get_clock: unsafe extern "C" fn(NvmlDevice, c_int, *mut c_uint) -> c_int,
    device_get_memory: unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> c_int,
    device_get_temp: unsafe extern "C" fn(NvmlDevice, c_int, *mut c_uint) -> c_int,
    device_get_fan: Option<unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_int>,
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
            ])?;

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

            if init() != 0 {
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
            let Some(device_get_name) = resolve_sym(module, b"nvmlDeviceGetName\0") else {
                shutdown();
                let _ = FreeLibrary(module);
                return None;
            };
            let Some(device_get_utilization) =
                resolve_sym(module, b"nvmlDeviceGetUtilizationRates\0")
            else {
                shutdown();
                let _ = FreeLibrary(module);
                return None;
            };
            let Some(device_get_clock) = resolve_sym(module, b"nvmlDeviceGetClockInfo\0") else {
                shutdown();
                let _ = FreeLibrary(module);
                return None;
            };
            let Some(device_get_memory) = resolve_sym(module, b"nvmlDeviceGetMemoryInfo\0") else {
                shutdown();
                let _ = FreeLibrary(module);
                return None;
            };
            let Some(device_get_temp) = resolve_sym(module, b"nvmlDeviceGetTemperature\0") else {
                shutdown();
                let _ = FreeLibrary(module);
                return None;
            };
            let device_get_fan = resolve_sym(module, b"nvmlDeviceGetFanSpeed\0");

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
            })
        }
    }

    fn sample(&self, expected_name: &str) -> Option<NvmlSample> {
        unsafe {
            let mut count = 0u32;
            if (self.device_get_count)(&mut count) != 0 || count == 0 {
                return None;
            }

            let mut chosen: Option<NvmlDevice> = None;
            let expected_lower = expected_name.to_ascii_lowercase();
            for i in 0..count {
                let mut dev: NvmlDevice = std::ptr::null_mut();
                if (self.device_get_handle)(i, &mut dev) != 0 || dev.is_null() {
                    continue;
                }
                if chosen.is_none() {
                    chosen = Some(dev);
                }
                let mut name_buf = [0 as c_char; 96];
                if (self.device_get_name)(dev, name_buf.as_mut_ptr(), name_buf.len() as u32) == 0 {
                    let nv_name = CStr::from_ptr(name_buf.as_ptr())
                        .to_string_lossy()
                        .to_ascii_lowercase();
                    if expected_lower.contains(&nv_name) || nv_name.contains(&expected_lower) {
                        chosen = Some(dev);
                        break;
                    }
                }
            }

            let dev = chosen?;

            let mut util = NvmlUtilization { gpu: 0, memory: 0 };
            let usage = if (self.device_get_utilization)(dev, &mut util) == 0 {
                Some((util.gpu as f32 / 100.0).clamp(0.0, 1.0))
            } else {
                None
            };

            let mut clock_mhz = 0u32;
            let clock_ghz = if (self.device_get_clock)(dev, 0, &mut clock_mhz) == 0 {
                Some(clock_mhz as f32 / 1000.0)
            } else {
                None
            };

            let mut mem = NvmlMemory {
                total: 0,
                free: 0,
                used: 0,
            };
            let (vram_used_gb, vram_total_gb, vram_percent) =
                if (self.device_get_memory)(dev, &mut mem) == 0 && mem.total > 0 {
                    let gib = 1024.0 * 1024.0 * 1024.0;
                    (
                        mem.used as f64 / gib,
                        mem.total as f64 / gib,
                        Some((mem.used as f64 / mem.total as f64).clamp(0.0, 1.0) as f32),
                    )
                } else {
                    (0.0, 0.0, None)
                };

            let mut temp_c = 0u32;
            let temperature_c = if (self.device_get_temp)(dev, 0, &mut temp_c) == 0 {
                Some(temp_c as f32)
            } else {
                None
            };

            let fan_percent = self.device_get_fan.and_then(|get_fan| {
                let mut fan = 0u32;
                if get_fan(dev, &mut fan) == 0 && fan <= 100 {
                    Some(fan as f32 / 100.0)
                } else {
                    None
                }
            });

            Some(NvmlSample {
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
    od5_temp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *mut AdlTemperature) -> c_int>,
    od5_activity: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut AdlPmActivity) -> c_int>,
    od5_fan: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *mut AdlFanSpeedValue) -> c_int>,
    pm_log_data: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut AdlPmLogDataOutput) -> c_int>,
}

impl AdlApi {
    fn load() -> Option<Self> {
        unsafe {
            let module = load_library_candidates(&[w!("atiadlxx.dll")])?;

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
            if create(adl_malloc, 1, &mut context) != 0 || context.is_null() {
                let _ = FreeLibrary(module);
                return None;
            }

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
                od5_temp,
                od5_activity,
                od5_fan,
                pm_log_data,
            })
        }
    }

    fn sample(&self, expected_name: &str) -> Option<AdlSample> {
        unsafe {
            let mut count: c_int = 0;
            if (self.num_adapters)(self.context, &mut count) != 0 || count <= 0 {
                return None;
            }

            let mut infos = vec![AdlAdapterInfo::default(); count as usize];
            let buf_size = (infos.len() * size_of::<AdlAdapterInfo>()) as c_int;
            if (self.adapter_info)(self.context, infos.as_mut_ptr(), buf_size) != 0 {
                return None;
            }

            let expected_lower = expected_name.to_ascii_lowercase();
            let mut chosen_idx: Option<c_int> = None;
            for info in &infos {
                if info.vendor_id != 0x1002 {
                    continue;
                }
                if chosen_idx.is_none() {
                    chosen_idx = Some(info.adapter_index);
                }
                let adl_name = CStr::from_ptr(info.adapter_name.as_ptr())
                    .to_string_lossy()
                    .to_ascii_lowercase();
                if expected_lower.contains(&adl_name) || adl_name.contains(&expected_lower) {
                    chosen_idx = Some(info.adapter_index);
                    break;
                }
            }

            let adapter_index = chosen_idx?;
            let mut usage: Option<f32> = None;
            let mut clock_ghz: Option<f32> = None;
            let mut temperature_c: Option<f32> = None;
            let mut fan_percent: Option<f32> = None;

            if let Some(query_pm) = self.pm_log_data {
                let mut pm = AdlPmLogDataOutput {
                    size: size_of::<AdlPmLogDataOutput>() as c_int,
                    ..Default::default()
                };
                if query_pm(self.context, adapter_index, &mut pm) == 0 {
                    // ADL_PMLOG_CLK_GFXCLK = 1 (MHz)
                    let gfx_clk = pm.sensors[1];
                    if gfx_clk.supported != 0 && gfx_clk.value > 0 && gfx_clk.value < 6000 {
                        clock_ghz = Some(gfx_clk.value as f32 / 1000.0);
                    }
                    // ADL_PMLOG_TEMPERATURE_EDGE = 5 (°C)
                    let temp_edge = pm.sensors[5];
                    if temp_edge.supported != 0 && temp_edge.value > 0 && temp_edge.value < 130 {
                        temperature_c = Some(temp_edge.value as f32);
                    }
                    // ADL_PMLOG_FAN_PERCENTAGE = 14 (%)
                    let fan = pm.sensors[14];
                    if fan.supported != 0 && (0..=100).contains(&fan.value) {
                        fan_percent = Some(fan.value as f32 / 100.0);
                    }
                    // ADL_PMLOG_INFO_ACTIVITY_GFX = 22 (%)
                    let act = pm.sensors[22];
                    if act.supported != 0 && (0..=100).contains(&act.value) {
                        usage = Some(act.value as f32 / 100.0);
                    }
                }
            }

            if temperature_c.is_none() {
                if let Some(get_temp) = self.od5_temp {
                    let mut t = AdlTemperature {
                        size: size_of::<AdlTemperature>() as c_int,
                        temperature: 0,
                    };
                    if get_temp(self.context, adapter_index, 0, &mut t) == 0 && t.temperature > 0 {
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
                    if get_act(self.context, adapter_index, &mut act) == 0 {
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
                    if get_fan(self.context, adapter_index, 0, &mut fan) == 0
                        && (0..=100).contains(&fan.fan_speed)
                    {
                        fan_percent = Some(fan.fan_speed as f32 / 100.0);
                    }
                }
            }

            Some(AdlSample {
                usage,
                clock_ghz,
                temperature_c,
                fan_percent,
            })
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
