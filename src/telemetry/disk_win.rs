use std::collections::HashMap;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn normalize_drive_key(raw: &str) -> String {
    raw.trim().trim_end_matches(['\\', '/']).to_ascii_uppercase()
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn aggregate_disk_io_samples(
    read_items: &[(String, f64)],
    write_items: &[(String, f64)],
) -> HashMap<String, (f64, f64)> {
    const MIB: f64 = 1024.0 * 1024.0;
    let mut speeds: HashMap<String, (f64, f64)> = HashMap::new();

    for (name, bytes_per_sec) in read_items {
        let key = normalize_drive_key(name);
        if !key.is_empty() && *bytes_per_sec >= 0.0 && bytes_per_sec.is_finite() {
            speeds.entry(key).or_insert((0.0, 0.0)).0 = bytes_per_sec / MIB;
        }
    }

    for (name, bytes_per_sec) in write_items {
        let key = normalize_drive_key(name);
        if !key.is_empty() && *bytes_per_sec >= 0.0 && bytes_per_sec.is_finite() {
            speeds.entry(key).or_insert((0.0, 0.0)).1 = bytes_per_sec / MIB;
        }
    }

    speeds
}

#[cfg(target_os = "windows")]
mod win_impl {
    use super::*;
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
            unsafe {
                if PdhCollectQueryData(self.hquery) != 0 {
                    return HashMap::new();
                }
                if !self.initialized {
                    self.initialized = true;
                    return HashMap::new();
                }

                let read_items = if !self.hcounter_read.is_invalid() {
                    super::super::gpu_win::read_pdh_double_array(self.hcounter_read).unwrap_or_default()
                } else {
                    Vec::new()
                };

                let write_items = if !self.hcounter_write.is_invalid() {
                    super::super::gpu_win::read_pdh_double_array(self.hcounter_write).unwrap_or_default()
                } else {
                    Vec::new()
                };

                aggregate_disk_io_samples(&read_items, &write_items)
            }
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
}

#[cfg(target_os = "windows")]
pub use win_impl::WindowsDiskIoMonitor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_drive_key_strips_slashes_and_uppercases() {
        assert_eq!(normalize_drive_key(" c:\\ "), "C:");
        assert_eq!(normalize_drive_key("d:/"), "D:");
        assert_eq!(normalize_drive_key("_Total"), "_TOTAL");
        assert_eq!(normalize_drive_key("   "), "");
    }

    #[test]
    fn aggregate_disk_io_samples_converts_bytes_to_mib_and_filters_invalid() {
        let mib = 1024.0 * 1024.0;
        let reads = vec![
            ("c:\\".to_string(), 10.0 * mib),
            ("D:".to_string(), -5.0),
            ("".to_string(), 5.0 * mib),
        ];
        let writes = vec![
            ("C:".to_string(), 2.5 * mib),
            ("e:/".to_string(), 4.0 * mib),
        ];

        let map = aggregate_disk_io_samples(&reads, &writes);
        assert_eq!(map.len(), 2);
        let (cr, cw) = map["C:"];
        assert!((cr - 10.0).abs() < 1e-6);
        assert!((cw - 2.5).abs() < 1e-6);
        let (er, ew) = map["E:"];
        assert_eq!(er, 0.0);
        assert!((ew - 4.0).abs() < 1e-6);
    }
}
