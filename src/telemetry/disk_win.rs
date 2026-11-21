use std::collections::HashMap;
use windows::core::w;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW, PDH_HCOUNTER,
    PDH_HQUERY,
};

pub struct WindowsDiskIoMonitor {
    hquery: PDH_HQUERY,
    hcounter_read: PDH_HCOUNTER,
    hcounter_write: PDH_HCOUNTER,
    initialized: bool,
}

impl WindowsDiskIoMonitor {
    pub fn new() -> Option<Self> {
        unsafe {
            let mut hquery = PDH_HQUERY::default();
            if PdhOpenQueryW(None, 0, &mut hquery) != 0 || hquery.is_invalid() {
                return None;
            }

            let mut hcounter_read = PDH_HCOUNTER::default();
            let read_ok = PdhAddEnglishCounterW(
                hquery,
                w!("\\LogicalDisk(*)\\Disk Read Bytes/sec"),
                0,
                &mut hcounter_read,
            ) == 0;

            let mut hcounter_write = PDH_HCOUNTER::default();
            let write_ok = PdhAddEnglishCounterW(
                hquery,
                w!("\\LogicalDisk(*)\\Disk Write Bytes/sec"),
                0,
                &mut hcounter_write,
            ) == 0;

            if !read_ok && !write_ok {
                let _ = PdhCloseQuery(hquery);
                return None;
            }

            let _ = PdhCollectQueryData(hquery);
            Some(Self {
                hquery,
                hcounter_read: if read_ok { hcounter_read } else { PDH_HCOUNTER::default() },
                hcounter_write: if write_ok { hcounter_write } else { PDH_HCOUNTER::default() },
                initialized: false,
            })
        }
    }

    /// Returns a map from uppercase drive key (e.g. `"C:"`) to `(read_mb_s, write_mb_s)`.
    pub fn sample(&mut self) -> HashMap<String, (f64, f64)> {
        let mut speeds: HashMap<String, (f64, f64)> = HashMap::new();
        unsafe {
            if PdhCollectQueryData(self.hquery) != 0 {
                return speeds;
            }
            if !self.initialized {
                self.initialized = true;
                return speeds;
            }

            const MIB: f64 = 1024.0 * 1024.0;

            if !self.hcounter_read.is_invalid() {
                if let Some(items) = super::gpu_win::read_pdh_double_array(self.hcounter_read) {
                    for (name, bytes_per_sec) in items {
                        let key = normalize_drive_key(&name);
                        if !key.is_empty() && bytes_per_sec >= 0.0 {
                            speeds.entry(key).or_insert((0.0, 0.0)).0 = bytes_per_sec / MIB;
                        }
                    }
                }
            }

            if !self.hcounter_write.is_invalid() {
                if let Some(items) = super::gpu_win::read_pdh_double_array(self.hcounter_write) {
                    for (name, bytes_per_sec) in items {
                        let key = normalize_drive_key(&name);
                        if !key.is_empty() && bytes_per_sec >= 0.0 {
                            speeds.entry(key).or_insert((0.0, 0.0)).1 = bytes_per_sec / MIB;
                        }
                    }
                }
            }
        }
        speeds
    }
}

impl Drop for WindowsDiskIoMonitor {
    fn drop(&mut self) {
        unsafe {
            if !self.hquery.is_invalid() {
                let _ = PdhCloseQuery(self.hquery);
            }
        }
    }
}

pub fn normalize_drive_key(raw: &str) -> String {
    raw.trim().trim_end_matches(['\\', '/']).to_ascii_uppercase()
}
