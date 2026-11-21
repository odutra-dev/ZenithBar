use std::ffi::CStr;
use std::mem::size_of;

#[derive(Default)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct ShmSensorSample {
    pub cpu_temp_c: Option<f32>,
    pub gpu_temp_c: Option<f32>,
    pub gpu_clock_ghz: Option<f32>,
    pub gpu_usage: Option<f32>,
    pub gpu_fan_percent: Option<f32>,
}

const HWINFO_SIGNATURE: u32 = 0x5369_5748; // "HWiS"
const HWINFO_READING_TYPE_TEMP: u32 = 1;
const HWINFO_READING_TYPE_CLOCK: u32 = 6;
const HWINFO_READING_TYPE_USAGE: u32 = 7;

#[repr(C, packed)]
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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

#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn c_bytes_to_lowercase(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    CStr::from_bytes_with_nul(&[&buf[..end], &[0]].concat())
        .ok()
        .and_then(|c| c.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn validate_hwinfo_header(hdr: &HwInfoHeader) -> Option<(usize, usize, usize, usize)> {
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
    Some((offset, elem_size, count, total_size))
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_hwinfo_readings_slice(bytes: &[u8], offset: usize, elem_size: usize, count: usize) -> Option<ShmSensorSample> {
    let required = offset.checked_add(count.checked_mul(elem_size)?)?;
    if bytes.len() < required {
        return None;
    }

    let mut out = ShmSensorSample::default();
    let mut best_cpu_prio = u8::MAX;
    let base = bytes.as_ptr();

    for i in 0..count {
        let reading = unsafe {
            let ptr = base.add(offset + i * elem_size) as *const HwInfoReading;
            std::ptr::read_unaligned(ptr)
        };
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
            } else if (label.contains("gpu temperature") || label.contains("gpu edge"))
                && out.gpu_temp_c.is_none()
            {
                out.gpu_temp_c = Some(val);
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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn parse_hwinfo_shm_bytes(bytes: &[u8]) -> Option<ShmSensorSample> {
    if bytes.len() < size_of::<HwInfoHeader>() {
        return None;
    }
    let hdr = unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const HwInfoHeader) };
    let (offset, elem_size, count, _total) = validate_hwinfo_header(&hdr)?;
    parse_hwinfo_readings_slice(bytes, offset, elem_size, count)
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn parse_coretemp_shm_bytes(bytes: &[u8]) -> Option<f32> {
    if bytes.len() < size_of::<CoreTempSharedData>() {
        return None;
    }
    let data = unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const CoreTempSharedData) };
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

#[cfg(target_os = "windows")]
mod win_impl {
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Memory::{
        MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS,
    };

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
            let (offset, elem_size, count, total_size) = validate_hwinfo_header(&hdr)?;
            drop(hdr_view);

            let full_view = MappedView(MapViewOfFile(handle.0, FILE_MAP_READ, 0, 0, total_size));
            if full_view.0.Value.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(full_view.0.Value as *const u8, total_size);
            parse_hwinfo_readings_slice(bytes, offset, elem_size, count)
        }
    }

    fn read_coretemp_shm() -> Option<f32> {
        unsafe {
            let handle = open_mapping_candidates(&[
                w!("CoreTempMappingObject"),
                w!("Global\\CoreTempMappingObject"),
            ])?;

            let len = size_of::<CoreTempSharedData>();
            let view = MappedView(MapViewOfFile(handle.0, FILE_MAP_READ, 0, 0, len));
            if view.0.Value.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(view.0.Value as *const u8, len);
            parse_coretemp_shm_bytes(bytes)
        }
    }
}

#[cfg(target_os = "windows")]
pub use win_impl::WindowsShmMonitor;

#[cfg(test)]
mod tests {
    use super::*;

    fn as_bytes<T>(val: &T) -> &[u8] {
        unsafe { std::slice::from_raw_parts(val as *const T as *const u8, size_of::<T>()) }
    }

    fn make_reading(t_reading: u32, label: &str, value: f64) -> HwInfoReading {
        let mut reading = HwInfoReading {
            t_reading,
            dw_sensor_index: 0,
            dw_reading_id: 0,
            sz_label_orig: [0; 128],
            sz_label_user: [0; 128],
            sz_unit: [0; 16],
            value,
            value_min: value,
            value_max: value,
            value_avg: value,
        };
        let bytes = label.as_bytes();
        let copy_len = bytes.len().min(127);
        reading.sz_label_orig[..copy_len].copy_from_slice(&bytes[..copy_len]);
        reading
    }

    fn make_coretemp() -> CoreTempSharedData {
        CoreTempSharedData {
            ui_load: [0; 256],
            ui_tj_max: [100; 128],
            ui_core_cnt: 0,
            ui_cpu_cnt: 1,
            f_temp: [0.0; 256],
            f_vid: 1.2,
            f_cpu_speed: 3800.0,
            f_fsb_speed: 100.0,
            f_multiplier: 38.0,
            s_cpu_name: [0; 100],
            uc_fahrenheit: 0,
            uc_delta_to_tj_max: 0,
        }
    }

    #[test]
    fn c_bytes_to_lowercase_handles_nul_and_unterminated_slices() {
        assert_eq!(c_bytes_to_lowercase(b"CPU (Tctl/Tdie)\0garbage"), "cpu (tctl/tdie)");
        assert_eq!(c_bytes_to_lowercase(b"GPU Clock"), "gpu clock");
        assert_eq!(c_bytes_to_lowercase(&[]), "");
    }

    #[test]
    fn parse_hwinfo_shm_bytes_rejects_bad_headers_and_truncated_buffers() {
        assert!(parse_hwinfo_shm_bytes(&[0u8; 8]).is_none());

        let bad_sig = HwInfoHeader {
            dw_signature: 0xDEAD_BEEF,
            dw_version: 1,
            dw_revision: 0,
            poll_time: 0,
            dw_offset_of_sensor_section: 0,
            dw_size_of_sensor_element: 0,
            dw_num_sensor_elements: 0,
            dw_offset_of_reading_section: size_of::<HwInfoHeader>() as u32,
            dw_size_of_reading_element: size_of::<HwInfoReading>() as u32,
            dw_num_reading_elements: 1,
        };
        assert!(parse_hwinfo_shm_bytes(as_bytes(&bad_sig)).is_none());

        // Valid signature, but buffer is missing the reading element payload
        let truncated = HwInfoHeader {
            dw_signature: HWINFO_SIGNATURE,
            ..bad_sig
        };
        assert!(parse_hwinfo_shm_bytes(as_bytes(&truncated)).is_none());
    }

    #[test]
    fn parse_hwinfo_shm_bytes_extracts_priority_cpu_temp_and_gpu_metrics() {
        let readings = [
            make_reading(HWINFO_READING_TYPE_TEMP, "Core Max", 71.0),
            make_reading(HWINFO_READING_TYPE_TEMP, "CPU (Tctl/Tdie)", 64.5),
            make_reading(HWINFO_READING_TYPE_TEMP, "GPU Temperature", 58.0),
            make_reading(HWINFO_READING_TYPE_CLOCK, "GPU Clock", 2450.0),
            make_reading(HWINFO_READING_TYPE_USAGE, "GPU Core Load", 42.0),
            make_reading(HWINFO_READING_TYPE_TEMP, "CPU Package", f64::NAN),
        ];

        let offset = size_of::<HwInfoHeader>();
        let hdr = HwInfoHeader {
            dw_signature: HWINFO_SIGNATURE,
            dw_version: 1,
            dw_revision: 0,
            poll_time: 123,
            dw_offset_of_sensor_section: 0,
            dw_size_of_sensor_element: 0,
            dw_num_sensor_elements: 0,
            dw_offset_of_reading_section: offset as u32,
            dw_size_of_reading_element: size_of::<HwInfoReading>() as u32,
            dw_num_reading_elements: readings.len() as u32,
        };

        let mut buf = Vec::with_capacity(offset + readings.len() * size_of::<HwInfoReading>());
        buf.extend_from_slice(as_bytes(&hdr));
        for r in &readings {
            buf.extend_from_slice(as_bytes(r));
        }

        let sample = parse_hwinfo_shm_bytes(&buf).expect("valid HWiNFO buffer must parse");
        assert_eq!(sample.cpu_temp_c, Some(64.5));
        assert_eq!(sample.gpu_temp_c, Some(58.0));
        assert!((sample.gpu_clock_ghz.unwrap() - 2.45).abs() < 1e-4);
        assert!((sample.gpu_usage.unwrap() - 0.42).abs() < 1e-4);
    }

    #[test]
    fn parse_coretemp_shm_bytes_handles_celsius_delta_tjmax_and_fahrenheit() {
        assert!(parse_coretemp_shm_bytes(&[0u8; 16]).is_none());

        // Zero cores -> None
        let mut ct = make_coretemp();
        assert!(parse_coretemp_shm_bytes(as_bytes(&ct)).is_none());

        // Direct Celsius
        ct.ui_core_cnt = 2;
        ct.f_temp[0] = 52.0;
        ct.f_temp[1] = 61.5;
        assert_eq!(parse_coretemp_shm_bytes(as_bytes(&ct)), Some(61.5));

        // Delta to TjMax (100 - 30 = 70 C)
        ct.uc_delta_to_tj_max = 1;
        ct.ui_tj_max[0] = 100;
        ct.f_temp[0] = 40.0;
        ct.f_temp[1] = 30.0;
        assert_eq!(parse_coretemp_shm_bytes(as_bytes(&ct)), Some(70.0));

        // Fahrenheit (140 F = 60 C)
        ct.uc_delta_to_tj_max = 0;
        ct.uc_fahrenheit = 1;
        ct.f_temp[0] = 122.0; // 50 C
        ct.f_temp[1] = 140.0; // 60 C
        let temp = parse_coretemp_shm_bytes(as_bytes(&ct)).unwrap();
        assert!((temp - 60.0).abs() < 0.01);
    }
}
