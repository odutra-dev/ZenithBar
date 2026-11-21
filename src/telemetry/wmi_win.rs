use windows::core::{w, BSTR, PCWSTR};
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BSTR, VT_I4, VT_R4, VT_UI4};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};

#[derive(Default)]
pub struct WmiSensorSample {
    pub cpu_temp_c: Option<f32>,
    pub gpu_temp_c: Option<f32>,
    pub gpu_clock_ghz: Option<f32>,
    pub gpu_usage: Option<f32>,
    pub gpu_fan_percent: Option<f32>,
}

struct VariantGuard(VARIANT);

impl VariantGuard {
    fn new() -> Self {
        Self(VARIANT::default())
    }
}

impl Drop for VariantGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}

pub struct WindowsWmiMonitor {
    com_owned: bool,
    locator: Option<IWbemLocator>,
    lhm_services: Option<IWbemServices>,
    lhm_checked: bool,
    cim_services: Option<IWbemServices>,
    cim_checked: bool,
    cim_disabled: bool,
    acpi_services: Option<IWbemServices>,
    acpi_checked: bool,
    acpi_disabled: bool,
}

impl WindowsWmiMonitor {
    pub fn new() -> Self {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            let com_owned = hr == S_OK || hr == S_FALSE;
            let com_ready = com_owned || hr == RPC_E_CHANGED_MODE;

            let locator = if com_ready {
                CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()
            } else {
                None
            };

            Self {
                com_owned,
                locator,
                lhm_services: None,
                lhm_checked: false,
                cim_services: None,
                cim_checked: false,
                cim_disabled: false,
                acpi_services: None,
                acpi_checked: false,
                acpi_disabled: false,
            }
        }
    }

    pub fn sample(&mut self, need_cpu_temp: bool, need_gpu_sensors: bool) -> WmiSensorSample {
        let mut out = WmiSensorSample::default();
        if !need_cpu_temp && !need_gpu_sensors {
            return out;
        }

        if !self.lhm_checked {
            self.lhm_checked = true;
            self.lhm_services = self
                .connect_namespace("root\\LibreHardwareMonitor")
                .or_else(|| self.connect_namespace("root\\OpenHardwareMonitor"));
            if self.lhm_services.is_some() {
                crate::log_msg("[WMI Win] Namespace LibreHardwareMonitor/OpenHardwareMonitor detectado.");
            }
        }

        if let Some(svc) = self.lhm_services.as_ref() {
            query_lhm_sensors(svc, &mut out);
        }

        if need_cpu_temp && out.cpu_temp_c.is_none() && !self.cim_disabled {
            if !self.cim_checked {
                self.cim_checked = true;
                self.cim_services = self.connect_namespace("root\\CIMV2");
                if self.cim_services.is_none() {
                    self.cim_disabled = true;
                }
            }
            if let Some(svc) = self.cim_services.as_ref() {
                match query_cimv2_thermal_zone(svc) {
                    Ok(Some(temp)) => out.cpu_temp_c = Some(temp),
                    Ok(None) => {
                        self.cim_disabled = true;
                        crate::log_msg("[WMI Win] Win32_PerfFormattedData_Counters_ThermalZoneInformation sem sensores térmicos válidos.");
                    }
                    Err(err_hr) => {
                        self.cim_disabled = true;
                        crate::log_msg(&format!(
                            "[WMI Win] Win32_PerfFormattedData_Counters_ThermalZoneInformation indisponível (hr=0x{:08X}).",
                            err_hr as u32
                        ));
                    }
                }
            }
        }

        if need_cpu_temp && out.cpu_temp_c.is_none() && !self.acpi_disabled {
            if !self.acpi_checked {
                self.acpi_checked = true;
                self.acpi_services = self.connect_namespace("root\\WMI");
                if self.acpi_services.is_none() {
                    self.acpi_disabled = true;
                }
            }
            if let Some(svc) = self.acpi_services.as_ref() {
                match query_acpi_thermal_zone(svc) {
                    Ok(temp) => out.cpu_temp_c = temp,
                    Err(err_hr) => {
                        self.acpi_disabled = true;
                        crate::log_msg(&format!(
                            "[WMI Win] MSAcpi_ThermalZoneTemperature indisponível sem admin (hr=0x{:08X}).",
                            err_hr as u32
                        ));
                    }
                }
            }
        }

        out
    }

    fn connect_namespace(&self, ns: &str) -> Option<IWbemServices> {
        let locator = self.locator.as_ref()?;
        unsafe {
            let ns_bstr = BSTR::from(ns);
            let empty = BSTR::new();
            let svc = locator
                .ConnectServer(&ns_bstr, &empty, &empty, &empty, 0, &empty, None)
                .ok()?;
            let _ = CoSetProxyBlanket(
                &svc,
                10, // RPC_C_AUTHN_WINNT
                0,  // RPC_C_AUTHZ_NONE
                None,
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            );
            Some(svc)
        }
    }
}

impl Drop for WindowsWmiMonitor {
    fn drop(&mut self) {
        self.lhm_services = None;
        self.cim_services = None;
        self.acpi_services = None;
        self.locator = None;
        if self.com_owned {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

unsafe fn get_prop_string(obj: &IWbemClassObject, name: PCWSTR) -> Option<String> {
    unsafe {
        let mut guard = VariantGuard::new();
        if obj.Get(name, 0, &mut guard.0, None, None).is_err() {
            return None;
        }
        let vt = guard.0.Anonymous.Anonymous.vt;
        if vt == VT_BSTR {
            let bstr = &*guard.0.Anonymous.Anonymous.Anonymous.bstrVal;
            Some(bstr.to_string())
        } else {
            None
        }
    }
}

unsafe fn get_prop_f32(obj: &IWbemClassObject, name: PCWSTR) -> Option<f32> {
    unsafe {
        let mut guard = VariantGuard::new();
        if obj.Get(name, 0, &mut guard.0, None, None).is_err() {
            return None;
        }
        let inner = &guard.0.Anonymous.Anonymous;
        if inner.vt == VT_R4 {
            Some(inner.Anonymous.fltVal)
        } else if inner.vt == VT_I4 {
            Some(inner.Anonymous.lVal as f32)
        } else if inner.vt == VT_UI4 {
            Some(inner.Anonymous.ulVal as f32)
        } else {
            None
        }
    }
}

fn query_lhm_sensors(svc: &IWbemServices, out: &mut WmiSensorSample) {
    unsafe {
        let wql = BSTR::from("WQL");
        let query = BSTR::from("SELECT Name, SensorType, Value, Identifier FROM Sensor");
        let flags = WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY;
        let Ok(enumerator) = svc.ExecQuery(&wql, &query, flags, None) else {
            return;
        };

        let mut best_cpu_prio = u8::MAX;
        loop {
            let mut objs = [None];
            let mut returned = 0u32;
            let hr = enumerator.Next(WBEM_INFINITE, &mut objs, &mut returned);
            if returned == 0 {
                break;
            }
            if let Some(obj) = objs[0].as_ref() {
                let name = get_prop_string(obj, w!("Name")).unwrap_or_default();
                let stype = get_prop_string(obj, w!("SensorType")).unwrap_or_default();
                let ident = get_prop_string(obj, w!("Identifier")).unwrap_or_default();
                let val = get_prop_f32(obj, w!("Value"));

                if let Some(v) = val {
                    let ident_l = ident.to_ascii_lowercase();
                    let name_l = name.to_ascii_lowercase();

                    if ident_l.contains("cpu") && stype == "Temperature" && v > 0.0 && v < 130.0 {
                        let prio = if name_l.contains("package") || name_l.contains("tctl/tdie") {
                            0
                        } else if name_l.contains("tdie") || name_l.contains("ccds max") {
                            1
                        } else {
                            2
                        };
                        if prio < best_cpu_prio {
                            best_cpu_prio = prio;
                            out.cpu_temp_c = Some(v);
                        }
                    } else if ident_l.contains("gpu") {
                        match stype.as_str() {
                            "Temperature" if v > 0.0 && v < 130.0 => {
                                if out.gpu_temp_c.is_none() || name_l.contains("core") || name_l.contains("edge") {
                                    out.gpu_temp_c = Some(v);
                                }
                            }
                            "Clock" if v > 0.0 && name_l.contains("core") => {
                                out.gpu_clock_ghz = Some(v / 1000.0);
                            }
                            "Load" if (0.0..=100.0).contains(&v) && name_l.contains("core") => {
                                out.gpu_usage = Some(v / 100.0);
                            }
                            "Control" if (0.0..=100.0).contains(&v) && name_l.contains("fan") => {
                                out.gpu_fan_percent = Some(v / 100.0);
                            }
                            _ => {}
                        }
                    }
                }
            }
            if hr.is_err() {
                break;
            }
        }
    }
}

fn query_cimv2_thermal_zone(svc: &IWbemServices) -> Result<Option<f32>, i32> {
    unsafe {
        let wql = BSTR::from("WQL");
        let query = BSTR::from(
            "SELECT HighPrecisionTemperature, Temperature FROM Win32_PerfFormattedData_Counters_ThermalZoneInformation",
        );
        let flags = WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY;
        let enumerator = svc.ExecQuery(&wql, &query, flags, None).map_err(|e| e.code().0)?;

        let mut max_temp: Option<f32> = None;
        loop {
            let mut objs = [None];
            let mut returned = 0u32;
            let hr = enumerator.Next(WBEM_INFINITE, &mut objs, &mut returned);
            if hr.is_err() && returned == 0 {
                return Err(hr.0);
            }
            if returned == 0 {
                break;
            }
            if let Some(obj) = objs[0].as_ref() {
                let hp = get_prop_f32(obj, w!("HighPrecisionTemperature"));
                let raw = get_prop_f32(obj, w!("Temperature"));
                let celsius = hp
                    .and_then(|v| {
                        if v > 2000.0 {
                            Some((v / 10.0) - 273.15)
                        } else if v > 200.0 && v < 450.0 {
                            Some(v - 273.15)
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        raw.and_then(|v| {
                            if v > 200.0 && v < 450.0 {
                                Some(v - 273.15)
                            } else {
                                None
                            }
                        })
                    });
                if let Some(c) = celsius {
                    if c > 5.0 && c < 130.0 {
                        max_temp = Some(max_temp.map_or(c, |m| m.max(c)));
                    }
                }
            }
            if hr.is_err() {
                break;
            }
        }
        Ok(max_temp)
    }
}

fn query_acpi_thermal_zone(svc: &IWbemServices) -> Result<Option<f32>, i32> {
    unsafe {
        let wql = BSTR::from("WQL");
        let query = BSTR::from("SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature");
        let flags = WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY;
        let enumerator = svc.ExecQuery(&wql, &query, flags, None).map_err(|e| e.code().0)?;

        let mut max_temp: Option<f32> = None;
        loop {
            let mut objs = [None];
            let mut returned = 0u32;
            let hr = enumerator.Next(WBEM_INFINITE, &mut objs, &mut returned);
            if hr.is_err() && returned == 0 {
                return Err(hr.0);
            }
            if returned == 0 {
                break;
            }
            if let Some(obj) = objs[0].as_ref() {
                if let Some(raw_tenths_k) = get_prop_f32(obj, w!("CurrentTemperature")) {
                    let celsius = (raw_tenths_k / 10.0) - 273.15;
                    if celsius > 0.0 && celsius < 130.0 {
                        max_temp = Some(max_temp.map_or(celsius, |m| m.max(celsius)));
                    }
                }
            }
            if hr.is_err() {
                break;
            }
        }
        Ok(max_temp)
    }
}
