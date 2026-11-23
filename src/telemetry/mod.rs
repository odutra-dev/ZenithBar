use std::collections::VecDeque;
use std::time::Instant;
use sysinfo::{Components, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

mod disk_win;
mod gpu_win;
pub mod pawnio_win;
mod shm_win;
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
    ext_ip_rx: Option<std::sync::mpsc::Receiver<String>>,
    last_net_check: Instant,
    last_disk_check: Instant,
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
    win_disk: Option<disk_win::WindowsDiskIoMonitor>,
    #[cfg(target_os = "windows")]
    win_gpu: gpu_win::WindowsGpuMonitor,
    #[cfg(target_os = "windows")]
    win_shm: shm_win::WindowsShmMonitor,
    #[cfg(target_os = "windows")]
    win_wmi: wmi_win::WindowsWmiMonitor,
    #[cfg(target_os = "windows")]
    win_pawnio: pawnio_win::PawnIoCpuReader,
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
            ext_ip_rx: None,
            last_net_check: Instant::now(),
            last_disk_check: Instant::now(),
            last_ext_ip_check: Instant::now().checked_sub(std::time::Duration::from_secs(3600)).unwrap_or_else(Instant::now),
            prev_rx_bytes: 0,
            prev_tx_bytes: 0,
            cpu_history,
            ram_history,
            gpu_history,
            net_history,
            nvidia_smi_available: None,
            #[cfg(target_os = "windows")]
            win_disk: disk_win::WindowsDiskIoMonitor::new(),
            #[cfg(target_os = "windows")]
            win_gpu: gpu_win::WindowsGpuMonitor::new(),
            #[cfg(target_os = "windows")]
            win_shm: shm_win::WindowsShmMonitor::new(),
            #[cfg(target_os = "windows")]
            win_wmi: wmi_win::WindowsWmiMonitor::new(),
            #[cfg(target_os = "windows")]
            win_pawnio: pawnio_win::PawnIoCpuReader::new(),
        }
    }

    pub fn reload_sensors(&mut self) {
        self.disks = Disks::new_with_refreshed_list()
            .or_else(|_| Disks::new())
            .unwrap_or_else(|_| Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing()).unwrap());
        self.networks = Networks::new_with_refreshed_list()
            .or_else(|_| Networks::new())
            .unwrap_or_else(|_| Networks::new().unwrap());
        self.components = Components::new_with_refreshed_list()
            .or_else(|_| Components::new())
            .unwrap_or_else(|_| Components::new().unwrap());
        self.nvidia_smi_available = None;
        self.external_ip = String::from("--");
        self.ext_ip_rx = None;
        self.last_ext_ip_check = Instant::now()
            .checked_sub(std::time::Duration::from_secs(3600))
            .unwrap_or_else(Instant::now);
        #[cfg(target_os = "windows")]
        {
            self.win_disk = disk_win::WindowsDiskIoMonitor::new();
            self.win_gpu = gpu_win::WindowsGpuMonitor::new();
            self.win_shm = shm_win::WindowsShmMonitor::new();
            self.win_wmi = wmi_win::WindowsWmiMonitor::new();
            self.win_pawnio = pawnio_win::PawnIoCpuReader::new();
        }
    }

    pub fn snapshot(&mut self) -> TelemetrySnapshot {
        self.tick();

        if let Some(rx) = self.ext_ip_rx.as_ref() {
            match rx.try_recv() {
                Ok(ext) => {
                    if ext != "--" {
                        self.external_ip = ext;
                    }
                    self.ext_ip_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.ext_ip_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        // Refresh external IP asynchronously every 30 minutes (or retry every 60s when unknown)
        let elapsed_ext = self.last_ext_ip_check.elapsed().as_secs();
        let should_refresh_ext = elapsed_ext > 1800 || (self.external_ip == "--" && elapsed_ext > 60);
        if should_refresh_ext && self.ext_ip_rx.is_none() {
            self.last_ext_ip_check = Instant::now();
            let (tx, rx) = std::sync::mpsc::channel();
            self.ext_ip_rx = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(fetch_external_ip());
            });
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
                let shm = self.win_shm.sample(need_cpu_temp, need_gpu);
                if cpu.temperature_c.is_none() {
                    cpu.temperature_c = shm.cpu_temp_c;
                }
                if gpu.temperature_c.is_none() {
                    gpu.temperature_c = shm.gpu_temp_c;
                }
                if gpu.clock_ghz.is_none() {
                    gpu.clock_ghz = shm.gpu_clock_ghz;
                }
                if gpu.usage.is_none() {
                    gpu.usage = shm.gpu_usage;
                    if let Some(u) = gpu.usage {
                        if let Some(last) = gpu.history.last_mut() {
                            *last = u.clamp(0.0, 1.0);
                        }
                    }
                }
                if gpu.fan_percent.is_none() {
                    gpu.fan_percent = shm.gpu_fan_percent;
                }
            }

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

            if cpu.temperature_c.is_none() {
                cpu.temperature_c = self.win_pawnio.sample_cpu_temp();
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

        // Try reading CPU temperature from components (and Linux sysfs fallback)
        #[allow(unused_mut)]
        let mut cpu_candidates: Vec<(String, Option<f32>)> = self
            .components
            .iter()
            .map(|c| (c.label().to_string(), c.temperature()))
            .collect();
        #[cfg(not(target_os = "windows"))]
        cpu_candidates.extend(collect_linux_sysfs_cpu_temps_from(std::path::Path::new("/sys/class")));
        let cpu_temp = select_cpu_temperature(
            cpu_candidates.iter().map(|(l, t)| (l.as_str(), *t)),
        );

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
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_disk_check).as_secs_f64().max(0.1);
        self.last_disk_check = now;

        #[cfg(target_os = "windows")]
        let win_io = self.win_disk.as_mut().map(|d| d.sample()).unwrap_or_default();

        let mut results = Vec::new();
        for disk in &self.disks {
            let total = disk.total_space() as f64 / (1024.0 * 1024.0 * 1024.0);
            let available = disk.available_space() as f64 / (1024.0 * 1024.0 * 1024.0);
            let used = (total - available).max(0.0);
            let percent = if total > 0.0 { (used / total) as f32 } else { 0.0 };

            let name = disk.name().to_string_lossy().to_string();
            let mount = disk.mount_point().to_string_lossy().to_string();

            #[cfg(target_os = "windows")]
            let (read_mb_s, write_mb_s) = {
                let key = disk_win::normalize_drive_key(&mount);
                win_io.get(&key).copied().unwrap_or((0.0, 0.0))
            };

            #[cfg(not(target_os = "windows"))]
            let (read_mb_s, write_mb_s) = {
                let io = disk.usage();
                const MIB: f64 = 1024.0 * 1024.0;
                (
                    io.read_bytes as f64 / (elapsed * MIB),
                    io.written_bytes as f64 / (elapsed * MIB),
                )
            };

            results.push(DiskSnapshot {
                name: if name.is_empty() { mount.clone() } else { name },
                mount,
                used_gb: used,
                total_gb: total,
                percent,
                read_mb_s,
                write_mb_s,
            });
        }
        #[cfg(not(target_os = "windows"))]
        let _ = elapsed;
        #[cfg(target_os = "windows")]
        let _ = elapsed;

        results.sort_by_key(|a| a.mount.to_ascii_lowercase());
        results
    }

    pub fn sample_network(&mut self) -> NetworkSnapshot {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_net_check).as_secs_f64().max(0.1);
        self.last_net_check = now;

        let candidates: Vec<NetCandidate> = self
            .networks
            .iter()
            .map(|(interface_name, data)| {
                let ipv4 = data
                    .ip_networks()
                    .iter()
                    .find(|ip| ip.addr.is_ipv4() && !ip.addr.is_loopback())
                    .map(|ip| ip.addr.to_string());
                NetCandidate {
                    name: interface_name.to_string_lossy().to_string(),
                    ipv4,
                    rx_bytes: data.total_received(),
                    tx_bytes: data.total_transmitted(),
                }
            })
            .collect();

        let (active_name, ip_addr, current_rx, current_tx) = select_primary_network(&candidates);

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
        if needs_nvidia_cli && self.nvidia_smi_available != Some(false)
            && let Some(nv) = query_nvidia_smi(&mut self.nvidia_smi_available) {
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

        #[cfg(not(target_os = "windows"))]
        if usage.is_none() && temp.is_none() && clock_ghz.is_none()
            && let Some(sysfs_gpu) = query_linux_drm_gpu() {
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

        if temp.is_none() {
            for comp in &self.components {
                let label = comp.label().to_lowercase();
                if (label.contains("gpu") || label.contains("graphics") || label.contains("amdgpu"))
                    && let Some(t) = comp.temperature() {
                        temp = Some(t);
                        break;
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

fn nvidia_smi_executable() -> String {
    #[cfg(target_os = "windows")]
    {
        for candidate in [
            "C:\\Windows\\System32\\nvidia-smi.exe",
            "C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvidia-smi.exe",
        ] {
            if std::path::Path::new(candidate).is_file() {
                return candidate.to_string();
            }
        }
        if let Some(found) = gpu_win::find_nvidia_driverstore_file("nvidia-smi.exe") {
            return found;
        }
    }
    "nvidia-smi".to_string()
}

fn parse_nvidia_smi_csv_line(line: &str) -> Option<RawGpuTelemetry> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let cols: Vec<&str> = trimmed.split(',').collect();
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
        Some((vram_used_mib / vram_total_mib).clamp(0.0, 1.0) as f32)
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
    let line = stdout.lines().next()?;
    parse_nvidia_smi_csv_line(line)
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

struct NetCandidate {
    name: String,
    ipv4: Option<String>,
    rx_bytes: u64,
    tx_bytes: u64,
}

fn is_loopback_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l == "lo" || l.contains("loopback")
}

fn is_virtual_interface(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    [
        "loopback",
        "vethernet",
        "hyper-v",
        "vmware",
        "vbox",
        "virtual",
        "wsl",
        "docker",
        "br-",
        "veth",
        "tailscale",
        "zerotier",
        "teredo",
        "isatap",
        "pseudo",
        "tun",
        "tap",
    ]
    .iter()
    .any(|kw| l.contains(kw))
        || l == "lo"
}

fn select_primary_network(candidates: &[NetCandidate]) -> (String, String, u64, u64) {
    let has_physical = candidates
        .iter()
        .any(|c| !is_virtual_interface(&c.name) && c.ipv4.is_some());

    let mut total_rx: u64 = 0;
    let mut total_tx: u64 = 0;
    for c in candidates {
        if is_loopback_name(&c.name) {
            continue;
        }
        if has_physical && is_virtual_interface(&c.name) {
            continue;
        }
        total_rx = total_rx.saturating_add(c.rx_bytes);
        total_tx = total_tx.saturating_add(c.tx_bytes);
    }

    let best = candidates
        .iter()
        .filter(|c| !is_loopback_name(&c.name))
        .max_by(|a, b| {
            let key_a = (
                !is_virtual_interface(&a.name),
                a.ipv4.is_some(),
                a.rx_bytes.saturating_add(a.tx_bytes),
            );
            let key_b = (
                !is_virtual_interface(&b.name),
                b.ipv4.is_some(),
                b.rx_bytes.saturating_add(b.tx_bytes),
            );
            key_a.cmp(&key_b).then_with(|| b.name.cmp(&a.name))
        });

    match best {
        Some(b) => (
            b.name.clone(),
            b.ipv4.clone().unwrap_or_else(|| "127.0.0.1".to_string()),
            total_rx,
            total_tx,
        ),
        None => ("Rede".to_string(), "127.0.0.1".to_string(), total_rx, total_tx),
    }
}

fn classify_cpu_sensor_priority(label: &str) -> Option<u8> {
    let l = label.trim().to_ascii_lowercase();
    if l.is_empty() {
        return None;
    }

    // Reject non-CPU thermal sensors that may contain "core", "package", or generic words
    if l.contains("gpu")
        || l.contains("graphics")
        || l.contains("amdgpu")
        || l.contains("radeon")
        || l.contains("nvidia")
        || l.contains("nouveau")
        || l.contains("nvme")
        || l.contains("ssd")
        || l.contains("hdd")
        || l.contains("drivetemp")
        || l.contains("wifi")
        || l.contains("iwlwifi")
        || l.contains("wlan")
        || l.contains("battery")
        || l.contains("bat")
        || l.contains("pch")
        || l.contains("dimm")
        || l.contains("spd")
        || l.contains("vrm")
        || l.contains("mos")
        || l.contains("ambient")
        || l.contains("coolant")
        || l.contains("water")
        || l.contains("composite")
        || l == "computer"
    {
        return None;
    }

    if l.contains("tctl/tdie")
        || l.contains("package id")
        || l.contains("cpu package")
        || l.contains("x86_pkg_temp")
        || l.contains("tdie")
    {
        Some(0)
    } else if l.contains("k10temp")
        || l.contains("zenpower")
        || l.contains("coretemp")
        || l.contains("tccd")
        || l.contains("ccd1")
        || l.contains("ccd2")
        || l.contains("cpu_thermal")
        || l.contains("cpu-thermal")
        || l.contains("soc_thermal")
        || l.contains("soc-thermal")
        || l.contains("cpu die")
        || l.contains("cputin")
    {
        Some(1)
    } else if l.contains("tctl")
        || l.contains("package")
        || l.contains("cpu")
        || l.contains("core")
    {
        Some(2)
    } else if l.contains("acpitz") {
        Some(3)
    } else {
        None
    }
}

fn select_cpu_temperature<'a, I>(readings: I) -> Option<f32>
where
    I: IntoIterator<Item = (&'a str, Option<f32>)>,
{
    let mut best: Option<(u8, f32)> = None;
    for (label, temp_opt) in readings {
        let Some(t) = temp_opt else { continue };
        if !t.is_finite() || t <= 0.0 || t >= 130.0 {
            continue;
        }
        let Some(prio) = classify_cpu_sensor_priority(label) else {
            continue;
        };
        match best {
            None => best = Some((prio, t)),
            Some((best_prio, _)) if prio < best_prio => best = Some((prio, t)),
            _ => {}
        }
    }
    best.map(|(_, t)| t)
}

#[cfg(any(not(target_os = "windows"), test))]
fn collect_linux_sysfs_cpu_temps_from(sys_class: &std::path::Path) -> Vec<(String, Option<f32>)> {
    let mut out = Vec::new();

    if let Ok(hwmon_dirs) = std::fs::read_dir(sys_class.join("hwmon")) {
        for entry in hwmon_dirs.flatten() {
            let dir = entry.path();
            let chip_name = std::fs::read_to_string(dir.join("name"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if chip_name.is_empty() {
                continue;
            }
            if let Ok(files) = std::fs::read_dir(&dir) {
                for f in files.flatten() {
                    let fname = f.file_name().to_string_lossy().to_string();
                    if let Some(rest) = fname.strip_prefix("temp")
                        && let Some(id) = rest.strip_suffix("_input")
                            && let Ok(raw_mc) = std::fs::read_to_string(f.path())
                                .unwrap_or_default()
                                .trim()
                                .parse::<i32>()
                            {
                                let temp_label = std::fs::read_to_string(dir.join(format!("temp{}_label", id)))
                                    .map(|s| s.trim().to_string())
                                    .unwrap_or_default();
                                let full_label = if temp_label.is_empty() {
                                    format!("{} temp{}", chip_name, id)
                                } else {
                                    format!("{} {}", chip_name, temp_label)
                                };
                                out.push((full_label, Some(raw_mc as f32 / 1000.0)));
                            }
                }
            }
        }
    }

    if let Ok(tz_dirs) = std::fs::read_dir(sys_class.join("thermal")) {
        for entry in tz_dirs.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("thermal_zone") {
                continue;
            }
            let dir = entry.path();
            let tz_type = std::fs::read_to_string(dir.join("type"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if tz_type.is_empty() {
                continue;
            }
            if let Ok(raw_mc) = std::fs::read_to_string(dir.join("temp"))
                .unwrap_or_default()
                .trim()
                .parse::<i32>()
            {
                out.push((tz_type, Some(raw_mc as f32 / 1000.0)));
            }
        }
    }

    out
}

fn parse_external_ip_body(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.parse::<std::net::IpAddr>().is_ok() {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn fetch_external_ip() -> String {
    use std::time::Duration;

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();

    if let Ok(resp) = agent
        .get("https://api.ipify.org")
        .set("User-Agent", "ZenithBar")
        .call()
        && let Ok(body) = resp.into_string()
            && let Some(ip) = parse_external_ip_body(&body) {
                return ip;
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

    #[test]
    fn select_primary_network_prefers_physical_ipv4_deterministically() {
        let candidates = vec![
            NetCandidate {
                name: "vEthernet (WSL)".to_string(),
                ipv4: Some("10.240.128.128".to_string()),
                rx_bytes: 999_999,
                tx_bytes: 999_999,
            },
            NetCandidate {
                name: "Ethernet".to_string(),
                ipv4: Some("192.168.15.5".to_string()),
                rx_bytes: 50_000,
                tx_bytes: 10_000,
            },
            NetCandidate {
                name: "Loopback Pseudo-Interface 1".to_string(),
                ipv4: Some("127.0.0.1".to_string()),
                rx_bytes: 1_000_000,
                tx_bytes: 1_000_000,
            },
        ];
        let (name, ip, rx, tx) = select_primary_network(&candidates);
        assert_eq!(name, "Ethernet");
        assert_eq!(ip, "192.168.15.5");
        assert_eq!(rx, 50_000);
        assert_eq!(tx, 10_000);
    }

    #[test]
    fn parse_optional_f32_handles_na_brackets_and_numbers() {
        assert_eq!(parse_optional_f32(""), None);
        assert_eq!(parse_optional_f32("  N/A "), None);
        assert_eq!(parse_optional_f32("[Not Supported]"), None);
        assert_eq!(parse_optional_f32(" 63.5 "), Some(63.5));
    }

    #[test]
    fn parse_nvidia_smi_csv_line_parses_metrics_and_missing_fields() {
        let parsed = parse_nvidia_smi_csv_line(
            "NVIDIA GeForce RTX 4070, 35, 2550, 2048, 12288, 52, [Not Supported]",
        )
        .expect("valid csv line must parse");

        assert_eq!(parsed.name, "NVIDIA GeForce RTX 4070");
        assert!((parsed.usage.unwrap() - 0.35).abs() < 1e-4);
        assert!((parsed.clock_ghz.unwrap() - 2.55).abs() < 1e-4);
        assert!((parsed.vram_used_gb - 2.0).abs() < 1e-6);
        assert!((parsed.vram_total_gb - 12.0).abs() < 1e-6);
        assert!((parsed.vram_percent.unwrap() - (2048.0 / 12288.0)).abs() < 1e-4);
        assert_eq!(parsed.temperature_c, Some(52.0));
        assert_eq!(parsed.fan_percent, None);

        assert!(parse_nvidia_smi_csv_line("").is_none());
        assert!(parse_nvidia_smi_csv_line("only,two,cols").is_none());
    }

    #[test]
    fn network_helpers_handle_virtual_fallback_and_empty_candidates() {
        assert!(is_loopback_name("lo"));
        assert!(is_loopback_name("Loopback Pseudo-Interface 1"));
        assert!(!is_loopback_name("eth0"));

        assert!(is_virtual_interface("docker0"));
        assert!(is_virtual_interface("Tailscale"));
        assert!(!is_virtual_interface("Wi-Fi"));

        let empty = select_primary_network(&[]);
        assert_eq!(empty, ("Rede".to_string(), "127.0.0.1".to_string(), 0, 0));

        // When no physical adapter has IPv4, falls back to virtual non-loopback
        let virtual_only = vec![
            NetCandidate {
                name: "lo".to_string(),
                ipv4: Some("127.0.0.1".to_string()),
                rx_bytes: 999,
                tx_bytes: 999,
            },
            NetCandidate {
                name: "vEthernet (Default Switch)".to_string(),
                ipv4: Some("172.20.0.1".to_string()),
                rx_bytes: 1200,
                tx_bytes: 300,
            },
        ];
        let (name, ip, rx, tx) = select_primary_network(&virtual_only);
        assert_eq!(name, "vEthernet (Default Switch)");
        assert_eq!(ip, "172.20.0.1");
        assert_eq!(rx, 1200);
        assert_eq!(tx, 300);
    }

    #[test]
    fn snapshot_formats_uptime_and_sorts_disks() {
        let mut engine = TelemetryEngine::new();
        engine.external_ip = "127.0.0.1".to_string();
        engine.last_ext_ip_check = Instant::now();
        engine.nvidia_smi_available = Some(false);

        let snap = engine.snapshot();
        assert!(snap.uptime.contains('h') && snap.uptime.contains('m'));
        assert!(snap.mem.percent >= 0.0 && snap.mem.percent <= 1.0);
        assert!(!snap.cpu.name.is_empty());
        for w in snap.disks.windows(2) {
            assert!(w[0].mount.to_ascii_lowercase() <= w[1].mount.to_ascii_lowercase());
        }
    }

    #[test]
    fn select_cpu_temperature_prioritizes_cpu_sensors_and_filters_non_cpu_or_invalid() {
        let readings = [
            ("GPU Core", Some(52.0)),
            ("Computer", Some(40.0)),
            ("acpitz temp1", Some(43.0)),
            ("coretemp Core 0", Some(58.0)),
            ("coretemp Package id 0", Some(63.0)),
            ("k10temp Tdie", Some(0.0)),
        ];
        assert_eq!(select_cpu_temperature(readings), Some(63.0));

        // Supports AMD k10temp/zenpower without Tctl keyword (e.g. Tdie, Tccd1, or raw k10temp temp1)
        assert_eq!(
            select_cpu_temperature([("k10temp temp1", Some(54.5))]),
            Some(54.5)
        );
        assert_eq!(
            select_cpu_temperature([("zenpower Tdie", Some(61.25))]),
            Some(61.25)
        );
        assert_eq!(
            select_cpu_temperature([("nvme Composite", Some(39.0))]),
            None
        );
    }

    #[test]
    fn collect_linux_sysfs_cpu_temps_reads_hwmon_and_thermal_zones() {
        let dir = std::env::temp_dir().join(format!("zenithbar_sysfs_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let hwmon0 = dir.join("hwmon/hwmon0");
        let hwmon1 = dir.join("hwmon/hwmon1");
        let tz0 = dir.join("thermal/thermal_zone0");
        std::fs::create_dir_all(&hwmon0).unwrap();
        std::fs::create_dir_all(&hwmon1).unwrap();
        std::fs::create_dir_all(&tz0).unwrap();

        std::fs::write(hwmon0.join("name"), "nvme\n").unwrap();
        std::fs::write(hwmon0.join("temp1_label"), "Composite\n").unwrap();
        std::fs::write(hwmon0.join("temp1_input"), "38000\n").unwrap();

        std::fs::write(hwmon1.join("name"), "k10temp\n").unwrap();
        std::fs::write(hwmon1.join("temp1_input"), "56250\n").unwrap();

        std::fs::write(tz0.join("type"), "x86_pkg_temp\n").unwrap();
        std::fs::write(tz0.join("temp"), "61000\n").unwrap();

        let collected = collect_linux_sysfs_cpu_temps_from(&dir);
        let selected = select_cpu_temperature(collected.iter().map(|(l, t)| (l.as_str(), *t)));
        assert_eq!(selected, Some(61.0));

        // Remove thermal zone so hwmon k10temp without label is selected
        let _ = std::fs::remove_dir_all(&tz0);
        let collected = collect_linux_sysfs_cpu_temps_from(&dir);
        let selected = select_cpu_temperature(collected.iter().map(|(l, t)| (l.as_str(), *t)));
        assert_eq!(selected, Some(56.25));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_external_ip_body_validates_ip_addresses() {
        assert_eq!(
            parse_external_ip_body(" 203.0.113.42 \n"),
            Some("203.0.113.42".to_string())
        );
        assert_eq!(
            parse_external_ip_body("2001:db8::1"),
            Some("2001:db8::1".to_string())
        );
        assert_eq!(parse_external_ip_body("<html>error</html>"), None);
        assert_eq!(parse_external_ip_body(""), None);
    }
}
