//! GitHub release update checker and self-updater.
//!
//! Queries the GitHub API for the latest release of
//! Cheerimy-Studio/Soundguard, compares versions, and can download +
//! replace the server binary with an atomic swap.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use std::path::Path;

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub state: String,
    pub progress: f64,
    pub downloaded: u64,
    pub total: u64,
    pub message: String,
    pub new_version: String,
    pub restart_required: bool,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            progress: 0.0,
            downloaded: 0,
            total: 0,
            message: String::new(),
            new_version: String::new(),
            restart_required: false,
        }
    }
}

/// Shared state for the asynchronous update job. Long downloads must not be
/// tied to one HTTP request: reverse proxies (Cloudflare) return 524 when a
/// request takes more than ~100 seconds.
pub struct UpdateState {
    status: tokio::sync::Mutex<UpdateStatus>,
    pub running: std::sync::atomic::AtomicBool,
}

impl UpdateState {
    pub fn new() -> Self {
        Self {
            status: tokio::sync::Mutex::new(UpdateStatus::default()),
            running: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub async fn snapshot(&self) -> UpdateStatus {
        self.status.lock().await.clone()
    }

    pub async fn set(&self, state: &str, message: impl Into<String>) {
        let mut status = self.status.lock().await;
        status.state = state.into();
        status.message = message.into();
        if state == "downloading" {
            status.progress = 0.0;
            status.downloaded = 0;
            status.total = 0;
            status.new_version.clear();
            status.restart_required = false;
        }
    }

    pub async fn progress(&self, downloaded: u64, total: u64) {
        let mut status = self.status.lock().await;
        status.state = "downloading".into();
        status.downloaded = downloaded;
        status.total = total;
        status.progress = if total > 0 {
            (downloaded as f64 / total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    pub async fn finish(&self, state: &str, message: impl Into<String>, version: &str, restart_required: bool) {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let mut status = self.status.lock().await;
        status.state = state.into();
        status.message = message.into();
        status.new_version = version.to_string();
        status.restart_required = restart_required;
        status.progress = if state == "done" { 1.0 } else { status.progress };
    }
}

impl Default for UpdateState {
    fn default() -> Self {
        Self::new()
    }
}

const DEFAULT_MIRRORS: [&str; 3] = [
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://ghproxy.net/",
];

/// Mirrors are only used for binary downloads. The expected SHA-256 still comes
/// from the official GitHub API, so a mirror cannot substitute another binary.
fn mirror_prefixes() -> Vec<String> {
    match std::env::var("UPDATE_MIRRORS") {
        Ok(raw) if raw.trim().eq_ignore_ascii_case("off") => Vec::new(),
        Ok(raw) if !raw.trim().is_empty() => raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| {
                if item.ends_with('/') {
                    item.to_string()
                } else {
                    format!("{item}/")
                }
            })
            .collect(),
        _ => DEFAULT_MIRRORS.iter().map(|item| item.to_string()).collect(),
    }
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

/// Validates that the URL comes from this project's GitHub release directory.
/// Mirror URLs are generated internally from this trusted URL.
fn validate_asset_url(asset_url: &str) -> Result<reqwest::Url, String> {
    if asset_url.is_empty() {
        return Err("该平台暂无可用的更新包".to_string());
    }
    let url = reqwest::Url::parse(asset_url).map_err(|_| "更新地址无效")?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url
            .path()
            .starts_with(&format!("/{GITHUB_REPO}/releases/download/"))
    {
        return Err("更新地址不属于项目发布目录".to_string());
    }
    Ok(url)
}

fn managed_restart() -> bool {
    std::env::var_os("INVOCATION_ID").is_some()
        || std::env::var("SOUNDGUARD_AUTO_RESTART").is_ok_and(|value| value == "1")
}

async fn download_candidate(
    client: &reqwest::Client,
    url: &str,
    expected_sha256: &str,
    state: &UpdateState,
) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url)
        // Do not let one blocked source hold the job forever; a mirror gets a
        // turn after this per-source timeout.
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| format!("连接失败: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BINARY_BYTES as u64)
    {
        return Err("更新包超过大小限制".to_string());
    }
    let total = response.content_length().unwrap_or(0);
    let mut bytes = Vec::new();
    let mut reported = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("读取下载数据失败: {e}"))?
    {
        if bytes.len() + chunk.len() > MAX_BINARY_BYTES {
            return Err("更新包超过大小限制".to_string());
        }
        bytes.extend_from_slice(&chunk);
        let downloaded = bytes.len() as u64;
        if downloaded.saturating_sub(reported) >= 256 * 1024 {
            reported = downloaded;
            state.progress(downloaded, total).await;
        }
    }
    state.progress(bytes.len() as u64, total).await;
    verify_binary(&bytes, expected_sha256)?;
    Ok(bytes)
}

/// Windows refuses to rename a directory while a browser request still holds
/// a file inside it. Retry briefly instead of failing an otherwise valid
/// upload when the dashboard is open.
fn rename_with_retry(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    let mut last_error = None;
    for _ in 0..20 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

fn replace_binary(bytes: &[u8]) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| format!("无法定位当前程序: {e}"))?;
    let parent = current
        .parent()
        .ok_or_else(|| "无法定位程序目录".to_string())?;

    // On Unix, write new binary alongside and rename over the old one.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let new_path = parent.join(".soundguard-server.new");
        std::fs::write(&new_path, bytes).map_err(|e| format!("写入新程序失败: {e}"))?;
        std::fs::set_permissions(&new_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置执行权限失败: {e}"))?;
        std::fs::rename(&new_path, &current).map_err(|e| format!("替换程序失败: {e}"))?;
    }

    // On Windows, rename current to .old, then rename new to current.
    #[cfg(windows)]
    {
        let new_path = parent.join("soundguard-server.exe.new");
        let old_path = parent.join("soundguard-server.exe.old");
        std::fs::write(&new_path, bytes).map_err(|e| format!("写入新程序失败: {e}"))?;
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
    Ok(())
}

/// Downloads the new binary from GitHub, falling back to mirrors when the
/// official asset host is blocked or slow. Every candidate is checked against
/// the official release's SHA-256 before the executable is replaced.
pub async fn apply_update(
    asset_url: &str,
    expected_sha256: &str,
    state: &UpdateState,
    new_version: &str,
) -> Result<bool, String> {
    let _guard = UPDATE_LOCK
        .try_lock()
        .map_err(|_| "更新正在进行，请稍后查看版本")?;
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("发布文件缺少有效的 SHA-256 校验值，请手动更新".to_string());
    }
    validate_asset_url(asset_url)?;
    let client = reqwest::Client::new();
    let mirrors = mirror_prefixes();
    let mut candidates = vec![asset_url.to_string()];
    candidates.extend(
        mirrors
            .iter()
            .map(|prefix| format!("{prefix}{asset_url}")),
    );

    let mut bytes = None;
    let mut last_error = String::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let label = if index == 0 {
            "官方源".to_string()
        } else {
            format!("镜像源 {}/{}", index, mirrors.len())
        };
        state
            .set("downloading", format!("正在从{label}下载更新…"))
            .await;
        match download_candidate(&client, candidate, expected_sha256, state).await {
            Ok(data) => {
                bytes = Some(data);
                break;
            }
            Err(error) => {
                last_error = format!("{label}: {error}");
            }
        }
    }
    let bytes = bytes.ok_or_else(|| {
        if last_error.is_empty() {
            "下载更新失败".to_string()
        } else {
            format!("官方源与镜像均下载失败（{last_error}）")
        }
    })?;

    state.set("applying", "正在校验并替换服务端程序…").await;
    replace_binary(&bytes)?;
    let restart_required = !managed_restart();
    state
        .finish(
            if restart_required { "done" } else { "restarting" },
            if restart_required {
                "程序已替换，请手动重启服务端以使用新版本"
            } else {
                "更新完成，服务端即将重启"
            },
            new_version,
            restart_required,
        )
        .await;
    if !restart_required {
        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            tracing::info!("Update applied; restarting server");
            // The supplied systemd unit restarts only non-zero exits.
            std::process::exit(75);
        });
    }
    tracing::info!("Server binary updated to latest release");
    Ok(restart_required)
}

fn verify_executable(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 1024 {
        return Err("程序文件过小，可能是无效更新包".into());
    }
    #[cfg(target_os = "linux")]
    if !bytes.starts_with(b"\x7fELF") {
        return Err("更新文件不是 Linux 可执行程序".into());
    }
    #[cfg(windows)]
    if !bytes.starts_with(b"MZ") {
        return Err("更新文件不是 Windows 可执行程序".into());
    }
    Ok(())
}

fn verify_binary(bytes: &[u8], expected_sha256: &str) -> Result<(), String> {
    verify_executable(bytes)?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err("更新文件校验失败，当前程序未替换".into());
    }
    Ok(())
}

const MAX_UPLOAD_UNCOMPRESSED: u64 = 256 * 1024 * 1024;
const MAX_UPLOAD_FILES: usize = 4_000;

/// Applies a ZIP containing the current platform's server binary and a
/// `web/dist` tree. The upload path is for offline/blocked-GitHub installs.
pub async fn apply_uploaded_package(
    package: Vec<u8>,
    web_dist: &Path,
    state: &UpdateState,
) -> Result<(), String> {
    state.set("applying", "正在校验完整包…").await;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(&package)).map_err(|e| format!("完整包不是有效 ZIP：{e}"))?;
    let binary_candidates: &[&str] = if cfg!(windows) {
        &["windows/soundguard-server.exe", "soundguard-server.exe"]
    } else {
        &["linux/soundguard-server", "soundguard-server"]
    };
    let mut binary: Option<Vec<u8>> = None;
    let mut web_files = 0usize;
    let mut total = 0u64;
    let mut has_index = false;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|e| format!("读取完整包失败：{e}"))?;
        let Some(path) = file.enclosed_name() else {
            return Err("完整包包含非法路径".into());
        };
        let path = path.to_string_lossy().replace('\\', "/");
        if file.is_dir() {
            continue;
        }
        if let Some(mode) = file.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err("完整包不能包含符号链接".into());
            }
        }
        total = total.saturating_add(file.size());
        if total > MAX_UPLOAD_UNCOMPRESSED {
            return Err("完整包解压后超过 256 MB".into());
        }
        if binary_candidates.iter().any(|candidate| path == *candidate) {
            let mut data = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut data)
                .map_err(|e| format!("读取服务端程序失败：{e}"))?;
            verify_executable(&data)?;
            binary = Some(data);
        } else if path.starts_with("web/dist/") {
            web_files += 1;
            if path == "web/dist/index.html" {
                has_index = true;
            }
        }
    }
    let binary = binary.ok_or_else(|| "完整包缺少当前平台的服务端程序".to_string())?;
    if web_files == 0 || !has_index {
        return Err("完整包缺少 web/dist/index.html".into());
    }
    if web_files > MAX_UPLOAD_FILES {
        return Err("完整包文件数量过多".into());
    }

    let parent = web_dist
        .parent()
        .ok_or_else(|| "无法定位管理端目录".to_string())?;
    let name = web_dist
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("dist");
    let new_dir = parent.join(format!(".{name}.upload-new"));
    let old_dir = parent.join(format!(".{name}.upload-old"));
    if new_dir.exists() {
        std::fs::remove_dir_all(&new_dir).map_err(|e| format!("清理旧暂存目录失败：{e}"))?;
    }
    if old_dir.exists() {
        std::fs::remove_dir_all(&old_dir).map_err(|e| format!("清理旧备份目录失败：{e}"))?;
    }
    std::fs::create_dir_all(&new_dir).map_err(|e| format!("创建暂存目录失败：{e}"))?;

    let mut archive =
        zip::ZipArchive::new(Cursor::new(&package)).map_err(|e| format!("完整包不是有效 ZIP：{e}"))?;
    let mut extracted = 0u64;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|e| format!("读取完整包失败：{e}"))?;
        let Some(path) = file.enclosed_name() else {
            continue;
        };
        let path = path.to_string_lossy().replace('\\', "/");
        let Some(relative) = path.strip_prefix("web/dist/") else {
            continue;
        };
        if relative.is_empty() || file.is_dir() {
            continue;
        }
        let target = new_dir.join(relative);
        if !target.starts_with(&new_dir) {
            return Err("完整包包含非法路径".into());
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建管理端目录失败：{e}"))?;
        }
        let mut output =
            std::fs::File::create(&target).map_err(|e| format!("写入管理端文件失败：{e}"))?;
        std::io::copy(&mut file, &mut output)
            .map_err(|e| format!("写入管理端文件失败：{e}"))?;
        extracted = extracted.saturating_add(file.size());
        if extracted > MAX_UPLOAD_UNCOMPRESSED {
            return Err("完整包解压后超过 256 MB".into());
        }
    }

    state.set("applying", "正在替换服务端程序…").await;
    replace_binary(&binary)?;
    state.set("applying", "正在更新管理端文件…").await;
    if web_dist.exists() {
        rename_with_retry(web_dist, &old_dir)
            .map_err(|e| format!("暂存旧管理端文件失败：{e}"))?;
    }
    if let Err(error) = rename_with_retry(&new_dir, web_dist) {
        let _ = rename_with_retry(&old_dir, web_dist);
        return Err(format!("更新管理端文件失败：{error}"));
    }
    if old_dir.exists() {
        let _ = std::fs::remove_dir_all(&old_dir);
    }

    let restart_required = !managed_restart();
    state
        .finish(
            if restart_required { "done" } else { "restarting" },
            if restart_required {
                "完整包已应用，请手动重启服务端"
            } else {
                "完整包已应用，服务端即将重启"
            },
            "完整包",
            restart_required,
        )
        .await;
    if !restart_required {
        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            std::process::exit(75);
        });
    }
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

    #[tokio::test]
    async fn uploaded_package_rejects_invalid_or_incomplete_archives() {
        use std::io::Write as _;
        let state = UpdateState::new();
        let dir = std::env::temp_dir().join(format!("sg-upload-{}", uuid::Uuid::new_v4()));
        let web = dir.join("web/dist");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(apply_uploaded_package(b"not a zip".to_vec(), &web, &state)
            .await
            .is_err());

        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            writer
                .start_file("web/dist/index.html", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"<html></html>").unwrap();
            writer.finish().unwrap();
        }
        let error = apply_uploaded_package(buffer.into_inner(), &web, &state)
            .await
            .unwrap_err();
        assert!(error.contains("服务端程序"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn status_tracks_progress_and_releases_the_update_lock() {
        let state = UpdateState::new();
        state
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        state.progress(512, 1024).await;
        let status = state.snapshot().await;
        assert_eq!(status.state, "downloading");
        assert!((status.progress - 0.5).abs() < 0.001);
        state.finish("done", "ok", "9.9.9", true).await;
        let status = state.snapshot().await;
        assert_eq!(status.state, "done");
        assert_eq!(status.progress, 1.0);
        assert_eq!(status.new_version, "9.9.9");
        assert!(status.restart_required);
        assert!(!state
            .running
            .load(std::sync::atomic::Ordering::SeqCst));
    }
}
