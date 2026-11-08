//! GitHub release update checker and self-updater.
//!
//! Queries the GitHub API for the latest release of
//! Cheerimy-Studio/Soundguard, compares versions, and can download +
//! replace the server binary with an atomic swap.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

static UPDATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const MAX_BINARY_BYTES: usize = 128 * 1024 * 1024;

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
    #[serde(default)]
    pub linux_sha256: String,
    #[serde(default)]
    pub windows_sha256: String,
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
    #[serde(default)]
    digest: Option<String>,
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
                assets.linux_sha256 = asset
                    .digest
                    .as_deref()
                    .unwrap_or_default()
                    .strip_prefix("sha256:")
                    .unwrap_or_default()
                    .to_string();
            }
            "soundguard-server.exe" => {
                assets.windows_server = asset.browser_download_url.clone();
                assets.windows_sha256 = asset
                    .digest
                    .as_deref()
                    .unwrap_or_default()
                    .strip_prefix("sha256:")
                    .unwrap_or_default()
                    .to_string();
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
pub async fn apply_update(asset_url: &str, expected_sha256: &str) -> Result<(), String> {
    let _guard = UPDATE_LOCK.try_lock().map_err(|_| "更新正在进行，请稍后查看版本")?;
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("发布文件缺少有效的 SHA-256 校验值，请手动更新".to_string());
    }
    let url = reqwest::Url::parse(asset_url).map_err(|_| "更新地址无效")?;
    if url.scheme() != "https" || url.host_str() != Some("github.com")
        || !url.path().starts_with(&format!("/{GITHUB_REPO}/releases/download/")) {
        return Err("更新地址不属于项目发布目录".to_string());
    }
    if asset_url.is_empty() {
        return Err("该平台暂无可用的更新包".to_string());
    }

    let mut response = reqwest::Client::new()
        .get(asset_url)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("下载更新失败: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("下载更新失败: HTTP {}", response.status()));
    }

    if response.content_length().is_some_and(|size| size > MAX_BINARY_BYTES as u64) {
        return Err("更新包超过大小限制".to_string());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("读取下载数据失败: {e}"))? {
        if bytes.len() + chunk.len() > MAX_BINARY_BYTES {
            return Err("更新包超过大小限制".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    verify_binary(&bytes, expected_sha256)?;

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

fn verify_binary(bytes: &[u8], expected_sha256: &str) -> Result<(), String> {
    if bytes.len() < 1024 { return Err("下载的文件过小，可能是无效更新包".into()); }
    let actual = format!("{:x}", Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err("更新文件校验失败，当前程序未替换".into());
    }
    #[cfg(target_os = "linux")]
    if !bytes.starts_with(b"\x7fELF") { return Err("更新文件不是 Linux 可执行程序".into()); }
    #[cfg(windows)]
    if !bytes.starts_with(b"MZ") { return Err("更新文件不是 Windows 可执行程序".into()); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupted_or_non_executable_updates_are_rejected() {
        let mut bytes = vec![0; 2048];
        #[cfg(target_os = "linux")]
        bytes[..4].copy_from_slice(b"\x7fELF");
        #[cfg(windows)]
        bytes[..2].copy_from_slice(b"MZ");
        let hash = format!("{:x}", Sha256::digest(&bytes));
        assert!(verify_binary(&bytes, &hash).is_ok());
        bytes[100] ^= 1;
        assert!(verify_binary(&bytes, &hash).is_err());
        let html = vec![b'x'; 2048];
        let hash = format!("{:x}", Sha256::digest(&html));
        assert!(verify_binary(&html, &hash).is_err());
        assert!(verify_binary(b"short", &hash).is_err());
    }
}
