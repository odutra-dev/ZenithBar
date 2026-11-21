use std::fs;
use std::io::Read;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag_name: String,
    #[allow(dead_code)]
    pub name: String,
    pub download_url: String,
    #[allow(dead_code)]
    pub html_url: String,
}

fn parse_version(v: &str) -> (u32, u32, u32) {
    let clean = v.trim_start_matches(|c| c == 'v' || c == 'V');
    let mut parts = clean.split('.');
    let maj = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let min = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let pat = parts.next().and_then(|s| s.split('-').next().and_then(|p| p.parse().ok())).unwrap_or(0);
    (maj, min, pat)
}

fn is_newer_version(remote: &str, current: &str) -> bool {
    parse_version(remote) > parse_version(current)
}

pub fn check_github_release() -> Result<Option<ReleaseInfo>, String> {
    let current_version = env!("CARGO_PKG_VERSION");
    let url = "https://api.github.com/repos/odutra-dev/ZenithBar/releases/latest";

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build();

    let response = agent
        .get(url)
        .set("User-Agent", &format!("ZenithBar/{}", current_version))
        .set("Accept", "application/vnd.github.v3+json")
        .call()
        .map_err(|e| format!("HTTP request error: {}", e))?;

    let json: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("JSON parsing error: {}", e))?;

    let tag_name = json["tag_name"].as_str().unwrap_or("").to_string();
    if tag_name.is_empty() {
        return Err("Missing tag_name in release payload".to_string());
    }

    if !is_newer_version(&tag_name, current_version) {
        return Ok(None);
    }

    let name = json["name"].as_str().unwrap_or(&tag_name).to_string();
    let html_url = json["html_url"].as_str().unwrap_or("").to_string();

    let mut download_url = None;
    if let Some(assets) = json["assets"].as_array() {
        for asset in assets {
            let asset_name = asset["name"].as_str().unwrap_or("");
            #[cfg(target_os = "windows")]
            let matches = asset_name.eq_ignore_ascii_case("ZenithBar.exe") || asset_name.ends_with(".exe");
            #[cfg(not(target_os = "windows"))]
            let matches = asset_name.ends_with(".AppImage") || asset_name.eq_ignore_ascii_case("ZenithBar");

            if matches {
                if let Some(url) = asset["browser_download_url"].as_str() {
                    download_url = Some(url.to_string());
                    break;
                }
            }
        }
    }

    let download_url = match download_url {
        Some(u) => u,
        None => {
            #[cfg(target_os = "windows")]
            return Err("No executable asset (.exe) found in release".to_string());
            #[cfg(not(target_os = "windows"))]
            return Err("No Linux package (.AppImage) found in release".to_string());
        }
    };

    Ok(Some(ReleaseInfo {
        tag_name,
        name,
        download_url,
        html_url,
    }))
}

pub fn download_and_install_update(download_url: &str) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|e| format!("Could not get current executable path: {}", e))?;
    let temp_file = std::env::temp_dir().join(format!("zenithbar_update_{}.tmp", std::process::id()));

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(120))
        .build();

    let response = agent
        .get(download_url)
        .set("User-Agent", "ZenithBar-Updater")
        .call()
        .map_err(|e| format!("Failed to download update binary: {}", e))?;

    let mut reader = response.into_reader();
    let mut file = fs::File::create(&temp_file)
        .map_err(|e| format!("Failed to create temporary file: {}", e))?;

    let mut buffer = [0u8; 16384];
    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|e| format!("Error while streaming update: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buffer[..bytes_read])
            .map_err(|e| format!("Error writing update file: {}", e))?;
    }
    drop(file);

    #[cfg(target_os = "windows")]
    {
        let old_exe = current_exe.with_extension("exe.old");
        if old_exe.exists() {
            let _ = fs::remove_file(&old_exe);
        }

        // On Windows, rename active running .exe to .exe.old
        fs::rename(&current_exe, &old_exe)
            .map_err(|e| format!("Failed to rename current executable to .old: {}", e))?;

        // Move downloaded binary into active position
        if let Err(e) = fs::rename(&temp_file, &current_exe) {
            // Restore previous exe if move fails
            let _ = fs::rename(&old_exe, &current_exe);
            return Err(format!("Failed to move update into position: {}", e));
        }

        // Spawn updated executable
        let _ = std::process::Command::new(&current_exe)
            .arg("--restarted")
            .spawn()
            .map_err(|e| format!("Failed to spawn updated process: {}", e))?;

        crate::log_msg("Atualização concluída com sucesso. Reiniciando novo ZenithBar.exe...");
        std::process::exit(0);
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&temp_file, fs::Permissions::from_mode(0o755));

        let old_exe = current_exe.with_extension("old");
        if old_exe.exists() {
            let _ = fs::remove_file(&old_exe);
        }
        let _ = fs::rename(&current_exe, &old_exe);
        if let Err(e) = fs::rename(&temp_file, &current_exe) {
            let _ = fs::rename(&old_exe, &current_exe);
            return Err(format!("Failed to replace executable: {}", e));
        }
        let _ = std::process::Command::new(&current_exe)
            .arg("--restarted")
            .spawn()
            .map_err(|e| format!("Failed to spawn process: {}", e))?;
        crate::log_msg("Atualização concluída no Linux. Reiniciando ZenithBar...");
        std::process::exit(0);
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(not(target_os = "windows"))]
fn prompt_user_linux(title: &str, body: &str) -> bool {
    // 1. Try zenity
    if let Ok(status) = std::process::Command::new("zenity")
        .args(["--question", "--title", title, "--text", body])
        .status()
    {
        return status.success();
    }

    // 2. Try kdialog (KDE)
    if let Ok(status) = std::process::Command::new("kdialog")
        .args(["--title", title, "--yesno", body])
        .status()
    {
        return status.success();
    }

    false
}

#[cfg(not(target_os = "windows"))]
fn notify_user_linux(title: &str, body: &str) {
    if let Ok(status) = std::process::Command::new("zenity")
        .args(["--info", "--title", title, "--text", body])
        .status()
    {
        if status.success() {
            return;
        }
    }
    if let Ok(status) = std::process::Command::new("kdialog")
        .args(["--title", title, "--msgbox", body])
        .status()
    {
        if status.success() {
            return;
        }
    }
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "ZenithBar", title, body])
        .status();
}

pub fn prompt_and_install_update(release: &ReleaseInfo, culture: &str) {
    let s = crate::i18n::load_locale(culture);
    let current_version = env!("CARGO_PKG_VERSION");

    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{
            MessageBoxW, IDYES, MB_ICONINFORMATION, MB_YESNO,
        };

        let title = to_wide(&format!("ZenithBar - {}", s.update_available_title));
        let body = to_wide(&format!(
            "{}\n\n{}: {}\n{}: v{}\n\n{}",
            s.update_available_msg,
            s.update_new_version,
            release.tag_name,
            s.update_current_version,
            current_version,
            s.update_ask_prompt
        ));

        let res = unsafe {
            MessageBoxW(
                None,
                PCWSTR(body.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_YESNO | MB_ICONINFORMATION,
            )
        };

        if res == IDYES {
            crate::log_msg(&format!("Usuário autorizou download da atualização {}.", release.tag_name));
            if let Err(e) = download_and_install_update(&release.download_url) {
                let err_title = to_wide("ZenithBar - Erro de Atualização");
                let err_body = to_wide(&format!("{} {}", s.update_failed, e));
                unsafe {
                    let _ = MessageBoxW(
                        None,
                        PCWSTR(err_body.as_ptr()),
                        PCWSTR(err_title.as_ptr()),
                        windows::Win32::UI::WindowsAndMessaging::MB_OK
                            | windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                    );
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let title = format!("ZenithBar - {}", s.update_available_title);
        let body = format!(
            "{}\n\n{}: {}\n{}: v{}\n\n{}",
            s.update_available_msg,
            s.update_new_version,
            release.tag_name,
            s.update_current_version,
            current_version,
            s.update_ask_prompt
        );

        if prompt_user_linux(&title, &body) {
            crate::log_msg(&format!("Usuário autorizou download da atualização {}.", release.tag_name));
            if let Err(e) = download_and_install_update(&release.download_url) {
                notify_user_linux("ZenithBar - Erro", &format!("{} {}", s.update_failed, e));
            }
        } else {
            // Se o usuário não tiver zenity/kdialog ou cancelar, abre no navegador padrão
            let _ = std::process::Command::new("xdg-open").arg(&release.html_url).spawn();
        }
    }
}

pub fn run_manual_update_check(culture: &str) {
    let s = crate::i18n::load_locale(culture);

    match check_github_release() {
        Ok(Some(release)) => {
            prompt_and_install_update(&release, culture);
        }
        Ok(None) => {
            #[cfg(target_os = "windows")]
            {
                use windows::core::PCWSTR;
                use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
                let title = to_wide(&s.update_up_to_date_title);
                let current_version = env!("CARGO_PKG_VERSION");
                let body = to_wide(&format!("{} (v{})", s.update_up_to_date_msg, current_version));
                unsafe {
                    let _ = MessageBoxW(None, PCWSTR(body.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONINFORMATION);
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let current_version = env!("CARGO_PKG_VERSION");
                let body = format!("{} (v{})", s.update_up_to_date_msg, current_version);
                notify_user_linux(&s.update_up_to_date_title, &body);
            }
        }
        Err(e) => {
            #[cfg(target_os = "windows")]
            {
                use windows::core::PCWSTR;
                use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
                let title = to_wide("ZenithBar - Update Error");
                let body = to_wide(&format!("{} {}", s.update_failed, e));
                unsafe {
                    let _ = MessageBoxW(None, PCWSTR(body.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONERROR);
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                notify_user_linux("ZenithBar - Update Error", &format!("{} {}", s.update_failed, e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_handles_prefix_prerelease_and_partial_strings() {
        assert_eq!(parse_version("0.6.2"), (0, 6, 2));
        assert_eq!(parse_version("v0.6.2"), (0, 6, 2));
        assert_eq!(parse_version("V1.2.3-rc.1"), (1, 2, 3));
        assert_eq!(parse_version("2.10"), (2, 10, 0));
        assert_eq!(parse_version("invalid"), (0, 0, 0));
    }

    #[test]
    fn is_newer_version_compares_numeric_semver_components() {
        assert!(is_newer_version("v0.10.0", "0.6.2"));
        assert!(is_newer_version("v0.6.3", "0.6.2"));
        assert!(is_newer_version("1.0.0", "0.99.99"));

        assert!(!is_newer_version("v0.6.2", "0.6.2"));
        assert!(!is_newer_version("0.6.1", "0.6.2"));
        assert!(!is_newer_version("0.6.2-beta", "0.6.2"));
    }
}
