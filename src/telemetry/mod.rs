use std::collections::VecDeque;
use std::time::Instant;
use sysinfo::{Components, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

#[cfg(target_os = "windows")]
mod gpu_win;
#[cfg(target_os = "windows")]
mod wmi_win;

const HISTORY_SIZE: usize = 12;

#[derive(Debug, Clone)]
pub struct CpuSnapshot {
    pub name: String,
    pub total_usage: f32, // 0.0 .. 1.0
    pub clock_ghz: f32,
    pub temperature_c: Option<f32>,
    pub history: Vec<f32>,
    pub core_usages: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    pub used_gb: f64,
    pub total_gb: f64,
    pub available_gb: f64,
    pub percent: f32, // 0.0 .. 1.0
    pub history: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct GpuSnapshot {
    pub detected: bool,
    pub name: String,
    pub usage: Option<f32>, // 0.0 .. 1.0
    pub clock_ghz: Option<f32>,
    pub vram_used_gb: f64,
    pub vram_total_gb: f64,
    pub vram_percent: Option<f32>, // 0.0 .. 1.0
    pub temperature_c: Option<f32>,
    pub fan_percent: Option<f32>, // 0.0 .. 1.0
    pub history: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct DiskSnapshot {
    pub name: String,
    pub mount: String,
    pub used_gb: f64,
    pub total_gb: f64,
    pub percent: f32,
    pub read_mb_s: f64,
    pub write_mb_s: f64,
}

#[derive(Debug, Clone)]
pub struct NetworkSnapshot {
    pub adapter_name: String,
    pub ip_address: String,
    pub external_ip: String,
    pub download_mbps: f64,
    pub upload_mbps: f64,
    pub history: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct TelemetrySnapshot {
    pub hostname: String,
    pub cpu: CpuSnapshot,
    pub mem: MemorySnapshot,
    pub gpu: GpuSnapshot,
    pub disks: Vec<DiskSnapshot>,
    pub net: NetworkSnapshot,
    pub uptime: String,
}

pub struct TelemetryEngine {
    sys: System,
    disks: Disks,
    networks: Networks,
    components: Components,
    hostname: String,
    external_ip: String,
    last_net_check: Instant,
    last_ext_ip_check: Instant,
    prev_rx_bytes: u64,
    prev_tx_bytes: u64,

    // Circular history buffers for UI sparklines
    cpu_history: VecDeque<f32>,
    ram_history: VecDeque<f32>,
    gpu_history: VecDeque<f32>,
    net_history: VecDeque<f32>,

    nvidia_smi_available: Option<bool>,
    #[cfg(target_os = "windows")]
    win_gpu: gpu_win::WindowsGpuMonitor,
    #[cfg(target_os = "windows")]
    win_wmi: wmi_win::WindowsWmiMonitor,
}

impl TelemetryEngine {
    pub fn new() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        )
        .or_else(|_| System::new())
        .unwrap();
        sys.refresh_cpu_all();
        sys.refresh_memory();

        let disks = Disks::new_with_refreshed_list()
            .or_else(|_| Disks::new())
            .unwrap_or_else(|_| Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing()).unwrap());

        let networks = Networks::new_with_refreshed_list()
            .or_else(|_| Networks::new())
            .unwrap_or_else(|_| Networks::new().unwrap());

        let components = Components::new_with_refreshed_list()
            .or_else(|_| Components::new())
            .unwrap_or_else(|_| Components::new().unwrap());

        let mut cpu_history = VecDeque::with_capacity(HISTORY_SIZE);
        let mut ram_history = VecDeque::with_capacity(HISTORY_SIZE);
        let mut gpu_history = VecDeque::with_capacity(HISTORY_SIZE);
        let mut net_history = VecDeque::with_capacity(HISTORY_SIZE);

        for _ in 0..HISTORY_SIZE {
            cpu_history.push_back(0.1);
            ram_history.push_back(0.2);
            gpu_history.push_back(0.05);
            net_history.push_back(0.05);
        }

        let hostname = System::host_name().unwrap_or_else(|_| "Meu Computador".to_string());

        Self {
            sys,
            disks,
            networks,
            components,
            hostname,
            external_ip: String::from("--"),
            last_net_check: Instant::now(),
            last_ext_ip_check: Instant::now().checked_sub(std::time::Duration::from_secs(3600)).unwrap_or_else(Instant::now),
            prev_rx_bytes: 0,
            prev_tx_bytes: 0,
            cpu_history,
            ram_history,
            gpu_history,
            net_history,
            nvidia_smi_available: None,
            #[cfg(target_os = "windows")]
            win_gpu: gpu_win::WindowsGpuMonitor::new(),
            #[cfg(target_os = "windows")]
            win_wmi: wmi_win::WindowsWmiMonitor::new(),
        }
    }

    pub fn snapshot(&mut self) -> TelemetrySnapshot {
        self.tick();

        // Refresh external IP periodically every 30 minutes
        if self.last_ext_ip_check.elapsed().as_secs() > 1800 || self.external_ip == "--" {
            self.last_ext_ip_check = Instant::now();
            let ext = fetch_external_ip();
            if ext != "--" {
                self.external_ip = ext;
            }
        }

        #[allow(unused_mut)]
        let mut cpu = self.sample_cpu();
        #[allow(unused_mut)]
        let mut gpu = self.sample_gpu();

        #[cfg(target_os = "windows")]
        {
            let need_cpu_temp = cpu.temperature_c.is_none();
            let need_gpu = gpu.temperature_c.is_none()
                || gpu.clock_ghz.is_none()
                || gpu.usage.is_none()
                || gpu.fan_percent.is_none();
            if need_cpu_temp || need_gpu {
                let wmi = self.win_wmi.sample(need_cpu_temp, need_gpu);
                if cpu.temperature_c.is_none() {
                    cpu.temperature_c = wmi.cpu_temp_c;
                }
                if gpu.temperature_c.is_none() {
                    gpu.temperature_c = wmi.gpu_temp_c;
                }
                if gpu.clock_ghz.is_none() {
                    gpu.clock_ghz = wmi.gpu_clock_ghz;
                }
                if gpu.usage.is_none() {
                    gpu.usage = wmi.gpu_usage;
                    if let Some(u) = gpu.usage {
                        if let Some(last) = gpu.history.last_mut() {
                            *last = u.clamp(0.0, 1.0);
                        }
                    }
                }
                if gpu.fan_percent.is_none() {
                    gpu.fan_percent = wmi.gpu_fan_percent;
                }
            }
        }

        TelemetrySnapshot {
            hostname: self.hostname.clone(),
            cpu,
            mem: self.sample_memory(),
            gpu,
            disks: self.sample_disks(),
            net: self.sample_network(),
            uptime: self.get_uptime_string(),
        }
    }

    pub fn tick(&mut self) {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.disks.refresh(true);
        self.networks.refresh(true);
        self.components.refresh(true);
    }

    pub fn get_uptime_string(&self) -> String {
        let secs = System::uptime().unwrap_or(0);
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    }

    pub fn sample_cpu(&mut self) -> CpuSnapshot {
        let cpus = self.sys.cpus();
        let total_usage = (self.sys.global_cpu_usage() / 100.0).clamp(0.0, 1.0);

        let name = if let Some(first) = cpus.first() {
            first.brand().trim().to_string()
        } else {
            "Processador".to_string()
        };

        let avg_freq_mhz = if !cpus.is_empty() {
            let sum: u64 = cpus.iter().map(|c| c.frequency()).sum();
            sum as f32 / cpus.len() as f32
        } else {
            0.0
        };

        // Try reading CPU temperature from components
        let mut cpu_temp: Option<f32> = None;
        for comp in &self.components {
            let label = comp.label().to_lowercase();
            if label.contains("cpu") || label.contains("core") || label.contains("package") || label.contains("tctl") {
                if let Some(t) = comp.temperature() {
                    cpu_temp = Some(t);
                    break;
                }
            }
        }

        self.cpu_history.pop_front();
        self.cpu_history.push_back(total_usage);

        let core_usages: Vec<f32> = cpus
            .iter()
            .map(|c| (c.usage() / 100.0).clamp(0.0, 1.0))
            .collect();

        CpuSnapshot {
            name,
            total_usage,
            clock_ghz: avg_freq_mhz / 1000.0,
            temperature_c: cpu_temp,
            history: self.cpu_history.iter().copied().collect(),
            core_usages,
        }
    }

    pub fn sample_memory(&mut self) -> MemorySnapshot {
        let total = self.sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let used = self.sys.used_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let available = self.sys.available_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let percent = if total > 0.0 { (used / total) as f32 } else { 0.0 };

        self.ram_history.pop_front();
        self.ram_history.push_back(percent.clamp(0.0, 1.0));

        MemorySnapshot {
            used_gb: used,
            total_gb: total,
            available_gb: available,
            percent,
            history: self.ram_history.iter().copied().collect(),
        }
    }

    pub fn sample_disks(&mut self) -> Vec<DiskSnapshot> {
        let mut results = Vec::new();
        for disk in &self.disks {
            let total = disk.total_space() as f64 / (1024.0 * 1024.0 * 1024.0);
            let available = disk.available_space() as f64 / (1024.0 * 1024.0 * 1024.0);
            let used = (total - available).max(0.0);
            let percent = if total > 0.0 { (used / total) as f32 } else { 0.0 };

            let name = disk.name().to_string_lossy().to_string();
            let mount = disk.mount_point().to_string_lossy().to_string();

            results.push(DiskSnapshot {
                name: if name.is_empty() { mount.clone() } else { name },
                mount,
                used_gb: used,
                total_gb: total,
                percent,
                read_mb_s: 0.0,
                write_mb_s: 0.0,
            });
        }
        results
    }

    pub fn sample_network(&mut self) -> NetworkSnapshot {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_net_check).as_secs_f64().max(0.1);
        self.last_net_check = now;

        let mut current_rx: u64 = 0;
        let mut current_tx: u64 = 0;
        let mut active_name = String::from("Rede");
        let mut ip_addr = String::from("127.0.0.1");

        for (interface_name, data) in &self.networks {
            let rx = data.total_received();
            let tx = data.total_transmitted();
            current_rx += rx;
            current_tx += tx;

            let name_str = interface_name.to_string_lossy();
            if !name_str.contains("Loopback") && !name_str.contains("vEthernet") {
                active_name = name_str.to_string();
                for ip in data.ip_networks() {
                    if ip.addr.is_ipv4() && !ip.addr.is_loopback() {
                        ip_addr = ip.addr.to_string();
                        break;
                    }
                }
            }
        }

        let delta_rx = if self.prev_rx_bytes > 0 && current_rx >= self.prev_rx_bytes {
            current_rx - self.prev_rx_bytes
        } else {
            0
        };

        let delta_tx = if self.prev_tx_bytes > 0 && current_tx >= self.prev_tx_bytes {
            current_tx - self.prev_tx_bytes
        } else {
            0
        };

        self.prev_rx_bytes = current_rx;
        self.prev_tx_bytes = current_tx;

        let download_mbps = (delta_rx as f64 * 8.0) / (elapsed * 1_000_000.0);
        let upload_mbps = (delta_tx as f64 * 8.0) / (elapsed * 1_000_000.0);

        let norm_net = ((download_mbps + upload_mbps) / 50.0).clamp(0.02, 1.0) as f32;
        self.net_history.pop_front();
        self.net_history.push_back(norm_net);

        NetworkSnapshot {
            adapter_name: active_name,
            ip_address: ip_addr,
            external_ip: self.external_ip.clone(),
            download_mbps,
            upload_mbps,
            history: self.net_history.iter().copied().collect(),
        }
    }

    pub fn sample_gpu(&mut self) -> GpuSnapshot {
        let mut name = String::from("GPU do Sistema");
        let mut usage: Option<f32> = None;
        let mut clock_ghz: Option<f32> = None;
        let mut vram_used_gb = 0.0;
        let mut vram_total_gb = 0.0;
        let mut vram_percent: Option<f32> = None;
        let mut temp: Option<f32> = None;
        let mut fan_percent: Option<f32> = None;

        #[cfg(target_os = "windows")]
        if let Some(win_sample) = self.win_gpu.sample() {
            if !win_sample.name.is_empty() {
                name = win_sample.name;
            }
            usage = win_sample.usage;
            clock_ghz = win_sample.clock_ghz;
            vram_used_gb = win_sample.vram_used_gb;
            vram_total_gb = win_sample.vram_total_gb;
            vram_percent = win_sample.vram_percent;
            temp = win_sample.temperature_c;
            fan_percent = win_sample.fan_percent;
        }

        // Fallback to nvidia-smi CLI if Windows NVML/ADL or Linux didn't fill full NVIDIA telemetry
        let needs_nvidia_cli = temp.is_none() || clock_ghz.is_none() || usage.is_none();
        if needs_nvidia_cli && self.nvidia_smi_available != Some(false) {
            if let Some(nv) = query_nvidia_smi(&mut self.nvidia_smi_available) {
                if !nv.name.is_empty() && name == "GPU do Sistema" {
                    name = nv.name;
                }
                if usage.is_none() {
                    usage = nv.usage;
                }
                if clock_ghz.is_none() {
                    clock_ghz = nv.clock_ghz;
                }
                if vram_percent.is_none() {
                    vram_used_gb = nv.vram_used_gb;
                    vram_total_gb = nv.vram_total_gb;
                    vram_percent = nv.vram_percent;
                }
                if temp.is_none() {
                    temp = nv.temperature_c;
                }
                if fan_percent.is_none() {
                    fan_percent = nv.fan_percent;
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        if usage.is_none() && temp.is_none() && clock_ghz.is_none() {
            if let Some(sysfs_gpu) = query_linux_drm_gpu() {
                if usage.is_none() {
                    usage = sysfs_gpu.usage;
                }
                if clock_ghz.is_none() {
                    clock_ghz = sysfs_gpu.clock_ghz;
                }
                if vram_percent.is_none() {
                    vram_used_gb = sysfs_gpu.vram_used_gb;
                    vram_total_gb = sysfs_gpu.vram_total_gb;
                    vram_percent = sysfs_gpu.vram_percent;
                }
                if temp.is_none() {
                    temp = sysfs_gpu.temperature_c;
                }
                if fan_percent.is_none() {
                    fan_percent = sysfs_gpu.fan_percent;
                }
            }
        }

        if temp.is_none() {
            for comp in &self.components {
                let label = comp.label().to_lowercase();
                if label.contains("gpu") || label.contains("graphics") || label.contains("amdgpu") {
                    if let Some(t) = comp.temperature() {
                        temp = Some(t);
                        break;
                    }
                }
            }
        }

        let hist_val = usage.unwrap_or(0.0).clamp(0.0, 1.0);
        self.gpu_history.pop_front();
        self.gpu_history.push_back(hist_val);

        GpuSnapshot {
            detected: true,
            name,
            usage,
            clock_ghz,
            vram_used_gb,
            vram_total_gb,
            vram_percent,
            temperature_c: temp,
            fan_percent,
            history: self.gpu_history.iter().copied().collect(),
        }
    }
}

struct RawGpuTelemetry {
    name: String,
    usage: Option<f32>,
    clock_ghz: Option<f32>,
    vram_used_gb: f64,
    vram_total_gb: f64,
    vram_percent: Option<f32>,
    temperature_c: Option<f32>,
    fan_percent: Option<f32>,
}

fn parse_optional_f32(s: &str) -> Option<f32> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.starts_with('[') || trimmed.eq_ignore_ascii_case("n/a") {
        return None;
    }
    trimmed.parse::<f32>().ok()
}

fn nvidia_smi_executable() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        for candidate in [
            "C:\\Windows\\System32\\nvidia-smi.exe",
            "C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvidia-smi.exe",
        ] {
            if std::path::Path::new(candidate).is_file() {
                return candidate;
            }
        }
    }
    "nvidia-smi"
}

fn query_nvidia_smi(available_cache: &mut Option<bool>) -> Option<RawGpuTelemetry> {
    let mut cmd = std::process::Command::new(nvidia_smi_executable());
    cmd.args([
        "--query-gpu=name,utilization.gpu,clocks.current.graphics,memory.used,memory.total,temperature.gpu,fan.speed",
        "--format=csv,noheader,nounits",
    ]);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match cmd.output() {
        Ok(out) => {
            if !out.status.success() {
                if available_cache.is_none() {
                    *available_cache = Some(false);
                }
                return None;
            }
            *available_cache = Some(true);
            out
        }
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                *available_cache = Some(false);
            }
            return None;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }

    let cols: Vec<&str> = line.split(',').collect();
    if cols.len() < 7 {
        return None;
    }

    let name = cols[0].trim().to_string();
    let usage = parse_optional_f32(cols[1]).map(|v| (v / 100.0).clamp(0.0, 1.0));
    let clock_ghz = parse_optional_f32(cols[2]).map(|mhz| mhz / 1000.0);
    let vram_used_mib = parse_optional_f32(cols[3]).map(|v| v as f64).unwrap_or(0.0);
    let vram_total_mib = parse_optional_f32(cols[4]).map(|v| v as f64).unwrap_or(0.0);
    let vram_used_gb = vram_used_mib / 1024.0;
    let vram_total_gb = vram_total_mib / 1024.0;
    let vram_percent = if vram_total_mib > 0.0 {
        Some((vram_used_mib / vram_total_mib) as f32)
    } else {
        None
    };
    let temperature_c = parse_optional_f32(cols[5]);
    let fan_percent = parse_optional_f32(cols[6]).map(|v| (v / 100.0).clamp(0.0, 1.0));

    Some(RawGpuTelemetry {
        name,
        usage,
        clock_ghz,
        vram_used_gb,
        vram_total_gb,
        vram_percent,
        temperature_c,
        fan_percent,
    })
}

#[cfg(not(target_os = "windows"))]
fn query_linux_drm_gpu() -> Option<RawGpuTelemetry> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let dev_path = entry.path().join("device");
        if !dev_path.exists() {
            continue;
        }

        let usage = std::fs::read_to_string(dev_path.join("gpu_busy_percent"))
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .map(|v| (v / 100.0).clamp(0.0, 1.0));

        let vram_used_bytes = std::fs::read_to_string(dev_path.join("mem_info_vram_used"))
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok());
        let vram_total_bytes = std::fs::read_to_string(dev_path.join("mem_info_vram_total"))
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok());

        let (vram_used_gb, vram_total_gb, vram_percent) = match (vram_used_bytes, vram_total_bytes) {
            (Some(u), Some(t)) if t > 0.0 => (
                u / (1024.0 * 1024.0 * 1024.0),
                t / (1024.0 * 1024.0 * 1024.0),
                Some((u / t) as f32),
            ),
            _ => (0.0, 0.0, None),
        };

        let mut temperature_c = None;
        let mut clock_ghz = None;
        let mut fan_percent = None;

        if let Ok(hwmons) = std::fs::read_dir(dev_path.join("hwmon")) {
            for hw in hwmons.flatten() {
                let hp = hw.path();
                if temperature_c.is_none() {
                    temperature_c = std::fs::read_to_string(hp.join("temp1_input"))
                        .ok()
                        .and_then(|s| s.trim().parse::<f32>().ok())
                        .map(|mc| mc / 1000.0);
                }
                if clock_ghz.is_none() {
                    clock_ghz = std::fs::read_to_string(hp.join("freq1_input"))
                        .ok()
                        .and_then(|s| s.trim().parse::<f32>().ok())
                        .map(|hz| hz / 1_000_000_000.0);
                }
                if fan_percent.is_none() {
                    fan_percent = std::fs::read_to_string(hp.join("pwm1"))
                        .ok()
                        .and_then(|s| s.trim().parse::<f32>().ok())
                        .map(|pwm| (pwm / 255.0).clamp(0.0, 1.0));
                }
            }
        }

        if usage.is_some() || temperature_c.is_some() || vram_percent.is_some() || clock_ghz.is_some() {
            return Some(RawGpuTelemetry {
                name: String::new(),
                usage,
                clock_ghz,
                vram_used_gb,
                vram_total_gb,
                vram_percent,
                temperature_c,
                fan_percent,
            });
        }
    }
    None
}

fn fetch_external_ip() -> String {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    if let Ok(mut stream) = TcpStream::connect_timeout(
        &"64.185.227.156:80".parse().unwrap(),
        Duration::from_millis(1500),
    ) {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
        let _ = stream.write_all(b"GET / HTTP/1.1\r\nHost: api.ipify.org\r\nUser-Agent: ZenithBar\r\nConnection: close\r\n\r\n");
        let mut resp = String::new();
        if stream.read_to_string(&mut resp).is_ok() {
            if let Some(body) = resp.split("\r\n\r\n").nth(1) {
                let ip = body.trim().to_string();
                if !ip.is_empty() && ip.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ':') {
                    return ip;
                }
            }
        }
    }
    String::from("--")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_engine_avoids_process_list_and_bounds_histories() {
        let mut engine = TelemetryEngine::new();
        assert!(
            engine.sys.processes().is_empty(),
            "System should not allocate OS process table"
        );

        // Pre-seed external IP and disable nvidia-smi probing to keep test deterministic and fast
        engine.external_ip = "127.0.0.1".to_string();
        engine.last_ext_ip_check = Instant::now();
        engine.nvidia_smi_available = Some(false);

        for _ in 0..500 {
            let snap = engine.snapshot();
            assert_eq!(snap.cpu.history.len(), HISTORY_SIZE);
            assert_eq!(snap.mem.history.len(), HISTORY_SIZE);
            assert_eq!(snap.gpu.history.len(), HISTORY_SIZE);
            assert_eq!(snap.net.history.len(), HISTORY_SIZE);
        }
    }
}
