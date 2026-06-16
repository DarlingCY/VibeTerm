//! GitHub release update logic. Pure networking + filesystem; no UI deps.
//! The orchestration (event emission, app shutdown) is left to the caller; this
//! module only exposes the building blocks.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

pub const GITHUB_REPOSITORY: &str = "DarlingCY/VibeTerm";
pub const GITHUB_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/DarlingCY/VibeTerm/releases/latest";
pub const GITHUB_LATEST_RELEASE_PAGE: &str =
    "https://github.com/DarlingCY/VibeTerm/releases/latest";
pub const UPDATE_USER_AGENT: &str = concat!("VibeTerm/", env!("CARGO_PKG_VERSION"));
pub const MIN_INSTALLER_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub html_url: String,
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
pub struct GithubReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

pub fn normalized_release_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split_once('+')
        .map(|(version, _)| version)
        .unwrap_or_else(|| version.trim().trim_start_matches(['v', 'V']))
        .to_owned()
}

pub fn version_components(version: &str) -> Vec<u64> {
    normalized_release_version(version)
        .split(['.', '-'])
        .map(|part| {
            part.chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect()
}

pub fn is_newer_version(latest: &str, current: &str) -> bool {
    let latest = version_components(latest);
    let current = version_components(current);
    let len = latest.len().max(current.len()).max(3);
    for index in 0..len {
        let left = latest.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

pub fn preferred_update_asset(release: &GithubRelease) -> Option<&GithubReleaseAsset> {
    release
        .assets
        .iter()
        .filter(|asset| asset.name.to_ascii_lowercase().ends_with(".exe"))
        .max_by_key(|asset| {
            let name = asset.name.to_ascii_lowercase();
            let setup_score = usize::from(name.contains("setup")) * 8;
            let windows_score = usize::from(name.contains("windows") || name.contains("win")) * 4;
            let arch_score = usize::from(name.contains("x64") || name.contains("amd64")) * 2;
            setup_score + windows_score + arch_score
        })
}

fn response_body_message(body: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(message) = value.get("message").and_then(|value| value.as_str()) {
            return Some(message.trim().to_owned());
        }
    }

    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(180).collect::<String>())
}

fn describe_ureq_error(error: ureq::Error) -> anyhow::Error {
    match error {
        ureq::Error::Status(status, response) => {
            let status_text = response.status_text().to_owned();
            let body = response.into_string().unwrap_or_default();
            let message = response_body_message(&body)
                .map(|message| format!(": {message}"))
                .unwrap_or_default();
            anyhow!("HTTP {status} {status_text}{message}")
        }
        ureq::Error::Transport(error) => anyhow!("网络请求失败: {error}"),
    }
}

fn github_get(url: &str) -> Result<ureq::Response> {
    ureq::get(url)
        .set("User-Agent", UPDATE_USER_AGENT)
        .set("Accept", "application/vnd.github+json, text/html;q=0.9")
        .call()
        .map_err(describe_ureq_error)
}

pub fn fetch_latest_release() -> Result<GithubRelease> {
    match fetch_latest_release_from_api() {
        Ok(release) => Ok(release),
        Err(api_error) => fetch_latest_release_from_page()
            .with_context(|| format!("GitHub API 请求失败，页面兜底也失败。API 错误: {api_error}")),
    }
}

fn fetch_latest_release_from_api() -> Result<GithubRelease> {
    let response =
        github_get(GITHUB_LATEST_RELEASE_API).context("请求 GitHub 最新版本 API 失败")?;
    response
        .into_json::<GithubRelease>()
        .context("解析 GitHub 最新版本 API 失败")
}

fn fetch_latest_release_from_page() -> Result<GithubRelease> {
    let response =
        github_get(GITHUB_LATEST_RELEASE_PAGE).context("请求 GitHub 最新版本页面失败")?;
    let final_url = response.get_url().to_owned();
    let tag_name = release_tag_from_url(&final_url)
        .with_context(|| format!("无法从 GitHub 跳转地址识别版本号: {final_url}"))?;
    let html_url = format!("https://github.com/{GITHUB_REPOSITORY}/releases/tag/{tag_name}");
    let assets = fetch_release_assets_from_page(&tag_name).unwrap_or_default();

    Ok(GithubRelease {
        tag_name,
        html_url,
        body: None,
        assets,
    })
}

fn release_tag_from_url(url: &str) -> Option<String> {
    let marker = "/releases/tag/";
    let (_, tag) = url.split_once(marker)?;
    let tag = tag.split(['?', '#', '/']).next()?.trim();
    (!tag.is_empty()).then(|| tag.to_owned())
}

fn fetch_release_assets_from_page(tag_name: &str) -> Result<Vec<GithubReleaseAsset>> {
    let url = format!("https://github.com/{GITHUB_REPOSITORY}/releases/expanded_assets/{tag_name}");
    let html = github_get(&url)
        .context("请求 GitHub release 资产列表失败")?
        .into_string()
        .context("读取 GitHub release 资产列表失败")?;
    Ok(extract_release_assets(&html, tag_name))
}

fn extract_release_assets(html: &str, tag_name: &str) -> Vec<GithubReleaseAsset> {
    let prefix = format!("/{GITHUB_REPOSITORY}/releases/download/{tag_name}/");
    let mut assets = Vec::new();

    for href in extract_href_values(html) {
        if !href.starts_with(&prefix) || !href.to_ascii_lowercase().ends_with(".exe") {
            continue;
        }
        let Some(asset_url) = github_url_from_href(&href) else {
            continue;
        };
        let name = href
            .rsplit('/')
            .next()
            .map(percent_decode)
            .unwrap_or_else(|| asset_url.clone());
        if !assets
            .iter()
            .any(|asset: &GithubReleaseAsset| asset.browser_download_url == asset_url)
        {
            assets.push(GithubReleaseAsset {
                name,
                browser_download_url: asset_url,
            });
        }
    }

    assets
}

fn extract_href_values(html: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = html;

    while let Some(index) = rest.find("href=") {
        rest = &rest[index + "href=".len()..];
        let Some(quote) = rest.chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            continue;
        }
        rest = &rest[quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            break;
        };
        values.push(html_unescape_attribute(&rest[..end]));
        rest = &rest[end + quote.len_utf8()..];
    }

    values
}

fn github_url_from_href(href: &str) -> Option<String> {
    if href.starts_with("https://github.com/") {
        Some(href.to_owned())
    } else if href.starts_with('/') {
        Some(format!("https://github.com{href}"))
    } else {
        None
    }
}

fn html_unescape_attribute(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                output.push(((high << 4) | low) as u8);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

/// Download the installer and launch it. Returns Ok once the installer process
/// has been spawned; the caller is responsible for shutting down the app.
pub fn download_and_launch_update(version: &str, asset_url: &str, silent: bool) -> Result<()> {
    let directory = env::temp_dir().join("VibeTerm").join("updates");
    fs::create_dir_all(&directory).context("failed to create update cache directory")?;
    let installer_path = directory.join(format!(
        "vibeterm-setup-{}-windows-x64.exe",
        sanitize_filename(version)
    ));
    let download_path = installer_path.with_extension("exe.download");
    cleanup_old_update_installers(&directory, &installer_path, &download_path);

    let response = ureq::get(asset_url)
        .set("User-Agent", UPDATE_USER_AGENT)
        .call()
        .context("failed to download update")?;
    let status = response.status();
    if !(200..300).contains(&status) {
        bail!("failed to download update: HTTP {status}");
    }
    let mut reader = response.into_reader();
    let mut file = fs::File::create(&download_path).context("failed to create update file")?;
    let bytes_written = io::copy(&mut reader, &mut file).context("failed to write update file")?;
    if bytes_written < MIN_INSTALLER_BYTES {
        bail!("downloaded update file is unexpectedly small ({bytes_written} bytes)");
    }
    file.flush().ok();
    file.sync_all()
        .context("failed to flush update file to disk")?;
    drop(file);
    validate_windows_executable(&download_path)?;
    let _ = fs::remove_file(&installer_path);
    fs::rename(&download_path, &installer_path).context("failed to finalize update file")?;

    let mut command = Command::new(&installer_path);
    command.current_dir(&directory);
    if silent {
        command.args([
            "/SILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            "/RESTARTAPPLICATIONS",
        ]);
    }
    command
        .spawn()
        .with_context(|| format!("failed to launch {}", installer_path.display()))?;
    Ok(())
}

fn validate_windows_executable(path: &Path) -> Result<()> {
    let mut file = fs::File::open(path).context("failed to reopen downloaded update file")?;
    let mut signature = [0u8; 2];
    file.read_exact(&mut signature)
        .context("failed to read downloaded update file header")?;
    if signature != *b"MZ" {
        bail!("downloaded update file is not a Windows executable");
    }
    Ok(())
}

fn cleanup_old_update_installers(directory: &Path, keep_installer: &Path, keep_download: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep_installer || path == keep_download {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("vibeterm-setup-")
            && (name.ends_with("-windows-x64.exe") || name.ends_with(".exe.download"))
        {
            let _ = fs::remove_file(path);
        }
    }
}

fn sanitize_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "latest".to_owned()
    } else {
        sanitized
    }
}
