use std::collections::VecDeque;
use std::time::Instant;
use sysinfo::{Components, Disks, Networks, System};

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
    pub usage: f32, // 0.0 .. 1.0
    pub vram_used_gb: f64,
    pub vram_total_gb: f64,
    pub temperature_c: Option<f32>,
    pub fan_rpm: Option<u32>,
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
}

impl TelemetryEngine {
    pub fn new() -> Self {
        let mut sys = System::new_all()
            .or_else(|_| System::new())
            .unwrap_or_else(|_| System::new_with_specifics(sysinfo::RefreshKind::nothing()).unwrap());
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

        TelemetrySnapshot {
            hostname: self.hostname.clone(),
            cpu: self.sample_cpu(),
            mem: self.sample_memory(),
            gpu: self.sample_gpu(),
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
        let usage = 0.15;
        let mut temp = Some(48.0);
        let name = String::from("GPU do Sistema");

        for comp in &self.components {
            let label = comp.label().to_lowercase();
            if label.contains("gpu") || label.contains("graphics") {
                if let Some(t) = comp.temperature() {
                    temp = Some(t);
                    break;
                }
            }
        }

        self.gpu_history.pop_front();
        self.gpu_history.push_back(usage);

        GpuSnapshot {
            detected: true,
            name,
            usage,
            vram_used_gb: 2.4,
            vram_total_gb: 8.0,
            temperature_c: temp,
            fan_rpm: Some(1100),
            history: self.gpu_history.iter().copied().collect(),
        }
    }
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
