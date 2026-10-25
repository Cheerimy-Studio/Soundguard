//! GitHub release update checker and self-updater.
//!
//! Queries the GitHub API for the latest release of
//! Cheerimy-Studio/Soundguard, compares versions, and can download +
//! replace the server binary with an atomic swap.

use serde::{Deserialize, Serialize};

pub const GITHUB_REPO: &str = "Cheerimy-Studio/Soundguard";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    #[serde(default)]
    pub release_url: String,
    #[serde(default)]
    pub release_notes: String,
    #[serde(default)]
    pub published_at: String,
    #[serde(default)]
    pub download_urls: UpdateAssets,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAssets {
    #[serde(default)]
    pub linux_x86_64: String,
    #[serde(default)]
    pub windows_server: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    #[allow(dead_code)] // used in release JSON
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    #[serde(default)]
    browser_download_url: String,
}

pub async fn check_github() -> Result<UpdateCheck, String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let response = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "SoundGuard-Server")
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("连接 GitHub 失败: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API 返回 {}", response.status()));
    }

    let release: GitHubRelease = response
        .json()
        .await
        .map_err(|e| format!("解析版本信息失败: {e}"))?;

    let latest = release.tag_name.trim_start_matches('v').trim().to_string();
    let current = CURRENT_VERSION.trim().to_string();
    let update_available = version_lt(&current, &latest);

    let mut assets = UpdateAssets::default();
    for asset in &release.assets {
        match asset.name.as_str() {
            "soundguard-server-linux-x86_64" => {
                assets.linux_x86_64 = asset.browser_download_url.clone();
            }
            "soundguard-server.exe" => {
                assets.windows_server = asset.browser_download_url.clone();
            }
            _ => {}
        }
    }

    Ok(UpdateCheck {
        current_version: current,
        latest_version: latest,
        update_available,
        release_url: release.html_url.clone().unwrap_or_default(),
        release_notes: release.body.clone().unwrap_or_default(),
        published_at: release.published_at.clone().unwrap_or_default(),
        download_urls: assets,
    })
}

fn version_lt(current: &str, latest: &str) -> bool {
    let parse =
        |v: &str| -> Vec<u32> { v.split('.').filter_map(|p| p.trim().parse().ok()).collect() };
    let c = parse(current);
    let l = parse(latest);
    for i in 0..c.len().max(l.len()) {
        let cv = c.get(i).copied().unwrap_or(0);
        let lv = l.get(i).copied().unwrap_or(0);
        if cv < lv {
            return true;
        }
        if cv > lv {
            return false;
        }
    }
    false
}

/// Download the new binary and atomically replace the current executable.
/// Returns Ok(()) when the swap succeeded; the caller should then exit(0)
/// and let systemd/restart the process.
pub async fn apply_update(asset_url: &str) -> Result<(), String> {
    if asset_url.is_empty() {
        return Err("该平台暂无可用的更新包".to_string());
    }

    let response = reqwest::Client::new()
        .get(asset_url)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("下载更新失败: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("下载更新失败: HTTP {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("读取下载数据失败: {e}"))?;

    if bytes.is_empty() || bytes.len() < 1024 {
        return Err("下载的文件过小，可能是无效更新包".to_string());
    }

    let current = std::env::current_exe().map_err(|e| format!("无法定位当前程序: {e}"))?;
    let parent = current
        .parent()
        .ok_or_else(|| "无法定位程序目录".to_string())?;

    // On Unix, write new binary alongside and rename over the old one.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let new_path = parent.join(".soundguard-server.new");
        std::fs::write(&new_path, &bytes).map_err(|e| format!("写入新程序失败: {e}"))?;
        std::fs::set_permissions(&new_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置执行权限失败: {e}"))?;
        std::fs::rename(&new_path, &current).map_err(|e| format!("替换程序失败: {e}"))?;
    }

    // On Windows, rename current to .old, then rename new to current.
    #[cfg(windows)]
    {
        let new_path = parent.join("soundguard-server.exe.new");
        let old_path = parent.join("soundguard-server.exe.old");
        std::fs::write(&new_path, &bytes).map_err(|e| format!("写入新程序失败: {e}"))?;
        if old_path.exists() {
            let _ = std::fs::remove_file(&old_path);
        }
        std::fs::rename(&current, &old_path).map_err(|e| format!("备份旧程序失败: {e}"))?;
        std::fs::rename(&new_path, &current).map_err(|e| {
            // Rollback: put the old binary back.
            let _ = std::fs::rename(&old_path, &current);
            format!("替换程序失败: {e}")
        })?;
    }

    tracing::info!("Server binary updated to latest release");
    Ok(())
}
