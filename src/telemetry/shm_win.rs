use std::ffi::CStr;
use std::mem::size_of;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS,
};

#[derive(Default)]
pub struct ShmSensorSample {
    pub cpu_temp_c: Option<f32>,
    pub gpu_temp_c: Option<f32>,
    pub gpu_clock_ghz: Option<f32>,
    pub gpu_usage: Option<f32>,
    pub gpu_fan_percent: Option<f32>,
}

struct MappingHandle(HANDLE);

impl Drop for MappingHandle {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_invalid() {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

struct MappedView(MEMORY_MAPPED_VIEW_ADDRESS);

impl Drop for MappedView {
    fn drop(&mut self) {
        unsafe {
            if !self.0.Value.is_null() {
                let _ = UnmapViewOfFile(self.0);
            }
        }
    }
}

pub struct WindowsShmMonitor {
    logged_hwinfo: bool,
    logged_coretemp: bool,
}

impl WindowsShmMonitor {
    pub fn new() -> Self {
        Self {
            logged_hwinfo: false,
            logged_coretemp: false,
        }
    }

    pub fn sample(&mut self, need_cpu_temp: bool, need_gpu_sensors: bool) -> ShmSensorSample {
        let mut out = ShmSensorSample::default();
        if !need_cpu_temp && !need_gpu_sensors {
            return out;
        }

        if let Some(hw) = read_hwinfo_shm() {
            if !self.logged_hwinfo {
                self.logged_hwinfo = true;
                crate::log_msg("[SHM Win] HWiNFO Shared Memory detectada.");
            }
            out = hw;
        }

        if need_cpu_temp && out.cpu_temp_c.is_none() {
            if let Some(ct_temp) = read_coretemp_shm() {
                if !self.logged_coretemp {
                    self.logged_coretemp = true;
                    crate::log_msg("[SHM Win] Core Temp Shared Memory detectada.");
                }
                out.cpu_temp_c = Some(ct_temp);
            }
        }

        out
    }
}

// ============================================================================
// HWiNFO Shared Memory (Global\HWiNFO_SENS_SM2 / Local\HWiNFO_SENS_SM2)
// ============================================================================

const HWINFO_SIGNATURE: u32 = 0x5369_5748; // "HWiS"
const HWINFO_READING_TYPE_TEMP: u32 = 1;
const HWINFO_READING_TYPE_CLOCK: u32 = 6;
const HWINFO_READING_TYPE_USAGE: u32 = 7;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct HwInfoHeader {
    dw_signature: u32,
    dw_version: u32,
    dw_revision: u32,
    poll_time: i64,
    dw_offset_of_sensor_section: u32,
    dw_size_of_sensor_element: u32,
    dw_num_sensor_elements: u32,
    dw_offset_of_reading_section: u32,
    dw_size_of_reading_element: u32,
    dw_num_reading_elements: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct HwInfoReading {
    t_reading: u32,
    dw_sensor_index: u32,
    dw_reading_id: u32,
    sz_label_orig: [u8; 128],
    sz_label_user: [u8; 128],
    sz_unit: [u8; 16],
    value: f64,
    value_min: f64,
    value_max: f64,
    value_avg: f64,
}

fn open_mapping_candidates(names: &[PCWSTR]) -> Option<MappingHandle> {
    unsafe {
        for &name in names {
            if let Ok(h) = OpenFileMappingW(FILE_MAP_READ.0, false, name) {
                if !h.is_invalid() {
                    return Some(MappingHandle(h));
                }
            }
        }
        None
    }
}

fn c_bytes_to_lowercase(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    CStr::from_bytes_with_nul(&[&buf[..end], &[0]].concat())
        .ok()
        .and_then(|c| c.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn read_hwinfo_shm() -> Option<ShmSensorSample> {
    unsafe {
        let handle = open_mapping_candidates(&[
            w!("Global\\HWiNFO_SENS_SM2"),
            w!("Local\\HWiNFO_SENS_SM2"),
            w!("HWiNFO_SENS_SM2"),
        ])?;

        let hdr_view = MappedView(MapViewOfFile(
            handle.0,
            FILE_MAP_READ,
            0,
            0,
            size_of::<HwInfoHeader>(),
        ));
        if hdr_view.0.Value.is_null() {
            return None;
        }
        let hdr = std::ptr::read_unaligned(hdr_view.0.Value as *const HwInfoHeader);
        if hdr.dw_signature != HWINFO_SIGNATURE {
            return None;
        }
        let count = hdr.dw_num_reading_elements as usize;
        let elem_size = hdr.dw_size_of_reading_element as usize;
        let offset = hdr.dw_offset_of_reading_section as usize;
        if count == 0 || count > 4096 || elem_size < size_of::<HwInfoReading>() {
            return None;
        }
        let total_size = offset.checked_add(count.checked_mul(elem_size)?)?;
        drop(hdr_view);

        let full_view = MappedView(MapViewOfFile(handle.0, FILE_MAP_READ, 0, 0, total_size));
        if full_view.0.Value.is_null() {
            return None;
        }
        let base = full_view.0.Value as *const u8;

        let mut out = ShmSensorSample::default();
        let mut best_cpu_prio = u8::MAX;

        for i in 0..count {
            let ptr = base.add(offset + i * elem_size) as *const HwInfoReading;
            let reading = std::ptr::read_unaligned(ptr);
            let label = c_bytes_to_lowercase(&reading.sz_label_orig);
            let val = reading.value as f32;

            if !val.is_finite() {
                continue;
            }

            if reading.t_reading == HWINFO_READING_TYPE_TEMP && val > 0.0 && val < 130.0 {
                if label.contains("tctl/tdie") || label.contains("cpu package") {
                    if best_cpu_prio > 0 {
                        best_cpu_prio = 0;
                        out.cpu_temp_c = Some(val);
                    }
                } else if label.contains("tdie") || label.contains("ccd1") || label.contains("cpu (") {
                    if best_cpu_prio > 1 {
                        best_cpu_prio = 1;
                        out.cpu_temp_c = Some(val);
                    }
                } else if label.contains("core max") && best_cpu_prio > 2 {
                    best_cpu_prio = 2;
                    out.cpu_temp_c = Some(val);
                } else if label.contains("gpu temperature") || label.contains("gpu edge") {
                    if out.gpu_temp_c.is_none() {
                        out.gpu_temp_c = Some(val);
                    }
                }
            } else if reading.t_reading == HWINFO_READING_TYPE_CLOCK
                && label.contains("gpu clock")
                && val > 0.0
                && out.gpu_clock_ghz.is_none()
            {
                out.gpu_clock_ghz = Some(val / 1000.0);
            } else if reading.t_reading == HWINFO_READING_TYPE_USAGE
                && label.contains("gpu core load")
                && (0.0..=100.0).contains(&val)
                && out.gpu_usage.is_none()
            {
                out.gpu_usage = Some(val / 100.0);
            }
        }

        Some(out)
    }
}

// ============================================================================
// Core Temp Shared Memory (CoreTempMappingObject)
// ============================================================================

#[repr(C)]
struct CoreTempSharedData {
    ui_load: [u32; 256],
    ui_tj_max: [u32; 128],
    ui_core_cnt: u32,
    ui_cpu_cnt: u32,
    f_temp: [f32; 256],
    f_vid: f32,
    f_cpu_speed: f32,
    f_fsb_speed: f32,
    f_multiplier: f32,
    s_cpu_name: [u8; 100],
    uc_fahrenheit: u8,
    uc_delta_to_tj_max: u8,
}

fn read_coretemp_shm() -> Option<f32> {
    unsafe {
        let handle = open_mapping_candidates(&[
            w!("CoreTempMappingObject"),
            w!("Global\\CoreTempMappingObject"),
        ])?;

        let view = MappedView(MapViewOfFile(
            handle.0,
            FILE_MAP_READ,
            0,
            0,
            size_of::<CoreTempSharedData>(),
        ));
        if view.0.Value.is_null() {
            return None;
        }

        let data = std::ptr::read_unaligned(view.0.Value as *const CoreTempSharedData);
        if data.ui_core_cnt == 0 {
            return None;
        }

        let count = (data.ui_core_cnt as usize).min(256);
        let mut max_temp: Option<f32> = None;
        for i in 0..count {
            let mut t = data.f_temp[i];
            if !t.is_finite() {
                continue;
            }
            if data.uc_delta_to_tj_max != 0 {
                let tj = data.ui_tj_max.first().copied().unwrap_or(100) as f32;
                t = tj - t;
            }
            if data.uc_fahrenheit != 0 {
                t = (t - 32.0) / 1.8;
            }
            if t > 5.0 && t < 130.0 {
                max_temp = Some(max_temp.map_or(t, |m| m.max(t)));
            }
        }
        max_temp
    }
}
