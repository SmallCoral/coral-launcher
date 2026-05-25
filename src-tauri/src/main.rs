#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures_util::{stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const BMCLAPI_VERSION_MANIFEST_URL: &str =
    "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json";
const MODRINTH_API: &str = "https://api.modrinth.com/v2";
const FABRIC_META_API: &str = "https://meta.fabricmc.net/v2";
const FORGE_PROMOTIONS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const DEFAULT_MICROSOFT_CLIENT_ID: &str = "c36a9fb6-4f2a-41ff-90bd-ae7cc92031eb";
const JAVA_RUNTIME_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LatestVersion {
    release: String,
    snapshot: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct VersionEntry {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    time: String,
    #[serde(rename = "releaseTime")]
    release_time: String,
    sha1: String,
    #[serde(rename = "complianceLevel")]
    compliance_level: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct VersionManifest {
    latest: LatestVersion,
    versions: Vec<VersionEntry>,
}

#[derive(Debug, Serialize, Clone)]
struct DownloadProgress {
    phase: String,
    current: usize,
    total: usize,
    label: String,
}

#[derive(Debug, Serialize)]
struct InstalledVersion {
    id: String,
    display_name: String,
    kind: String,
    loader: String,
    has_client: bool,
    has_manifest: bool,
    path: String,
    inherits_from: Option<String>,
    jar: Option<String>,
}

#[derive(Debug, Serialize)]
struct VersionSummary {
    id: String,
    main_class: String,
    asset_index: String,
    java_component: Option<String>,
    java_major: Option<i64>,
    libraries: usize,
    assets: Option<usize>,
    client_size: Option<i64>,
    game_arguments: usize,
    jvm_arguments: usize,
}

#[derive(Debug, Serialize, Clone)]
struct JavaInstallation {
    path: String,
    folder: String,
    version: String,
    major: i64,
    is_jdk: bool,
    is_64_bit: bool,
    source: String,
    display_name: String,
}

#[derive(Debug, Serialize)]
struct DataPaths {
    launcher_root: String,
    minecraft_root: String,
    versions_root: String,
    instances_root: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct LauncherConfig {
    minecraft_root: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OfflineLoginRequest {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: i64,
    interval: i64,
    message: Option<String>,
    #[serde(default)]
    browser_opened: bool,
    browser_open_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct LoginPollResult {
    status: String,
    message: String,
    profile: Option<MinecraftProfile>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MinecraftProfile {
    id: String,
    name: String,
    access_token: String,
    refresh_token: Option<String>,
    xuid: Option<String>,
    owns_game: bool,
    expires_in: i64,
    expires_at: Option<u64>,
    #[serde(default = "default_account_type")]
    account_type: String,
}

#[derive(Debug, Deserialize)]
struct LaunchAccount {
    name: String,
    uuid: String,
    access_token: String,
    xuid: Option<String>,
    owns_game: bool,
    #[serde(default = "default_account_type")]
    account_type: String,
}

#[derive(Debug, Serialize)]
struct LaunchResult {
    pid: Option<u32>,
    command_preview: String,
    game_directory: String,
}

#[derive(Debug, Serialize, Clone)]
struct LaunchLogEvent {
    stream: String,
    line: String,
    pid: Option<u32>,
}

#[derive(Debug, Serialize, Clone)]
struct MemoryRecommendation {
    total_mb: i64,
    available_mb: i64,
    recommended_mb: i64,
    mod_count: usize,
    modable: bool,
    reason: String,
}

#[derive(Debug, Serialize, Clone)]
struct LoaderVersionOption {
    loader: String,
    version: String,
    display_name: String,
    recommended: bool,
    stable: bool,
}

#[derive(Debug, Serialize)]
struct ModInstallResult {
    project_id: String,
    version_id: String,
    file_name: String,
    path: String,
    project_type: String,
}

fn http_client() -> Result<Client, String> {
    Client::builder()
        .user_agent("CoralLauncher/0.1.0 (contact: local-dev)")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(to_string)
}

fn to_string<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn default_account_type() -> String {
    "microsoft".to_string()
}

fn microsoft_client_id() -> String {
    std::env::var("CORAL_MS_CLIENT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_MICROSOFT_CLIENT_ID.to_string())
}

fn launcher_root() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir().ok_or("无法定位系统数据目录")?;
    Ok(data_dir.join("CoralLauncher"))
}

fn config_path() -> Result<PathBuf, String> {
    Ok(launcher_root()?.join("launcher-config.json"))
}

fn read_launcher_config() -> LauncherConfig {
    let Ok(path) = config_path() else {
        return LauncherConfig::default();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return LauncherConfig::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_launcher_config(config: &LauncherConfig) -> Result<(), String> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(to_string)?;
    }
    let content = serde_json::to_string_pretty(config).map_err(to_string)?;
    fs::write(path, content).map_err(to_string)
}

fn official_minecraft_root() -> Option<PathBuf> {
    dirs::data_dir().map(|data_dir| data_dir.join(".minecraft"))
}

fn directory_has_children(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

fn default_minecraft_root() -> Result<PathBuf, String> {
    let launcher_minecraft = launcher_root()?.join("minecraft");
    if directory_has_children(&launcher_minecraft.join("versions")) {
        return Ok(launcher_minecraft);
    }
    if let Some(root) = official_minecraft_root() {
        if root.exists() || directory_has_children(&root.join("versions")) {
            return Ok(root);
        }
    }
    Ok(launcher_minecraft)
}

fn minecraft_root() -> Result<PathBuf, String> {
    let config = read_launcher_config();
    if let Some(root) = config
        .minecraft_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(root));
    }
    default_minecraft_root()
}

fn versions_root() -> Result<PathBuf, String> {
    Ok(minecraft_root()?.join("versions"))
}

fn instances_root() -> Result<PathBuf, String> {
    Ok(minecraft_root()?.join("instances"))
}

fn ensure_minecraft_dirs(root: &Path) -> Result<(), String> {
    fs::create_dir_all(root).map_err(to_string)?;
    fs::create_dir_all(root.join("versions")).map_err(to_string)?;
    fs::create_dir_all(root.join("libraries")).map_err(to_string)?;
    fs::create_dir_all(root.join("assets")).map_err(to_string)?;
    fs::create_dir_all(root.join("mods")).map_err(to_string)?;
    Ok(())
}

fn data_paths() -> Result<DataPaths, String> {
    let minecraft = minecraft_root()?;
    Ok(DataPaths {
        launcher_root: launcher_root()?.display().to_string(),
        minecraft_root: minecraft.display().to_string(),
        versions_root: minecraft.join("versions").display().to_string(),
        instances_root: instances_root()?.display().to_string(),
    })
}

fn account_session_path() -> Result<PathBuf, String> {
    Ok(launcher_root()?.join("minecraft-account.json"))
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn save_minecraft_profile_to_disk(profile: &MinecraftProfile) -> Result<(), String> {
    let path = account_session_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(to_string)?;
    }
    let content = serde_json::to_string_pretty(profile).map_err(to_string)?;
    fs::write(path, content).map_err(to_string)
}

fn read_minecraft_profile_from_disk() -> Result<Option<MinecraftProfile>, String> {
    let path = account_session_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(to_string)?;
    serde_json::from_str::<MinecraftProfile>(&content)
        .map(Some)
        .map_err(|error| format!("本地登录状态文件损坏: {error}"))
}

fn remove_minecraft_profile_from_disk() -> Result<(), String> {
    let path = account_session_path()?;
    if path.exists() {
        fs::remove_file(path).map_err(to_string)?;
    }
    Ok(())
}

fn open_external_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("只允许打开 http/https 链接".to_string());
    }

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(trimmed);
        command
    };

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(trimmed);
        command
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(trimmed);
        command
    };

    command.spawn().map(|_| ()).map_err(to_string)
}

fn current_os_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "macos")]
    {
        "osx"
    }
}

fn current_native_key() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "natives-windows"
    }
    #[cfg(target_os = "linux")]
    {
        "natives-linux"
    }
    #[cfg(target_os = "macos")]
    {
        "natives-macos"
    }
}

fn emit_progress(
    app: &AppHandle,
    phase: &str,
    current: usize,
    total: usize,
    label: impl Into<String>,
) {
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            phase: phase.to_string(),
            current,
            total,
            label: label.into(),
        },
    );
}

fn emit_launch_log(app: &AppHandle, stream: &str, line: impl Into<String>, pid: Option<u32>) {
    let _ = app.emit(
        "launch-log",
        LaunchLogEvent {
            stream: stream.to_string(),
            line: line.into(),
            pid,
        },
    );
}

fn spawn_log_reader<R>(app: AppHandle, pid: u32, stream: &'static str, reader: R)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(line) => emit_launch_log(&app, stream, line, Some(pid)),
                Err(error) => {
                    emit_launch_log(
                        &app,
                        stream,
                        format!("读取 {stream} 失败: {error}"),
                        Some(pid),
                    );
                    break;
                }
            }
        }
    });
}

fn sanitize_command_preview(executable: &str, args: &[String], account: &LaunchAccount) -> String {
    let mut preview = format!("{} {}", executable, args.join(" "));
    for secret in [
        account.access_token.as_str(),
        account.xuid.as_deref().unwrap_or(""),
    ] {
        if !secret.is_empty() {
            preview = preview.replace(secret, "<hidden>");
        }
    }
    preview
}

fn java_platform_key() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        if cfg!(target_arch = "aarch64") {
            "windows-arm64"
        } else if cfg!(target_arch = "x86") {
            "windows-x86"
        } else {
            "windows-x64"
        }
    }
    #[cfg(target_os = "linux")]
    {
        if cfg!(target_arch = "x86") {
            "linux-i386"
        } else {
            "linux"
        }
    }
    #[cfg(target_os = "macos")]
    {
        if cfg!(target_arch = "aarch64") {
            "mac-os-arm64"
        } else {
            "mac-os"
        }
    }
}

fn java_executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    }
}

fn runtime_java_path(component: &str) -> Result<PathBuf, String> {
    Ok(launcher_root()?
        .join("runtime")
        .join(component)
        .join(java_platform_key())
        .join(component)
        .join("bin")
        .join(java_executable_name()))
}

fn parse_java_major(version_output: &str) -> Option<i64> {
    let version = version_output.split('"').nth(1).or_else(|| {
        version_output
            .split_whitespace()
            .find(|part| part.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
    })?;
    let mut numbers = version
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<i64>().ok())
        .collect::<Vec<_>>();
    if numbers.is_empty() {
        return None;
    }
    if numbers[0] == 1 && numbers.len() >= 2 {
        Some(numbers[1])
    } else {
        Some(numbers.remove(0))
    }
}

fn parse_java_version_string(version_output: &str) -> Option<String> {
    version_output
        .split('"')
        .nth(1)
        .map(|value| value.trim().replace('_', "."))
        .filter(|value| !value.is_empty())
        .or_else(|| {
            version_output
                .split_whitespace()
                .find(|part| part.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
                .map(|value| {
                    value
                        .trim_matches(|ch: char| {
                            !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-')
                        })
                        .replace('_', ".")
                })
                .filter(|value| !value.is_empty())
        })
}

fn java_major_version(executable: &str) -> Result<Option<i64>, String> {
    let mut command = Command::new(executable);
    command.arg("-version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .output()
        .map_err(|error| format!("无法运行 Java 版本检查 `{executable} -version`: {error}"))?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(parse_java_major(&text))
}

fn java_version_output(executable: &Path) -> Result<String, String> {
    let mut command = Command::new(executable);
    command.arg("-version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().map_err(|error| {
        format!(
            "无法运行 Java 版本检查 `{}` -version: {error}",
            executable.display()
        )
    })?;
    Ok(format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn java_executable_from_path(path: &Path) -> Option<PathBuf> {
    let java_name = java_executable_name();
    if path.is_file() {
        let file_name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        if file_name == java_name || file_name == "javaw.exe" || file_name == "java" {
            if file_name == "javaw.exe" {
                let java = path.with_file_name("java.exe");
                if java.exists() {
                    return Some(java);
                }
            }
            return Some(path.to_path_buf());
        }
    }
    if path.is_dir() {
        for candidate in [path.join(java_name), path.join("bin").join(java_name)] {
            if candidate.exists() {
                return Some(candidate);
            }
        }
        #[cfg(target_os = "windows")]
        {
            for candidate in [path.join("javaw.exe"), path.join("bin").join("javaw.exe")] {
                if candidate.exists() {
                    let java = candidate.with_file_name("java.exe");
                    if java.exists() {
                        return Some(java);
                    }
                }
            }
        }
    }
    None
}

fn normalized_path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn add_java_candidate(
    candidates: &mut HashMap<String, (PathBuf, String)>,
    path: PathBuf,
    source: &str,
) {
    if let Some(executable) = java_executable_from_path(&path) {
        let key = normalized_path_key(&executable);
        candidates
            .entry(key)
            .or_insert_with(|| (executable, source.to_string()));
    }
}

fn java_search_relevant(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "java",
        "jdk",
        "jre",
        "jvm",
        "runtime",
        "adoptium",
        "temurin",
        "corretto",
        "zulu",
        "bellsoft",
        "liberica",
        "microsoft",
        "oracle",
        "jetbrains",
        "jbr",
        "graal",
        "dragonwell",
        "semeru",
        "mc",
        "minecraft",
        "pcl",
        "hmcl",
        "launcher",
        "program",
        "software",
        "soft",
        "env",
        "cache",
        "游戏",
        "软件",
        "环境",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
        || lower
            .chars()
            .next()
            .map(|ch| ch.is_ascii_digit())
            .unwrap_or(false)
}

fn search_java_folder(
    root: &Path,
    candidates: &mut HashMap<String, (PathBuf, String)>,
    depth: usize,
    full_search: bool,
    source: &str,
) {
    add_java_candidate(candidates, root.to_path_buf(), source);
    if depth == 0 || !root.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if full_search || java_search_relevant(&name) {
            search_java_folder(
                &entry.path(),
                candidates,
                depth.saturating_sub(1),
                false,
                source,
            );
        }
    }
}

fn add_environment_java_candidates(candidates: &mut HashMap<String, (PathBuf, String)>) {
    if let Some(paths) = std::env::var_os("PATH") {
        for path in std::env::split_paths(&paths) {
            add_java_candidate(candidates, path, "PATH");
        }
    }
    for name in ["JAVA_HOME", "JDK_HOME", "JRE_HOME"] {
        if let Some(path) = std::env::var_os(name) {
            add_java_candidate(candidates, PathBuf::from(path), name);
        }
    }
}

fn common_java_roots() -> Vec<(PathBuf, usize, bool, &'static str)> {
    let mut roots = Vec::new();
    for env_name in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(value) = std::env::var_os(env_name) {
            let root = PathBuf::from(value);
            roots.push((root.clone(), 3, false, env_name));
            for child in [
                "Java",
                "Eclipse Adoptium",
                "Microsoft",
                "Amazon Corretto",
                "BellSoft",
                "Zulu",
                "JetBrains",
                "Semeru",
                "RedHat",
                "Programs",
            ] {
                roots.push((root.join(child), 4, true, env_name));
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        roots.push((home.join(".jdks"), 4, true, "用户 .jdks"));
        roots.push((
            home.join(".sdkman").join("candidates").join("java"),
            4,
            true,
            "SDKMAN",
        ));
    }
    if let Ok(root) = launcher_root() {
        roots.push((root.join("runtime"), 6, true, "启动器 Runtime"));
    }
    if let Ok(root) = minecraft_root() {
        roots.push((root.join("runtime"), 6, true, ".minecraft Runtime"));
        if let Some(parent) = root.parent() {
            roots.push((parent.to_path_buf(), 3, false, ".minecraft 附近"));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push((parent.to_path_buf(), 4, true, "启动器目录"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        for drive in 'C'..='Z' {
            let root = PathBuf::from(format!("{drive}:\\"));
            if root.exists() {
                roots.push((root, 4, false, "磁盘搜索"));
            }
        }
    }
    roots
}

fn inspect_java_installation(path: &Path, source: &str) -> Option<JavaInstallation> {
    let executable = java_executable_from_path(path)?;
    let output = java_version_output(&executable).ok()?;
    let major = parse_java_major(&output)?;
    let version = parse_java_version_string(&output).unwrap_or_else(|| major.to_string());
    let folder = executable.parent()?.to_path_buf();
    let is_jdk = folder
        .join(if cfg!(target_os = "windows") {
            "javac.exe"
        } else {
            "javac"
        })
        .exists();
    let output_lower = output.to_ascii_lowercase();
    let path_lower = executable.display().to_string().to_ascii_lowercase();
    let is_64_bit = output_lower.contains("64-bit")
        || output_lower.contains("64 bit")
        || (!path_lower.contains("program files (x86)") && cfg!(target_pointer_width = "64"));
    let kind = if is_jdk { "JDK" } else { "JRE" };
    let bit = if is_64_bit { "64 位" } else { "32 位" };
    Some(JavaInstallation {
        path: executable.display().to_string(),
        folder: folder.display().to_string(),
        version: version.clone(),
        major,
        is_jdk,
        is_64_bit,
        source: source.to_string(),
        display_name: format!("{kind} {major} ({version}) · {bit}"),
    })
}

fn scan_java_installations_sync() -> Result<Vec<JavaInstallation>, String> {
    let mut candidates = HashMap::<String, (PathBuf, String)>::new();
    add_environment_java_candidates(&mut candidates);
    for (root, depth, full_search, source) in common_java_roots() {
        search_java_folder(&root, &mut candidates, depth, full_search, source);
    }
    let mut installations = candidates
        .into_values()
        .filter_map(|(path, source)| inspect_java_installation(&path, &source))
        .collect::<Vec<_>>();
    installations.sort_by_key(|java| {
        (
            java.major,
            if java.is_64_bit { 0 } else { 1 },
            if java.is_jdk { 1 } else { 0 },
            java.path.clone(),
        )
    });
    installations.dedup_by(|left, right| {
        normalized_path_key(Path::new(&left.path)) == normalized_path_key(Path::new(&right.path))
    });
    Ok(installations)
}

fn java_score(java: &JavaInstallation, required_major: i64) -> (i64, i32, i32, i32, String) {
    let distance = if java.major >= required_major {
        java.major - required_major
    } else {
        10_000 + (required_major - java.major)
    };
    let bit_penalty = if java.is_64_bit { 0 } else { 1 };
    let jdk_penalty = if java.is_jdk { 1 } else { 0 };
    let source_penalty = if java.source.contains("启动器") || java.source.contains(".minecraft")
    {
        0
    } else if java.source == "JAVA_HOME" || java.source == "JDK_HOME" || java.source == "JRE_HOME" {
        1
    } else if java.source == "PATH" {
        2
    } else {
        3
    };
    (
        distance,
        bit_penalty,
        jdk_penalty,
        source_penalty,
        java.path.clone(),
    )
}

fn select_best_java(
    installations: &[JavaInstallation],
    required_major: i64,
) -> Option<JavaInstallation> {
    installations
        .iter()
        .filter(|java| java.major >= required_major)
        .min_by_key(|java| java_score(java, required_major))
        .cloned()
}

fn java_component_for_version(version_json: &Value) -> Option<String> {
    version_json
        .pointer("/javaVersion/component")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            match version_json
                .pointer("/javaVersion/majorVersion")
                .and_then(Value::as_i64)
                .or(Some(8))
            {
                Some(major) if major >= 25 => Some("java-runtime-epsilon".to_string()),
                Some(major) if major >= 21 => Some("java-runtime-delta".to_string()),
                Some(major) if major >= 17 => Some("java-runtime-gamma".to_string()),
                Some(major) if major >= 16 => Some("java-runtime-alpha".to_string()),
                Some(_) => Some("jre-legacy".to_string()),
                None => None,
            }
        })
}

fn required_java_major(version_json: &Value) -> Option<i64> {
    version_json
        .pointer("/javaVersion/majorVersion")
        .and_then(Value::as_i64)
}

fn required_java_major_or_default(version_json: &Value) -> i64 {
    required_java_major(version_json).unwrap_or(8)
}

fn manifest_urls() -> Vec<String> {
    vec![
        BMCLAPI_VERSION_MANIFEST_URL.to_string(),
        VERSION_MANIFEST_URL.to_string(),
    ]
}

fn mirror_urls(url: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for (from, to) in [
        (
            "https://piston-meta.mojang.com/",
            "https://bmclapi2.bangbang93.com/",
        ),
        (
            "https://launchermeta.mojang.com/",
            "https://bmclapi2.bangbang93.com/",
        ),
        (
            "https://piston-data.mojang.com/",
            "https://bmclapi2.bangbang93.com/",
        ),
        (
            "https://libraries.minecraft.net/",
            "https://bmclapi2.bangbang93.com/maven/",
        ),
        (
            "https://maven.fabricmc.net/",
            "https://bmclapi2.bangbang93.com/maven/",
        ),
        (
            "https://maven.minecraftforge.net/",
            "https://bmclapi2.bangbang93.com/maven/",
        ),
        (
            "https://resources.download.minecraft.net/",
            "https://bmclapi2.bangbang93.com/assets/",
        ),
    ] {
        if let Some(rest) = url.strip_prefix(from) {
            urls.push(format!("{to}{rest}"));
        }
    }
    urls.push(url.to_string());
    urls.dedup();
    urls
}

async fn fetch_json_from_urls<T: serde::de::DeserializeOwned>(
    client: &Client,
    urls: Vec<String>,
) -> Result<T, String> {
    let mut errors = Vec::new();
    for url in urls.iter() {
        for attempt in 1..=3 {
            match client.get(url).send().await {
                Ok(response) if response.status().is_success() => {
                    let text = response.text().await.map_err(to_string)?;
                    return serde_json::from_str::<T>(&text)
                        .map_err(|error| format!("JSON 解析失败 {url}: {error}"));
                }
                Ok(response) => {
                    let status = response.status();
                    errors.push(format!("{url} 第 {attempt} 次请求失败: {status}"));
                    if status.as_u16() == 403 || status.as_u16() == 404 {
                        break;
                    }
                }
                Err(error) => {
                    errors.push(format!("{url} 第 {attempt} 次请求失败: {error}"));
                }
            }
            tokio::time::sleep(Duration::from_millis(350 * attempt)).await;
        }
    }
    Err(format!("所有下载源均失败：{}", errors.join("；")))
}

async fn fetch_text_from_urls(client: &Client, urls: Vec<String>) -> Result<String, String> {
    let mut errors = Vec::new();
    for url in urls.iter() {
        for attempt in 1..=3 {
            match client.get(url).send().await {
                Ok(response) if response.status().is_success() => {
                    return response.text().await.map_err(to_string);
                }
                Ok(response) => {
                    let status = response.status();
                    errors.push(format!("{url} 第 {attempt} 次请求失败: {status}"));
                    if status.as_u16() == 403 || status.as_u16() == 404 {
                        break;
                    }
                }
                Err(error) => {
                    errors.push(format!("{url} 第 {attempt} 次请求失败: {error}"));
                }
            }
            tokio::time::sleep(Duration::from_millis(350 * attempt)).await;
        }
    }
    Err(format!("所有文本源均失败：{}", errors.join("；")))
}

fn sha1_matches(path: &Path, expected: &str) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let mut file = fs::File::open(path).map_err(to_string)?;
    let mut hasher = Sha1::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(to_string)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected))
}

fn file_is_valid(
    path: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<i64>,
    is_json: bool,
) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    if let Some(size) = expected_size {
        if size >= 0 {
            let actual = fs::metadata(path).map_err(to_string)?.len() as i64;
            if actual != size {
                return Ok(false);
            }
        }
    }
    if let Some(hash) = expected_sha1 {
        if !sha1_matches(path, hash)? {
            return Ok(false);
        }
    }
    if is_json {
        let content = fs::read_to_string(path).map_err(to_string)?;
        if serde_json::from_str::<Value>(&content).is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn download_to_path(
    client: &Client,
    urls: Vec<String>,
    path: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<i64>,
    is_json: bool,
) -> Result<(), String> {
    if file_is_valid(path, expected_sha1, expected_size, is_json)? {
        return Ok(());
    }
    if path.exists() {
        let _ = fs::remove_file(path);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(to_string)?;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    let temp_path = path.with_file_name(format!(
        "{file_name}.{}.{}.part",
        std::process::id(),
        unix_timestamp()
    ));
    let mut errors = Vec::new();

    for url in urls {
        for attempt in 1..=3 {
            let result = async {
                let response = client.get(&url).send().await.map_err(to_string)?;
                let status = response.status();
                if !status.is_success() {
                    return Err(format!("下载失败 {status}: {url}"));
                }
                let bytes = response.bytes().await.map_err(to_string)?;
                if let Some(size) = expected_size {
                    if size >= 0 && bytes.len() as i64 != size {
                        return Err(format!(
                            "文件大小不匹配: {}，期望 {}，实际 {}",
                            path.display(),
                            size,
                            bytes.len()
                        ));
                    }
                }
                tokio::fs::write(&temp_path, &bytes)
                    .await
                    .map_err(to_string)?;
                if !file_is_valid(&temp_path, expected_sha1, expected_size, is_json)? {
                    return Err(format!("文件校验失败: {}", path.display()));
                }
                if path.exists() {
                    let _ = tokio::fs::remove_file(path).await;
                }
                tokio::fs::rename(&temp_path, path)
                    .await
                    .map_err(to_string)?;
                Ok::<_, String>(())
            }
            .await;

            match result {
                Ok(()) => return Ok(()),
                Err(error) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    errors.push(format!("{url} 第 {attempt} 次失败: {error}"));
                }
            }
            tokio::time::sleep(Duration::from_millis(350 * attempt)).await;
        }
    }
    Err(format!("下载失败：{}", errors.join("；")))
}

async fn download_java_runtime(
    app: &AppHandle,
    client: &Client,
    component: &str,
) -> Result<PathBuf, String> {
    let platform = java_platform_key();
    emit_launch_log(
        app,
        "info",
        format!("正在获取 Mojang Java Runtime: {component} / {platform}"),
        None,
    );
    let all =
        fetch_json_from_urls::<Value>(client, vec![JAVA_RUNTIME_MANIFEST_URL.to_string()]).await?;
    let runtimes = all
        .get(platform)
        .and_then(|value| value.get(component))
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Mojang 未提供 {platform} 的 {component} 运行时"))?;
    let runtime = runtimes
        .first()
        .ok_or_else(|| format!("Mojang Java Runtime 清单为空: {component} / {platform}"))?;
    let manifest_url = runtime
        .pointer("/manifest/url")
        .and_then(Value::as_str)
        .ok_or("Java Runtime 条目缺少 manifest.url")?;
    let runtime_version = runtime
        .pointer("/version/name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    emit_launch_log(
        app,
        "info",
        format!("正在准备 Java Runtime {component} {runtime_version}"),
        None,
    );

    let manifest = fetch_json_from_urls::<Value>(client, mirror_urls(manifest_url)).await?;
    let files = manifest
        .get("files")
        .and_then(Value::as_object)
        .ok_or("Java Runtime manifest 缺少 files")?;
    let runtime_root = launcher_root()?
        .join("runtime")
        .join(component)
        .join(platform)
        .join(component);

    let mut downloads = Vec::<DownloadFile>::new();
    for (relative, entry) in files {
        let path = runtime_root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        match entry.get("type").and_then(Value::as_str) {
            Some("directory") => {
                fs::create_dir_all(&path).map_err(to_string)?;
            }
            Some("file") => {
                let raw = entry
                    .pointer("/downloads/raw")
                    .ok_or_else(|| format!("Java Runtime 文件缺少 raw 下载信息: {relative}"))?;
                let url = raw
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("Java Runtime 文件缺少 URL: {relative}"))?;
                downloads.push(DownloadFile {
                    urls: mirror_urls(url),
                    path,
                    sha1: raw
                        .get("sha1")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    size: raw.get("size").and_then(Value::as_i64),
                    label: relative.clone(),
                    is_json: false,
                });
            }
            Some(other) => {
                emit_launch_log(
                    app,
                    "info",
                    format!("跳过暂不支持的 Java Runtime 文件类型 {other}: {relative}"),
                    None,
                );
            }
            None => {}
        }
    }

    let total = downloads.len();
    let done = Arc::new(AtomicUsize::new(0));
    let results = stream::iter(downloads.into_iter())
        .map(|file| {
            let client = client.clone();
            let app = app.clone();
            let done = done.clone();
            async move {
                let result = download_to_path(
                    &client,
                    file.urls,
                    &file.path,
                    file.sha1.as_deref(),
                    file.size,
                    file.is_json,
                )
                .await;
                let current = done.fetch_add(1, Ordering::SeqCst) + 1;
                if current == 1 || current == total || current % 20 == 0 || result.is_err() {
                    emit_launch_log(
                        &app,
                        "info",
                        format!("Java Runtime 下载进度: {current}/{total} ({})", file.label),
                        None,
                    );
                }
                result.map_err(|error| format!("{}: {error}", file.label))
            }
        })
        .buffer_unordered(16)
        .collect::<Vec<_>>()
        .await;
    let failed = results
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        return Err(format!(
            "{} 个 Java Runtime 文件下载失败：{}",
            failed.len(),
            failed.into_iter().take(4).collect::<Vec<_>>().join("; ")
        ));
    }

    let java = runtime_java_path(component)?;
    if !java.exists() {
        return Err(format!(
            "Java Runtime 下载完成但未找到 Java: {}",
            java.display()
        ));
    }
    emit_launch_log(
        app,
        "info",
        format!("Java Runtime 已就绪: {}", java.display()),
        None,
    );
    Ok(java)
}

async fn prepare_java_executable(
    app: &AppHandle,
    version_json: &Value,
    requested_java: &str,
) -> Result<String, String> {
    let requested = requested_java.trim();
    let auto_mode = requested.is_empty() || requested.eq_ignore_ascii_case("auto");
    let requested = if auto_mode { "auto" } else { requested };
    let required_major = required_java_major_or_default(version_json);
    let component = java_component_for_version(version_json);

    if !auto_mode {
        match java_major_version(requested) {
            Ok(Some(major)) => {
                emit_launch_log(
                    app,
                    "info",
                    format!("检测到手动指定 Java `{requested}` 版本: {major}"),
                    None,
                );
                if major >= required_major {
                    return Ok(requested.to_string());
                }
                emit_launch_log(
                    app,
                    "info",
                    format!(
                        "手动指定的 Java 版本 {major} 不满足当前版本要求 Java {required_major}+，改用自动选择"
                    ),
                    None,
                );
            }
            Ok(None) => emit_launch_log(
                app,
                "info",
                format!("无法识别手动指定 Java `{requested}` 的版本，改用自动选择"),
                None,
            ),
            Err(error) => emit_launch_log(app, "info", format!("{error}，改用自动选择"), None),
        }
    } else {
        emit_launch_log(
            app,
            "info",
            format!("Java 模式: 自动选择，当前版本要求 Java {required_major}+"),
            None,
        );
    }

    match scan_java_installations_sync() {
        Ok(installations) => {
            emit_launch_log(
                app,
                "info",
                format!("本机 Java 扫描完成，发现 {} 个可用项", installations.len()),
                None,
            );
            if let Some(java) = select_best_java(&installations, required_major) {
                emit_launch_log(
                    app,
                    "info",
                    format!("自动选择 Java: {} · {}", java.display_name, java.path),
                    None,
                );
                return Ok(java.path);
            }
            emit_launch_log(
                app,
                "info",
                format!("未找到满足 Java {required_major}+ 的本机 Java，准备使用 Mojang Runtime"),
                None,
            );
        }
        Err(error) => emit_launch_log(app, "info", format!("扫描本机 Java 失败: {error}"), None),
    }

    let component = component.ok_or("版本 JSON 未声明 javaVersion，且当前 Java 不可用")?;
    if let Ok(java_path) = runtime_java_path(&component) {
        if java_path.exists() {
            let java = java_path.display().to_string();
            if let Ok(Some(major)) = java_major_version(&java) {
                if major >= required_major {
                    emit_launch_log(
                        app,
                        "info",
                        format!("使用已安装的 Mojang Java Runtime: {java}"),
                        None,
                    );
                    return Ok(java);
                }
            }
        }
    }

    let java = download_java_runtime(app, &http_client()?, &component).await?;
    Ok(java.display().to_string())
}

fn resolve_java_for_preview(version_json: &Value, requested_java: &str) -> String {
    let requested = requested_java.trim();
    let auto_mode = requested.is_empty() || requested.eq_ignore_ascii_case("auto");
    let required_major = required_java_major_or_default(version_json);
    if !auto_mode {
        if let Ok(Some(major)) = java_major_version(requested) {
            if major >= required_major {
                return requested.to_string();
            }
        }
    }
    if let Ok(installations) = scan_java_installations_sync() {
        if let Some(java) = select_best_java(&installations, required_major) {
            return java.path;
        }
    }
    if let Some(component) = java_component_for_version(version_json) {
        if let Ok(java_path) = runtime_java_path(&component) {
            if java_path.exists() {
                return java_path.display().to_string();
            }
        }
    }
    "java".to_string()
}

fn rule_matches(rule: &Value) -> bool {
    if rule.get("features").is_some() {
        return false;
    }

    if let Some(os) = rule.get("os") {
        if let Some(name) = os.get("name").and_then(Value::as_str) {
            if name != current_os_name() {
                return false;
            }
        }
        if let Some(arch) = os.get("arch").and_then(Value::as_str) {
            let current = if cfg!(target_arch = "x86_64") {
                "x64"
            } else {
                "x86"
            };
            if arch != current {
                return false;
            }
        }
    }
    true
}

fn rules_allow(rules: Option<&Value>) -> bool {
    let Some(Value::Array(rules)) = rules else {
        return true;
    };

    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule) {
            allowed = rule
                .get("action")
                .and_then(Value::as_str)
                .map(|action| action == "allow")
                .unwrap_or(false);
        }
    }
    allowed
}

fn replace_tokens(input: &str, replacements: &HashMap<&str, String>) -> String {
    let mut output = input.to_string();
    for (key, value) in replacements {
        output = output.replace(&format!("${{{key}}}"), value);
    }
    output
}

fn collect_arguments(items: Option<&Value>, replacements: &HashMap<&str, String>) -> Vec<String> {
    let Some(Value::Array(items)) = items else {
        return Vec::new();
    };

    let mut args = Vec::new();
    for item in items {
        match item {
            Value::String(value) => args.push(replace_tokens(value, replacements)),
            Value::Object(object) if rules_allow(object.get("rules")) => {
                match object.get("value") {
                    Some(Value::String(value)) => args.push(replace_tokens(value, replacements)),
                    Some(Value::Array(values)) => {
                        for value in values.iter().filter_map(Value::as_str) {
                            args.push(replace_tokens(value, replacements));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    args
}

fn artifact_path(root: &Path, artifact: &Value) -> Option<PathBuf> {
    artifact.get("path").and_then(Value::as_str).map(|path| {
        root.join("libraries")
            .join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
    })
}

fn library_name_to_path(root: &Path, name: &str, classifier: Option<&str>) -> Option<PathBuf> {
    let mut parts = name.split(':').collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }

    let group = parts.remove(0).replace('.', "/");
    let artifact = parts.remove(0);
    let version = parts.remove(0);
    let mut file_classifier = classifier.map(ToOwned::to_owned);
    let mut extension = "jar".to_string();

    if let Some(extra) = parts.first().copied() {
        let (extra_classifier, extra_extension) = extra
            .split_once('@')
            .map(|(left, right)| (left, right))
            .unwrap_or((extra, "jar"));
        if file_classifier.is_none() && !extra_classifier.is_empty() {
            file_classifier = Some(extra_classifier.to_string());
        }
        extension = extra_extension.to_string();
    }

    let classifier = file_classifier
        .filter(|value| !value.is_empty())
        .map(|value| format!("-{value}"))
        .unwrap_or_default();
    let relative =
        format!("{group}/{artifact}/{version}/{artifact}-{version}{classifier}.{extension}");
    Some(
        root.join("libraries")
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR)),
    )
}

fn library_name_to_relative_path(name: &str, classifier: Option<&str>) -> Option<String> {
    let mut parts = name.split(':').collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }

    let group = parts.remove(0).replace('.', "/");
    let artifact = parts.remove(0);
    let version = parts.remove(0);
    let mut file_classifier = classifier.map(ToOwned::to_owned);
    let mut extension = "jar".to_string();

    if let Some(extra) = parts.first().copied() {
        let (extra_classifier, extra_extension) = extra
            .split_once('@')
            .map(|(left, right)| (left, right))
            .unwrap_or((extra, "jar"));
        if file_classifier.is_none() && !extra_classifier.is_empty() {
            file_classifier = Some(extra_classifier.to_string());
        }
        extension = extra_extension.to_string();
    }

    let classifier = file_classifier
        .filter(|value| !value.is_empty())
        .map(|value| format!("-{value}"))
        .unwrap_or_default();
    Some(format!(
        "{group}/{artifact}/{version}/{artifact}-{version}{classifier}.{extension}"
    ))
}

fn library_maven_url(library: &Value, classifier: Option<&str>) -> Option<String> {
    let name = library.get("name").and_then(Value::as_str)?;
    let relative = library_name_to_relative_path(name, classifier)?;
    let base = library
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("https://libraries.minecraft.net/");
    Some(format!("{}/{}", base.trim_end_matches('/'), relative))
}

fn library_download_file(root: &Path, library: &Value) -> Option<DownloadFile> {
    if let Some(artifact) = library.pointer("/downloads/artifact") {
        if let (Some(url), Some(path)) = (
            artifact.get("url").and_then(Value::as_str),
            artifact_path(root, artifact),
        ) {
            return Some(DownloadFile {
                urls: mirror_urls(url),
                path,
                sha1: artifact
                    .get("sha1")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                size: artifact.get("size").and_then(Value::as_i64),
                label: library
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("library")
                    .to_string(),
                is_json: false,
            });
        }
    }

    let name = library.get("name").and_then(Value::as_str)?;
    let url = library_maven_url(library, None)?;
    let path = library_name_to_path(root, name, None)?;
    Some(DownloadFile {
        urls: mirror_urls(&url),
        path,
        sha1: library
            .get("sha1")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        size: library.get("size").and_then(Value::as_i64),
        label: name.to_string(),
        is_json: false,
    })
}

fn library_name_parts(library: &Value) -> Option<Vec<&str>> {
    library
        .get("name")
        .and_then(Value::as_str)
        .map(|name| name.split(':').collect::<Vec<_>>())
        .filter(|parts| parts.len() >= 3)
}

fn library_identity(library: &Value) -> String {
    if let Some(parts) = library_name_parts(library) {
        let classifier = parts.get(3).copied().unwrap_or("");
        if classifier.is_empty() {
            format!("{}:{}", parts[0], parts[1])
        } else {
            format!("{}:{}:{classifier}", parts[0], parts[1])
        }
    } else {
        library
            .pointer("/downloads/artifact/path")
            .and_then(Value::as_str)
            .map(|path| {
                let normalized = path.replace('\\', "/");
                normalized
                    .rsplit_once('/')
                    .map(|(parent, _)| parent.to_string())
                    .unwrap_or(normalized)
            })
            .unwrap_or_else(|| library.to_string())
    }
}

fn library_version(library: &Value) -> String {
    library_name_parts(library)
        .and_then(|parts| parts.get(2).map(|value| value.to_string()))
        .or_else(|| {
            library
                .pointer("/downloads/artifact/path")
                .and_then(Value::as_str)
                .and_then(|path| {
                    path.replace('\\', "/")
                        .split('/')
                        .rev()
                        .nth(1)
                        .map(ToOwned::to_owned)
                })
        })
        .unwrap_or_default()
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = left
        .split(|ch: char| !(ch.is_ascii_alphanumeric()))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let right_parts = right
        .split(|ch: char| !(ch.is_ascii_alphanumeric()))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    for index in 0..left_parts.len().max(right_parts.len()) {
        let left_part = left_parts.get(index).copied().unwrap_or("0");
        let right_part = right_parts.get(index).copied().unwrap_or("0");
        let ordering = match (left_part.parse::<i64>(), right_part.parse::<i64>()) {
            (Ok(left_num), Ok(right_num)) => left_num.cmp(&right_num),
            _ => left_part.cmp(right_part),
        };
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    std::cmp::Ordering::Equal
}

fn dedupe_libraries(libraries: Vec<Value>) -> Vec<Value> {
    let mut result = Vec::<Value>::new();
    let mut index_by_identity = HashMap::<String, usize>::new();

    for library in libraries {
        let identity = library_identity(&library);
        if let Some(index) = index_by_identity.get(&identity).copied() {
            let current_version = library_version(&library);
            let existing_version = library_version(&result[index]);
            if compare_versions(&current_version, &existing_version) != std::cmp::Ordering::Less {
                result[index] = library;
            }
        } else {
            index_by_identity.insert(identity, result.len());
            result.push(library);
        }
    }

    result
}

fn version_libraries(version_json: &Value) -> Vec<Value> {
    dedupe_libraries(
        version_json
            .get("libraries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    )
}

fn standalone_native_classifier(library: &Value) -> Option<&str> {
    library_name_parts(library).and_then(|parts| {
        parts
            .get(3)
            .copied()
            .and_then(|classifier| classifier.split('@').next())
            .filter(|classifier| classifier.starts_with("natives-"))
    })
}

fn native_classifier_matches_current(classifier: &str) -> bool {
    let Some(suffix) = classifier.strip_prefix("natives-") else {
        return false;
    };
    let os = match current_os_name() {
        "osx" => "macos",
        name => name,
    };
    if suffix == os {
        return true;
    }
    let Some(arch) = suffix.strip_prefix(&format!("{os}-")) else {
        return false;
    };
    match arch {
        "x64" | "x86_64" => cfg!(target_arch = "x86_64"),
        "x86" | "i386" => cfg!(target_arch = "x86"),
        "arm64" | "aarch64" => cfg!(target_arch = "aarch64"),
        _ => false,
    }
}

fn library_artifact_path(root: &Path, library: &Value) -> Option<PathBuf> {
    library
        .pointer("/downloads/artifact")
        .and_then(|artifact| artifact_path(root, artifact))
        .or_else(|| {
            library
                .get("name")
                .and_then(Value::as_str)
                .and_then(|name| library_name_to_path(root, name, None))
        })
}

fn library_native_key(library: &Value) -> String {
    library
        .get("natives")
        .and_then(|value| value.get(current_os_name()))
        .and_then(Value::as_str)
        .map(|key| key.replace("${arch}", "64"))
        .unwrap_or_else(|| current_native_key().to_string())
}

fn library_native_path(root: &Path, library: &Value) -> Option<PathBuf> {
    if let Some(classifier) = standalone_native_classifier(library) {
        if !native_classifier_matches_current(classifier) {
            return None;
        }
        return library_artifact_path(root, library);
    }

    let native_key = library_native_key(library);
    library
        .pointer("/downloads/classifiers")
        .and_then(|value| value.get(&native_key))
        .and_then(|classifier| artifact_path(root, classifier))
        .or_else(|| {
            if library.get("natives").is_none() {
                return None;
            }
            library
                .get("name")
                .and_then(Value::as_str)
                .and_then(|name| library_name_to_path(root, name, Some(&native_key)))
        })
}

fn native_extract_excludes(library: &Value) -> Vec<String> {
    library
        .pointer("/extract/exclude")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|item| item.replace('\\', "/"))
                .collect()
        })
        .unwrap_or_default()
}

fn extract_native_jar(
    jar_path: &Path,
    output_dir: &Path,
    excludes: &[String],
) -> Result<(), String> {
    fs::create_dir_all(output_dir).map_err(to_string)?;
    let file = fs::File::open(jar_path).map_err(to_string)?;
    let mut archive = zip::ZipArchive::new(file).map_err(to_string)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(to_string)?;
        let name = entry.name().replace('\\', "/");
        if name.starts_with("META-INF/")
            || name.ends_with('/')
            || excludes.iter().any(|exclude| name.starts_with(exclude))
        {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name().map(|path| path.to_owned()) else {
            continue;
        };
        let output = output_dir.join(enclosed);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(to_string)?;
        }
        let mut outfile = fs::File::create(output).map_err(to_string)?;
        std::io::copy(&mut entry, &mut outfile).map_err(to_string)?;
    }
    Ok(())
}

async fn load_manifest() -> Result<VersionManifest, String> {
    fetch_json_from_urls(&http_client()?, manifest_urls()).await
}

async fn load_version_json(client: &Client, version_id: &str) -> Result<Value, String> {
    let manifest = fetch_json_from_urls::<VersionManifest>(client, manifest_urls()).await?;
    let version = manifest
        .versions
        .iter()
        .find(|item| item.id == version_id)
        .ok_or_else(|| format!("找不到版本 {version_id}"))?;
    fetch_json_from_urls(client, mirror_urls(&version.url)).await
}

fn version_jar_path(version_id: &str) -> Result<PathBuf, String> {
    Ok(versions_root()?
        .join(version_id)
        .join(format!("{version_id}.jar")))
}

fn version_dir_path(version_id: &str) -> Result<PathBuf, String> {
    Ok(versions_root()?.join(version_id))
}

fn find_version_json_path(version_id: &str) -> Result<Option<PathBuf>, String> {
    let dir = version_dir_path(version_id)?;
    let exact = dir.join(format!("{version_id}.json"));
    if exact.exists() {
        return Ok(Some(exact));
    }
    if !dir.exists() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(&dir).map_err(to_string)? {
        let entry = entry.map_err(to_string)?;
        if !entry.file_type().map_err(to_string)?.is_file() {
            continue;
        }
        let path = entry.path();
        if path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
        {
            candidates.push(path);
        }
    }
    candidates.sort();
    Ok(candidates.into_iter().next())
}

fn read_installed_version_json(version_id: &str) -> Result<Value, String> {
    let path = find_version_json_path(version_id)?
        .ok_or_else(|| format!("版本 {version_id} 缺少 JSON 描述"))?;
    let text = fs::read_to_string(path).map_err(to_string)?;
    serde_json::from_str(&text).map_err(to_string)
}

fn version_parent_id(version_json: &Value) -> Option<String> {
    version_json
        .get("inheritsFrom")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn version_jar_id(version_id: &str, version_json: &Value) -> String {
    version_json
        .get("jar")
        .and_then(Value::as_str)
        .or_else(|| version_json.get("clientVersion").and_then(Value::as_str))
        .or_else(|| version_json.get("inheritsFrom").and_then(Value::as_str))
        .unwrap_or(version_id)
        .to_string()
}

fn primary_jar_path(version_id: &str, version_json: &Value) -> Result<PathBuf, String> {
    let direct = version_jar_path(version_id)?;
    if direct.exists() {
        return Ok(direct);
    }

    let jar_id = version_jar_id(version_id, version_json);
    let jar = version_jar_path(&jar_id)?;
    if jar.exists() || jar_id != version_id {
        return Ok(jar);
    }

    if let Some(parent) = version_parent_id(version_json) {
        let parent_json = read_installed_version_json(&parent)?;
        return primary_jar_path(&parent, &parent_json);
    }
    Ok(direct)
}

fn merge_objects(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                if key == "libraries" {
                    continue;
                }
                match base_map.get_mut(key) {
                    Some(existing) => merge_objects(existing, value),
                    None => {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value.clone();
        }
    }
}

fn pointer_array(value: &Value, pointer: &str) -> Vec<Value> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn set_argument_array(root: &mut Value, key: &str, values: Vec<Value>) {
    if values.is_empty() {
        return;
    }
    if !root.get("arguments").map(Value::is_object).unwrap_or(false) {
        root["arguments"] = json!({});
    }
    root["arguments"][key] = Value::Array(values);
}

fn merge_version_json(parent: Value, child: Value) -> Value {
    let parent_libraries = pointer_array(&parent, "/libraries");
    let child_libraries = pointer_array(&child, "/libraries");
    let parent_game_args = pointer_array(&parent, "/arguments/game");
    let child_game_args = pointer_array(&child, "/arguments/game");
    let parent_jvm_args = pointer_array(&parent, "/arguments/jvm");
    let child_jvm_args = pointer_array(&child, "/arguments/jvm");

    let mut merged = parent;
    merge_objects(&mut merged, &child);

    if !parent_libraries.is_empty() || !child_libraries.is_empty() {
        let libraries = dedupe_libraries(
            child_libraries
                .into_iter()
                .chain(parent_libraries)
                .collect(),
        );
        merged["libraries"] = Value::Array(libraries);
    }

    if !parent_game_args.is_empty() || !child_game_args.is_empty() {
        set_argument_array(
            &mut merged,
            "game",
            parent_game_args
                .into_iter()
                .chain(child_game_args)
                .collect(),
        );
    }
    if !parent_jvm_args.is_empty() || !child_jvm_args.is_empty() {
        set_argument_array(
            &mut merged,
            "jvm",
            parent_jvm_args.into_iter().chain(child_jvm_args).collect(),
        );
    }

    merged
}

fn normalize_version_json_libraries(mut version_json: Value) -> Value {
    if version_json
        .get("libraries")
        .and_then(Value::as_array)
        .is_some()
    {
        version_json["libraries"] = Value::Array(version_libraries(&version_json));
    }
    version_json
}

fn resolve_installed_version_json_inner(
    version_id: &str,
    visited: &mut HashSet<String>,
) -> Result<Value, String> {
    if !visited.insert(version_id.to_string()) {
        return Err(format!("版本继承链出现循环: {version_id}"));
    }

    let child = read_installed_version_json(version_id)?;
    let Some(parent_id) = version_parent_id(&child) else {
        return Ok(child);
    };
    let parent = resolve_installed_version_json_inner(&parent_id, visited)?;
    Ok(merge_version_json(parent, child))
}

fn resolve_installed_version_json(version_id: &str) -> Result<Value, String> {
    resolve_installed_version_json_inner(version_id, &mut HashSet::new())
        .map(normalize_version_json_libraries)
}

fn detect_loader(version_id: &str, version_json: &Value) -> String {
    let lower_id = version_id.to_ascii_lowercase();
    let main_class = version_json
        .get("mainClass")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let libraries = version_libraries(&version_json);
    let joined_libraries = libraries
        .iter()
        .filter_map(|library| library.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let text = format!("{lower_id}\n{main_class}\n{joined_libraries}");

    if text.contains("neoforge") || text.contains("net.neoforged") {
        "NeoForge".to_string()
    } else if text.contains("fabric-loader") || text.contains("net.fabricmc") {
        "Fabric".to_string()
    } else if text.contains("quilt-loader") || text.contains("org.quiltmc") {
        "Quilt".to_string()
    } else if text.contains("forge") || text.contains("fmlloader") {
        "Forge".to_string()
    } else if text.contains("liteloader") {
        "LiteLoader".to_string()
    } else {
        "原版".to_string()
    }
}

fn directory_has_files(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_type()
                    .map(|kind| kind.is_file())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn directory_has_directories(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        })
        .unwrap_or(false)
}

fn should_isolate_version(
    version_id: &str,
    version_json: &Value,
    version_dir: &Path,
) -> (bool, String) {
    if directory_has_files(&version_dir.join("mods")) {
        return (
            true,
            "版本文件夹中存在 mods，按 PCL 规则自动开启版本隔离".to_string(),
        );
    }
    if directory_has_directories(&version_dir.join("saves")) {
        return (
            true,
            "版本文件夹中存在 saves，按 PCL 规则自动开启版本隔离".to_string(),
        );
    }
    let loader = detect_loader(version_id, version_json);
    if loader != "原版" {
        return (
            true,
            format!("{loader} 是可安装 Mod 的版本，按默认规则开启版本隔离"),
        );
    }
    (
        false,
        "原版且版本文件夹未发现独立 mods/saves，使用 .minecraft 根目录".to_string(),
    )
}

fn isolated_game_dir(
    version_id: &str,
    version_json: &Value,
) -> Result<(PathBuf, bool, String), String> {
    let root = minecraft_root()?;
    let version_dir = version_dir_path(version_id)?;
    let (isolated, reason) = should_isolate_version(version_id, version_json, &version_dir);
    let game_dir = if isolated { version_dir } else { root };
    Ok((game_dir, isolated, reason))
}

fn run_hidden_output(command: &mut Command) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().map_err(to_string)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn parse_key_value_i64(text: &str, key: &str) -> Option<i64> {
    text.lines().find_map(|line| {
        let (left, right) = line.split_once('=')?;
        if left.trim().eq_ignore_ascii_case(key) {
            right.trim().parse::<i64>().ok()
        } else {
            None
        }
    })
}

fn system_memory_mb() -> (i64, i64) {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("powershell");
        command.args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_OperatingSystem | ForEach-Object { \"TotalVisibleMemorySize=$($_.TotalVisibleMemorySize)\"; \"FreePhysicalMemory=$($_.FreePhysicalMemory)\" }",
        ]);
        if let Ok(text) = run_hidden_output(&mut command) {
            if let (Some(total_kb), Some(free_kb)) = (
                parse_key_value_i64(&text, "TotalVisibleMemorySize"),
                parse_key_value_i64(&text, "FreePhysicalMemory"),
            ) {
                return ((total_kb / 1024).max(0), (free_kb / 1024).max(0));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = fs::read_to_string("/proc/meminfo") {
            let parse_mem = |name: &str| -> Option<i64> {
                text.lines().find_map(|line| {
                    let mut parts = line.split_whitespace();
                    if parts.next()? == name {
                        parts.next()?.parse::<i64>().ok().map(|kb| kb / 1024)
                    } else {
                        None
                    }
                })
            };
            let total = parse_mem("MemTotal:").unwrap_or(0);
            let available = parse_mem("MemAvailable:").unwrap_or_else(|| parse_mem("MemFree:").unwrap_or(0));
            return (total, available);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let mut total_cmd = Command::new("sysctl");
        total_cmd.args(["-n", "hw.memsize"]);
        if let Ok(total_text) = run_hidden_output(&mut total_cmd) {
            if let Ok(total_bytes) = total_text.trim().parse::<i64>() {
                let total_mb = total_bytes / 1024 / 1024;
                return (total_mb, total_mb / 2);
            }
        }
    }

    (0, 0)
}

fn count_mod_files(game_dir: &Path) -> usize {
    fs::read_dir(game_dir.join("mods"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().map(|kind| kind.is_file()).unwrap_or(false))
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .and_then(|value| value.to_str())
                        .map(|extension| {
                            matches!(
                                extension.to_ascii_lowercase().as_str(),
                                "jar" | "zip" | "litemod"
                            )
                        })
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or_default()
}

fn version_has_optifine(version_id: &str, version_json: &Value) -> bool {
    let text = format!(
        "{}\n{}\n{}",
        version_id,
        version_json
            .get("mainClass")
            .and_then(Value::as_str)
            .unwrap_or(""),
        version_libraries(version_json)
            .iter()
            .filter_map(|library| library.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    )
    .to_ascii_lowercase();
    text.contains("optifine")
}

fn recommend_memory_for_version(
    version_id: &str,
    version_json: &Value,
) -> Result<MemoryRecommendation, String> {
    let (total_mb, available_mb) = system_memory_mb();
    let (_, isolated, _) = isolated_game_dir(version_id, version_json)?;
    let game_dir = isolated_game_dir(version_id, version_json)?.0;
    let loader = detect_loader(version_id, version_json);
    let modable = loader != "原版";
    let mod_count = count_mod_files(&game_dir);
    let available_gb = if available_mb > 0 {
        available_mb as f64 / 1024.0
    } else if total_mb > 0 {
        total_mb as f64 / 1024.0 * 0.6
    } else {
        4.0
    };

    let (minimum, target1, target2, target3, reason) = if modable {
        (
            0.5 + mod_count as f64 / 150.0,
            1.5 + mod_count as f64 / 90.0,
            2.7 + mod_count as f64 / 50.0,
            4.5 + mod_count as f64 / 25.0,
            format!("{loader} 版本，检测到 {mod_count} 个 Mod 文件"),
        )
    } else if version_has_optifine(version_id, version_json) {
        (
            0.5,
            1.5,
            3.0,
            5.0,
            "OptiFine 版本，按光影预留内存".to_string(),
        )
    } else {
        (
            0.5,
            1.5,
            2.5,
            4.0,
            if isolated {
                "已隔离的原版实例，按普通版本分配".to_string()
            } else {
                "原版实例，按普通版本分配".to_string()
            },
        )
    };

    let mut ram_give = 0.0;
    let mut remaining = available_gb;
    let stage = |delta: f64, ratio: f64, ram_give: &mut f64, remaining: &mut f64| {
        if *remaining < 0.1 {
            return;
        }
        *ram_give += (*remaining * ratio).min(delta);
        *remaining -= delta / ratio;
    };
    stage(target1, 1.0, &mut ram_give, &mut remaining);
    stage(target2 - target1, 0.7, &mut ram_give, &mut remaining);
    stage(target3 - target2, 0.4, &mut ram_give, &mut remaining);
    stage(target3, 0.15, &mut ram_give, &mut remaining);

    let ram_gb = (ram_give.max(minimum) * 10.0).round() / 10.0;
    let mut recommended_mb = ((ram_gb * 1024.0 / 128.0).round() * 128.0) as i64;
    if total_mb > 0 {
        recommended_mb = recommended_mb.min((total_mb - 512).max(1024));
    }
    recommended_mb = recommended_mb.clamp(512, 32768);

    Ok(MemoryRecommendation {
        total_mb,
        available_mb,
        recommended_mb,
        mod_count,
        modable,
        reason,
    })
}

fn resolve_memory_mb(
    version_id: &str,
    version_json: &Value,
    memory_mode: Option<&str>,
    manual_mb: i64,
) -> Result<(i64, Option<MemoryRecommendation>), String> {
    if memory_mode
        .unwrap_or("manual")
        .eq_ignore_ascii_case("auto")
    {
        let recommendation = recommend_memory_for_version(version_id, version_json)?;
        Ok((recommendation.recommended_mb, Some(recommendation)))
    } else {
        Ok((manual_mb.max(512), None))
    }
}

#[derive(Clone)]
struct DownloadFile {
    urls: Vec<String>,
    path: PathBuf,
    sha1: Option<String>,
    size: Option<i64>,
    label: String,
    is_json: bool,
}

#[tauri::command]
async fn get_data_paths() -> Result<DataPaths, String> {
    data_paths()
}

#[tauri::command]
async fn set_minecraft_root(path: String) -> Result<DataPaths, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("请选择有效的 .minecraft 文件夹".to_string());
    }
    let root = PathBuf::from(trimmed);
    ensure_minecraft_dirs(&root)?;
    let mut config = read_launcher_config();
    config.minecraft_root = Some(root.display().to_string());
    save_launcher_config(&config)?;
    data_paths()
}

#[tauri::command]
async fn choose_minecraft_root() -> Result<Option<DataPaths>, String> {
    let current = minecraft_root().unwrap_or_else(|_| {
        official_minecraft_root().unwrap_or_else(|| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from(std::path::MAIN_SEPARATOR.to_string()))
        })
    });
    let picked = rfd::FileDialog::new()
        .set_title("选择 .minecraft 文件夹")
        .set_directory(current)
        .pick_folder();
    let Some(root) = picked else {
        return Ok(None);
    };
    ensure_minecraft_dirs(&root)?;
    let mut config = read_launcher_config();
    config.minecraft_root = Some(root.display().to_string());
    save_launcher_config(&config)?;
    Ok(Some(data_paths()?))
}

#[tauri::command]
async fn scan_java_installations() -> Result<Vec<JavaInstallation>, String> {
    tokio::task::spawn_blocking(scan_java_installations_sync)
        .await
        .map_err(to_string)?
}

#[tauri::command]
async fn recommend_memory(version_id: String) -> Result<MemoryRecommendation, String> {
    let version_json = resolve_installed_version_json(&version_id)?;
    recommend_memory_for_version(&version_id, &version_json)
}

#[tauri::command]
async fn choose_java_executable() -> Result<Option<JavaInstallation>, String> {
    let mut dialog = rfd::FileDialog::new().set_title("选择 Java 可执行文件");
    #[cfg(target_os = "windows")]
    {
        dialog = dialog.add_filter("Java", &["exe"]);
    }
    let Some(path) = dialog.pick_file() else {
        return Ok(None);
    };
    inspect_java_installation(&path, "手动导入")
        .map(Some)
        .ok_or_else(|| format!("无法识别这个 Java：{}", path.display()))
}

#[tauri::command]
async fn get_version_manifest() -> Result<VersionManifest, String> {
    load_manifest().await
}

#[tauri::command]
async fn list_installed_versions() -> Result<Vec<InstalledVersion>, String> {
    let root = versions_root()?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut versions = Vec::new();
    for entry in fs::read_dir(root).map_err(to_string)? {
        let entry = entry.map_err(to_string)?;
        if !entry.file_type().map_err(to_string)?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let dir = entry.path();
        let Some(json_path) = find_version_json_path(&id)? else {
            versions.push(InstalledVersion {
                display_name: id.clone(),
                kind: "unknown".to_string(),
                loader: "未知".to_string(),
                has_client: dir.join(format!("{id}.jar")).exists(),
                has_manifest: false,
                path: dir.display().to_string(),
                inherits_from: None,
                jar: None,
                id,
            });
            continue;
        };
        let text = fs::read_to_string(&json_path).map_err(to_string)?;
        let version_json = serde_json::from_str::<Value>(&text).map_err(to_string)?;
        let inherits_from = version_parent_id(&version_json);
        let jar = version_json
            .get("jar")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let loader = detect_loader(&id, &version_json);
        let kind = version_json
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(if loader == "原版" {
                "release"
            } else {
                "modded"
            })
            .to_string();
        let has_client = primary_jar_path(&id, &version_json)
            .map(|path| path.exists())
            .unwrap_or(false);
        versions.push(InstalledVersion {
            display_name: if loader == "原版" {
                id.clone()
            } else {
                format!("{id} · {loader}")
            },
            kind,
            loader,
            has_client,
            has_manifest: true,
            path: dir.display().to_string(),
            inherits_from,
            jar,
            id,
        });
    }
    versions.sort_by(|a, b| {
        let a_key = a.id.to_ascii_lowercase();
        let b_key = b.id.to_ascii_lowercase();
        b_key.cmp(&a_key)
    });
    Ok(versions)
}

#[tauri::command]
async fn delete_installed_version(version_id: String) -> Result<(), String> {
    let trimmed = version_id.trim();
    if trimmed.is_empty() {
        return Err("请选择要删除的版本".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("版本 ID 不能包含路径分隔符".to_string());
    }

    let root = versions_root()?;
    let target = root.join(trimmed);
    if !target.exists() {
        return Err(format!("版本不存在：{trimmed}"));
    }
    let root = root.canonicalize().map_err(to_string)?;
    let target = target.canonicalize().map_err(to_string)?;
    if target == root || !target.starts_with(&root) {
        return Err("拒绝删除 versions 目录之外的路径".to_string());
    }
    if !target.is_dir() {
        return Err("目标不是版本文件夹".to_string());
    }
    fs::remove_dir_all(target).map_err(to_string)
}

#[tauri::command]
async fn get_version_summary(version_id: String) -> Result<VersionSummary, String> {
    let client = http_client()?;
    let version_json = if find_version_json_path(&version_id)?.is_some() {
        resolve_installed_version_json(&version_id)?
    } else {
        load_version_json(&client, &version_id).await?
    };
    let asset_url = version_json
        .get("assetIndex")
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str);
    let assets = match asset_url {
        Some(url) => {
            let asset_index = fetch_json_from_urls::<Value>(&client, mirror_urls(url)).await?;
            asset_index
                .get("objects")
                .and_then(Value::as_object)
                .map(|objects| objects.len())
        }
        None => None,
    };

    let game_arguments = version_json
        .pointer("/arguments/game")
        .and_then(Value::as_array)
        .map(Vec::len)
        .or_else(|| {
            version_json
                .get("minecraftArguments")
                .and_then(Value::as_str)
                .map(|text| text.split_whitespace().count())
        })
        .unwrap_or_default();

    Ok(VersionSummary {
        id: version_json
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(&version_id)
            .to_string(),
        main_class: version_json
            .get("mainClass")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        asset_index: version_json
            .get("assets")
            .or_else(|| version_json.pointer("/assetIndex/id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        java_component: version_json
            .pointer("/javaVersion/component")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        java_major: version_json
            .pointer("/javaVersion/majorVersion")
            .and_then(Value::as_i64),
        libraries: version_libraries(&version_json).len(),
        assets,
        client_size: version_json
            .pointer("/downloads/client/size")
            .and_then(Value::as_i64),
        game_arguments,
        jvm_arguments: version_json
            .pointer("/arguments/jvm")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
    })
}

fn loader_download_label(loader: &str) -> Option<&'static str> {
    match loader.trim().to_ascii_lowercase().as_str() {
        "fabric" => Some("Fabric"),
        "forge" => Some("Forge"),
        "none" | "" => None,
        _ => None,
    }
}

async fn get_fabric_loader_versions(
    client: &Client,
    game_version: &str,
) -> Result<Vec<LoaderVersionOption>, String> {
    let versions = fetch_json_from_urls::<Value>(
        client,
        vec![format!("{FABRIC_META_API}/versions/loader/{game_version}")],
    )
    .await?;
    let mut output = versions
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let version = item.pointer("/loader/version")?.as_str()?.to_string();
            let stable = item
                .pointer("/loader/stable")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            Some(LoaderVersionOption {
                loader: "fabric".to_string(),
                display_name: if stable {
                    format!("Fabric {version}")
                } else {
                    format!("Fabric {version} beta")
                },
                recommended: false,
                stable,
                version,
            })
        })
        .collect::<Vec<_>>();
    if let Some(first) = output.first_mut() {
        first.recommended = true;
        first.display_name = format!("{} · 最新", first.display_name);
    }
    Ok(output)
}

fn parse_xml_versions(text: &str) -> Vec<String> {
    let mut versions = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<version>") {
        rest = &rest[start + "<version>".len()..];
        let Some(end) = rest.find("</version>") else {
            break;
        };
        let version = rest[..end].trim();
        if !version.is_empty() {
            versions.push(version.to_string());
        }
        rest = &rest[end + "</version>".len()..];
    }
    versions
}

async fn forge_promotions(client: &Client) -> Result<Value, String> {
    fetch_json_from_urls::<Value>(
        client,
        vec![
            FORGE_PROMOTIONS_URL.to_string(),
            "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/promotions_slim.json"
                .to_string(),
        ],
    )
    .await
}

async fn get_forge_loader_versions(
    client: &Client,
    game_version: &str,
) -> Result<Vec<LoaderVersionOption>, String> {
    let metadata = fetch_text_from_urls(
        client,
        vec![
            "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml"
                .to_string(),
            "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/maven-metadata.xml"
                .to_string(),
        ],
    )
    .await?;
    let prefix = format!("{game_version}-");
    let mut versions = parse_xml_versions(&metadata)
        .into_iter()
        .filter_map(|artifact| artifact.strip_prefix(&prefix).map(ToOwned::to_owned))
        .filter(|version| !version.trim().is_empty())
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| compare_versions(right, left));
    versions.dedup();

    let promotions = forge_promotions(client).await.ok();
    let recommended = promotions
        .as_ref()
        .and_then(|value| value.pointer(&format!("/promos/{game_version}-recommended")))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let latest = promotions
        .as_ref()
        .and_then(|value| value.pointer(&format!("/promos/{game_version}-latest")))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let mut output = versions
        .into_iter()
        .map(|version| {
            let is_recommended = recommended.as_deref() == Some(version.as_str());
            let is_latest = latest.as_deref() == Some(version.as_str());
            let suffix = if is_recommended {
                " · 推荐"
            } else if is_latest {
                " · 最新"
            } else {
                ""
            };
            LoaderVersionOption {
                loader: "forge".to_string(),
                display_name: format!("Forge {version}{suffix}"),
                recommended: is_recommended || is_latest,
                stable: true,
                version,
            }
        })
        .collect::<Vec<_>>();
    output.sort_by(|left, right| {
        right
            .recommended
            .cmp(&left.recommended)
            .then_with(|| compare_versions(&right.version, &left.version))
    });
    Ok(output)
}

#[tauri::command]
async fn get_loader_versions(
    game_version: String,
    loader: String,
) -> Result<Vec<LoaderVersionOption>, String> {
    let game_version = modrinth_game_version(&game_version);
    if game_version.trim().is_empty() {
        return Ok(Vec::new());
    }
    let client = http_client()?;
    match loader.trim().to_ascii_lowercase().as_str() {
        "fabric" => get_fabric_loader_versions(&client, &game_version).await,
        "forge" => get_forge_loader_versions(&client, &game_version).await,
        _ => Ok(Vec::new()),
    }
}

async fn download_profile_libraries(
    app: &AppHandle,
    client: &Client,
    version_json: &Value,
    phase: &str,
) -> Result<(), String> {
    let root = minecraft_root()?;
    let libraries = version_libraries(version_json);
    let mut downloads = Vec::<DownloadFile>::new();
    let mut paths = HashSet::new();
    for library in libraries.iter().filter(|library| rules_allow(library.get("rules"))) {
        if let Some(file) = library_download_file(&root, library) {
            if paths.insert(file.path.clone()) {
                downloads.push(file);
            }
        }
    }

    let total = downloads.len();
    let done = Arc::new(AtomicUsize::new(0));
    let results = stream::iter(downloads.into_iter())
        .map(|file| {
            let client = client.clone();
            let app = app.clone();
            let done = done.clone();
            async move {
                let result = download_to_path(
                    &client,
                    file.urls,
                    &file.path,
                    file.sha1.as_deref(),
                    file.size,
                    file.is_json,
                )
                .await;
                let current = done.fetch_add(1, Ordering::SeqCst) + 1;
                emit_progress(&app, phase, current, total.max(1), file.label.clone());
                result.map_err(|error| format!("{}: {error}", file.label))
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;
    let failed = results.into_iter().filter_map(Result::err).collect::<Vec<_>>();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} 个加载器支持库下载失败：{}",
            failed.len(),
            failed.into_iter().take(4).collect::<Vec<_>>().join("; ")
        ))
    }
}

async fn install_fabric_loader(
    app: &AppHandle,
    client: &Client,
    game_version: &str,
    selected_loader_version: Option<&str>,
) -> Result<String, String> {
    emit_progress(app, "loader", 0, 3, "获取 Fabric Loader 列表");
    let loader_versions = get_fabric_loader_versions(client, game_version).await?;
    let loader_version = selected_loader_version
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| loader_versions.first().map(|item| item.version.clone()))
        .ok_or_else(|| format!("Fabric 暂未提供 Minecraft {game_version} 的加载器"))?;

    emit_progress(
        app,
        "loader",
        1,
        3,
        format!("获取 Fabric {loader_version} 启动配置"),
    );
    let profile_url =
        format!("{FABRIC_META_API}/versions/loader/{game_version}/{loader_version}/profile/json");
    let profile = fetch_json_from_urls::<Value>(client, vec![profile_url]).await?;
    let profile_id = profile
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let profile_id = if profile_id.is_empty() {
        format!("fabric-loader-{loader_version}-{game_version}")
    } else {
        profile_id.to_string()
    };

    let profile_dir = versions_root()?.join(&profile_id);
    fs::create_dir_all(&profile_dir).map_err(to_string)?;
    let profile_path = profile_dir.join(format!("{profile_id}.json"));
    fs::write(
        &profile_path,
        serde_json::to_string_pretty(&profile).map_err(to_string)?,
    )
    .map_err(to_string)?;

    emit_progress(app, "loader", 2, 3, "下载 Fabric 支持库");
    download_profile_libraries(app, client, &profile, "loader").await?;
    emit_progress(
        app,
        "loader",
        3,
        3,
        format!("Fabric {loader_version} 安装完成"),
    );
    Ok(profile_id)
}

fn java_for_installer(version_json: &Value) -> String {
    let required_major = required_java_major_or_default(version_json);
    scan_java_installations_sync()
        .ok()
        .and_then(|installations| select_best_java(&installations, required_major))
        .map(|java| java.path)
        .unwrap_or_else(|| "java".to_string())
}

async fn install_forge_loader(
    app: &AppHandle,
    client: &Client,
    game_version: &str,
    vanilla_json: &Value,
    selected_forge_version: Option<&str>,
) -> Result<String, String> {
    emit_progress(app, "loader", 0, 4, "获取 Forge 版本信息");
    let selected_forge_version = selected_forge_version
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let forge_version = if let Some(version) = selected_forge_version {
        version
    } else {
        let promoted = if let Ok(promotions) = forge_promotions(client).await {
            promotions
                .get("promos")
                .and_then(Value::as_object)
                .and_then(|promos| {
                    promos
                        .get(&format!("{game_version}-recommended"))
                        .or_else(|| promos.get(&format!("{game_version}-latest")))
                        .and_then(Value::as_str)
                })
                .map(ToOwned::to_owned)
        } else {
            None
        };
        if let Some(version) = promoted {
            version
        } else {
            get_forge_loader_versions(client, game_version)
                .await?
                .first()
                .map(|item| item.version.clone())
                .ok_or_else(|| format!("Forge 暂未提供 Minecraft {game_version} 的安装器"))?
        }
    };

    let artifact = format!("{game_version}-{forge_version}");
    let installer_name = format!("forge-{artifact}-installer.jar");
    let installer_url =
        format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{artifact}/{installer_name}");
    let installer_path = launcher_root()?
        .join("cache")
        .join("forge")
        .join(&artifact)
        .join(&installer_name);

    emit_progress(app, "loader", 1, 4, format!("下载 Forge {forge_version} 安装器"));
    download_to_path(
        client,
        mirror_urls(&installer_url),
        &installer_path,
        None,
        None,
        false,
    )
    .await?;

    emit_progress(app, "loader", 2, 4, "运行 Forge 客户端安装器");
    let java = java_for_installer(vanilla_json);
    let root = minecraft_root()?;
    let mut command = Command::new(&java);
    command
        .arg("-jar")
        .arg(&installer_path)
        .arg("--installClient")
        .arg(&root)
        .current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().map_err(to_string)?;
    let output_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(format!("Forge 安装器执行失败：{}", output_text.trim()));
    }

    emit_progress(app, "loader", 3, 4, "校验 Forge 版本目录");
    let installed_versions = list_installed_versions().await?;
    let installed_id = installed_versions
        .iter()
        .find(|version| {
            version.loader.eq_ignore_ascii_case("forge")
                && version
                    .inherits_from
                    .as_deref()
                    .map(|parent| parent == game_version)
                    .unwrap_or_else(|| version.id.contains(game_version))
                && version.id.contains(&forge_version)
        })
        .map(|version| version.id.clone())
        .unwrap_or_else(|| format!("{game_version}-forge-{forge_version}"));
    emit_progress(
        app,
        "loader",
        4,
        4,
        format!("Forge {forge_version} 安装完成"),
    );
    Ok(installed_id)
}

#[tauri::command]
async fn download_version(
    app: AppHandle,
    version_id: String,
    include_assets: bool,
    loader: Option<String>,
    loader_version: Option<String>,
) -> Result<(), String> {
    let client = http_client()?;
    let root = minecraft_root()?;
    fs::create_dir_all(&root).map_err(to_string)?;

    emit_progress(&app, "metadata", 0, 1, "获取版本清单");
    let manifest = fetch_json_from_urls::<VersionManifest>(&client, manifest_urls()).await?;
    let version = manifest
        .versions
        .iter()
        .find(|item| item.id == version_id)
        .ok_or_else(|| format!("找不到版本 {version_id}"))?;

    emit_progress(&app, "metadata", 1, 3, "下载版本描述");
    let version_dir = versions_root()?.join(&version_id);
    fs::create_dir_all(&version_dir).map_err(to_string)?;
    let version_json_path = version_dir.join(format!("{version_id}.json"));
    download_to_path(
        &client,
        mirror_urls(&version.url),
        &version_json_path,
        Some(&version.sha1),
        None,
        true,
    )
    .await?;
    let version_json = resolve_installed_version_json(&version_id)?;

    if let Some(client_download) = version_json.pointer("/downloads/client") {
        if let Some(url) = client_download.get("url").and_then(Value::as_str) {
            emit_progress(&app, "client", 0, 1, "下载客户端 Jar");
            let sha1 = client_download.get("sha1").and_then(Value::as_str);
            let size = client_download.get("size").and_then(Value::as_i64);
            download_to_path(
                &client,
                mirror_urls(url),
                &version_dir.join(format!("{version_id}.jar")),
                sha1,
                size,
                false,
            )
            .await?;
        }
    }

    let libraries = version_libraries(&version_json);
    let mut library_downloads: Vec<DownloadFile> = Vec::new();
    let mut native_jars = Vec::new();
    let mut library_paths = HashSet::new();
    for library in libraries
        .iter()
        .filter(|library| rules_allow(library.get("rules")))
    {
        if let Some(classifier) = standalone_native_classifier(library) {
            if !native_classifier_matches_current(classifier) {
                continue;
            }
            if let Some(artifact) = library.pointer("/downloads/artifact") {
                if let (Some(url), Some(path)) = (
                    artifact.get("url").and_then(Value::as_str),
                    artifact_path(&root, artifact),
                ) {
                    native_jars.push(path.clone());
                    if library_paths.insert(path.clone()) {
                        library_downloads.push(DownloadFile {
                            urls: mirror_urls(url),
                            path,
                            sha1: artifact
                                .get("sha1")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned),
                            size: artifact.get("size").and_then(Value::as_i64),
                            label: library
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("native")
                                .to_string(),
                            is_json: false,
                        });
                    }
                }
            }
            continue;
        }
        if let Some(file) = library_download_file(&root, library) {
            if library_paths.insert(file.path.clone()) {
                library_downloads.push(file);
            }
        }

        let native_key = library
            .get("natives")
            .and_then(|value| value.get(current_os_name()))
            .and_then(Value::as_str)
            .map(|key| key.replace("${arch}", "64"))
            .unwrap_or_else(|| current_native_key().to_string());

        if let Some(classifier) = library
            .pointer("/downloads/classifiers")
            .and_then(|value| value.get(&native_key))
        {
            if let (Some(url), Some(path)) = (
                classifier.get("url").and_then(Value::as_str),
                artifact_path(&root, classifier),
            ) {
                native_jars.push(path.clone());
                if library_paths.insert(path.clone()) {
                    library_downloads.push(DownloadFile {
                        urls: mirror_urls(url),
                        path,
                        sha1: classifier
                            .get("sha1")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        size: classifier.get("size").and_then(Value::as_i64),
                        label: library
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("native")
                            .to_string(),
                        is_json: false,
                    });
                }
            }
        }
    }

    let total_libraries = library_downloads.len();
    let library_done = Arc::new(AtomicUsize::new(0));
    let library_results = stream::iter(library_downloads.into_iter())
        .map(|file| {
            let client = client.clone();
            let app = app.clone();
            let done = library_done.clone();
            async move {
                let result = download_to_path(
                    &client,
                    file.urls,
                    &file.path,
                    file.sha1.as_deref(),
                    file.size,
                    file.is_json,
                )
                .await;
                let current = done.fetch_add(1, Ordering::SeqCst) + 1;
                emit_progress(
                    &app,
                    "libraries",
                    current,
                    total_libraries.max(1),
                    file.label.clone(),
                );
                result.map_err(|error| format!("{}: {error}", file.label))
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;
    let failed_libraries = library_results
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();
    if !failed_libraries.is_empty() {
        return Err(format!(
            "{} 个支持库下载失败：{}",
            failed_libraries.len(),
            failed_libraries
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    let natives_dir = version_dir.join("natives");
    if natives_dir.exists() {
        fs::remove_dir_all(&natives_dir).map_err(to_string)?;
    }
    fs::create_dir_all(&natives_dir).map_err(to_string)?;
    for jar in native_jars {
        extract_native_jar(&jar, &natives_dir, &[])?;
    }

    if include_assets {
        if let Some(asset_index_value) = version_json.get("assetIndex") {
            if let Some(asset_index_url) = asset_index_value.get("url").and_then(Value::as_str) {
                let asset_index_id = asset_index_value
                    .get("id")
                    .and_then(Value::as_str)
                    .or_else(|| version_json.get("assets").and_then(Value::as_str))
                    .unwrap_or(&version_id);
                emit_progress(&app, "assets", 0, 1, "下载资源索引");
                let indexes_dir = root.join("assets").join("indexes");
                fs::create_dir_all(&indexes_dir).map_err(to_string)?;
                let asset_index_path = indexes_dir.join(format!("{asset_index_id}.json"));
                download_to_path(
                    &client,
                    mirror_urls(asset_index_url),
                    &asset_index_path,
                    asset_index_value.get("sha1").and_then(Value::as_str),
                    asset_index_value.get("size").and_then(Value::as_i64),
                    true,
                )
                .await?;
                let asset_index = serde_json::from_str::<Value>(
                    &fs::read_to_string(&asset_index_path).map_err(to_string)?,
                )
                .map_err(to_string)?;

                let objects = asset_index
                    .get("objects")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let mut asset_downloads = Vec::new();
                let mut asset_paths = HashSet::new();
                for (name, object) in objects.into_iter() {
                    let hash = object
                        .get("hash")
                        .and_then(Value::as_str)
                        .ok_or_else(|| format!("资源缺少 hash: {name}"))?;
                    if hash.len() < 2 {
                        return Err(format!("资源 hash 无效: {name}"));
                    }
                    let prefix = &hash[0..2];
                    let url = format!("https://resources.download.minecraft.net/{prefix}/{hash}");
                    let path = root.join("assets").join("objects").join(prefix).join(hash);
                    if asset_paths.insert(path.clone()) {
                        asset_downloads.push(DownloadFile {
                            urls: mirror_urls(&url),
                            path,
                            sha1: Some(hash.to_string()),
                            size: object.get("size").and_then(Value::as_i64),
                            label: name,
                            is_json: false,
                        });
                    }
                }
                let total_assets = asset_downloads.len();
                let done = Arc::new(AtomicUsize::new(0));
                let app_for_assets = app.clone();
                let client_for_assets = client.clone();

                let asset_results = stream::iter(asset_downloads.into_iter())
                    .map(move |file| {
                        let client = client_for_assets.clone();
                        let app = app_for_assets.clone();
                        let done = done.clone();
                        async move {
                            let result = download_to_path(
                                &client,
                                file.urls,
                                &file.path,
                                file.sha1.as_deref(),
                                file.size,
                                file.is_json,
                            )
                            .await;
                            let current = done.fetch_add(1, Ordering::SeqCst) + 1;
                            let label = if result.is_ok() {
                                file.label.clone()
                            } else {
                                format!("资源下载失败: {}", file.label)
                            };
                            emit_progress(&app, "assets", current, total_assets.max(1), label);
                            result.map_err(|error| format!("{}: {error}", file.label))
                        }
                    })
                    .buffer_unordered(24)
                    .collect::<Vec<_>>()
                    .await;

                let failed_assets = asset_results
                    .into_iter()
                    .filter_map(Result::err)
                    .collect::<Vec<_>>();
                if !failed_assets.is_empty() {
                    return Err(format!(
                        "{} 个资源下载失败：{}",
                        failed_assets.len(),
                        failed_assets
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
            }
        }
    }

    if let Some(loader_name) = loader
        .as_deref()
        .and_then(loader_download_label)
        .map(ToOwned::to_owned)
    {
        match loader_name.as_str() {
            "Fabric" => {
                let profile_id = install_fabric_loader(
                    &app,
                    &client,
                    &version_id,
                    loader_version.as_deref(),
                )
                .await?;
                emit_progress(
                    &app,
                    "done",
                    1,
                    1,
                    format!("版本下载完成，已安装 {profile_id}"),
                );
            }
            "Forge" => {
                let profile_id = install_forge_loader(
                    &app,
                    &client,
                    &version_id,
                    &version_json,
                    loader_version.as_deref(),
                )
                .await?;
                emit_progress(
                    &app,
                    "done",
                    1,
                    1,
                    format!("版本下载完成，已安装 {profile_id}"),
                );
            }
            _ => {}
        }
    } else {
        emit_progress(&app, "done", 1, 1, "版本下载完成");
    }
    Ok(())
}

#[tauri::command]
async fn begin_microsoft_device_login() -> Result<DeviceCodeResponse, String> {
    let client = http_client()?;
    let client_id = microsoft_client_id();
    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .form(&[
            ("client_id", client_id.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(to_string)?;

    if !response.status().is_success() {
        return Err(response.text().await.map_err(to_string)?);
    }

    let mut device_code_response = response
        .json::<DeviceCodeResponse>()
        .await
        .map_err(to_string)?;

    match open_external_url(&device_code_response.verification_uri) {
        Ok(()) => {
            device_code_response.browser_opened = true;
            device_code_response.browser_open_error = None;
        }
        Err(error) => {
            device_code_response.browser_opened = false;
            device_code_response.browser_open_error = Some(error);
        }
    }

    Ok(device_code_response)
}

#[tauri::command]
async fn poll_microsoft_device_login(device_code: String) -> Result<LoginPollResult, String> {
    let client = http_client()?;
    let client_id = microsoft_client_id();
    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id.as_str()),
            ("device_code", device_code.trim()),
        ])
        .send()
        .await
        .map_err(to_string)?;

    let status = response.status();
    let value = response.json::<Value>().await.map_err(to_string)?;
    if !status.is_success() {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if error == "authorization_pending" || error == "slow_down" {
            return Ok(LoginPollResult {
                status: "pending".to_string(),
                message: "等待 Microsoft 授权确认".to_string(),
                profile: None,
            });
        }
        return Err(value.to_string());
    }

    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("Microsoft 响应缺少 access_token")?;
    let refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let profile = complete_minecraft_auth(&client, access_token, refresh_token).await?;
    save_minecraft_profile_to_disk(&profile)?;

    Ok(LoginPollResult {
        status: "done".to_string(),
        message: "正版验证完成，账号已保存".to_string(),
        profile: Some(profile),
    })
}

#[tauri::command]
fn get_saved_minecraft_profile() -> Result<Option<MinecraftProfile>, String> {
    read_minecraft_profile_from_disk()
}

#[tauri::command]
fn logout_minecraft_profile() -> Result<(), String> {
    remove_minecraft_profile_from_disk()
}

#[tauri::command]
fn create_offline_profile(request: OfflineLoginRequest) -> Result<MinecraftProfile, String> {
    let name = request.name.trim();
    if !(3..=16).contains(&name.len()) {
        return Err("离线用户名需要 3-16 个字符".to_string());
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err("离线用户名只能包含英文、数字和下划线".to_string());
    }

    let mut digest = md5::compute(format!("OfflinePlayer:{name}").as_bytes()).0;
    digest[6] = (digest[6] & 0x0f) | 0x30;
    digest[8] = (digest[8] & 0x3f) | 0x80;
    let uuid = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let profile = MinecraftProfile {
        id: uuid.clone(),
        name: name.to_string(),
        access_token: format!("offline-{uuid}"),
        refresh_token: None,
        xuid: None,
        owns_game: false,
        expires_in: 0,
        expires_at: None,
        account_type: "offline".to_string(),
    };
    save_minecraft_profile_to_disk(&profile)?;
    Ok(profile)
}

#[tauri::command]
async fn refresh_minecraft_profile() -> Result<MinecraftProfile, String> {
    let saved_profile = read_minecraft_profile_from_disk()?.ok_or("本地没有已保存的正版账号")?;
    if saved_profile.account_type == "offline" {
        return Err("离线账号不需要刷新登录".to_string());
    }
    let saved_refresh_token = saved_profile
        .refresh_token
        .ok_or("本地账号缺少 refresh token，请重新登录")?;

    let client = http_client()?;
    let client_id = microsoft_client_id();
    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", saved_refresh_token.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(to_string)?;

    let status = response.status();
    let value = response.json::<Value>().await.map_err(to_string)?;
    if !status.is_success() {
        return Err(format!("刷新 Microsoft 登录失败: {value}"));
    }

    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("Microsoft 刷新响应缺少 access_token")?;
    let refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or(Some(saved_refresh_token));
    let profile = complete_minecraft_auth(&client, access_token, refresh_token).await?;
    save_minecraft_profile_to_disk(&profile)?;
    Ok(profile)
}

async fn complete_minecraft_auth(
    client: &Client,
    microsoft_access_token: &str,
    refresh_token: Option<String>,
) -> Result<MinecraftProfile, String> {
    let xbl = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={microsoft_access_token}")
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .map_err(to_string)?;
    if !xbl.status().is_success() {
        return Err(format!(
            "Xbox Live 鉴权失败: {}",
            xbl.text().await.map_err(to_string)?
        ));
    }
    let xbl_value = xbl.json::<Value>().await.map_err(to_string)?;
    let xbl_token = xbl_value
        .get("Token")
        .and_then(Value::as_str)
        .ok_or("Xbox Live 响应缺少 Token")?;

    let xsts = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl_token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .map_err(to_string)?;
    if !xsts.status().is_success() {
        return Err(format!(
            "XSTS 鉴权失败: {}",
            xsts.text().await.map_err(to_string)?
        ));
    }
    let xsts_value = xsts.json::<Value>().await.map_err(to_string)?;
    let xsts_token = xsts_value
        .get("Token")
        .and_then(Value::as_str)
        .ok_or("XSTS 响应缺少 Token")?;
    let uhs = xsts_value
        .pointer("/DisplayClaims/xui/0/uhs")
        .and_then(Value::as_str)
        .ok_or("XSTS 响应缺少 UHS")?;
    let xuid = xsts_value
        .pointer("/DisplayClaims/xui/0/xid")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let mc_auth = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
        }))
        .send()
        .await
        .map_err(to_string)?;
    if !mc_auth.status().is_success() {
        return Err(format!(
            "Minecraft Services 登录失败: {}",
            mc_auth.text().await.map_err(to_string)?
        ));
    }
    let mc_auth_value = mc_auth.json::<Value>().await.map_err(to_string)?;
    let mc_access_token = mc_auth_value
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("Minecraft Services 响应缺少 access_token")?
        .to_string();
    let expires_in = mc_auth_value
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let entitlements = client
        .get("https://api.minecraftservices.com/entitlements/mcstore")
        .bearer_auth(&mc_access_token)
        .send()
        .await
        .map_err(to_string)?;
    if !entitlements.status().is_success() {
        return Err(format!(
            "Minecraft 授权清单读取失败: {}",
            entitlements.text().await.map_err(to_string)?
        ));
    }
    let entitlements_value = entitlements.json::<Value>().await.map_err(to_string)?;
    let owns_game = entitlements_value
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items.iter().any(|item| {
                item.get("name")
                    .and_then(Value::as_str)
                    .map(|name| name.contains("minecraft") || name.contains("game_minecraft"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    let profile_response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&mc_access_token)
        .send()
        .await
        .map_err(to_string)?;
    if !profile_response.status().is_success() {
        return Err(format!(
            "Minecraft 档案读取失败: {}",
            profile_response.text().await.map_err(to_string)?
        ));
    }
    let profile = profile_response.json::<Value>().await.map_err(to_string)?;

    Ok(MinecraftProfile {
        id: profile
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        name: profile
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        access_token: mc_access_token,
        refresh_token,
        xuid,
        owns_game,
        expires_in,
        expires_at: Some(unix_timestamp().saturating_add(expires_in.max(0) as u64)),
        account_type: "microsoft".to_string(),
    })
}

fn build_launch_arguments(
    version_id: &str,
    version_json: &Value,
    java_path: &str,
    max_memory_mb: i64,
    account: &LaunchAccount,
) -> Result<(String, Vec<String>, PathBuf), String> {
    let root = minecraft_root()?;
    ensure_minecraft_dirs(&root)?;
    let version_dir = version_dir_path(version_id)?;
    let (game_dir, _, _) = isolated_game_dir(version_id, version_json)?;
    let natives_dir = version_dir.join("natives").join(format!(
        "{}-{}-{}",
        current_os_name(),
        std::process::id(),
        unix_timestamp()
    ));
    fs::create_dir_all(&game_dir).map_err(to_string)?;
    fs::create_dir_all(game_dir.join("mods")).map_err(to_string)?;

    let mut classpath = Vec::new();
    let mut native_jars: Vec<(PathBuf, Vec<String>)> = Vec::new();
    let mut missing = Vec::new();
    for library in version_libraries(version_json)
        .iter()
        .filter(|library| rules_allow(library.get("rules")))
    {
        if let Some(classifier) = standalone_native_classifier(library) {
            if !native_classifier_matches_current(classifier) {
                continue;
            }
            let library_name = library
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("library")
                .to_string();
            if let Some(path) = library_native_path(&root, library) {
                if path.exists() {
                    native_jars.push((path, native_extract_excludes(library)));
                } else {
                    missing.push(format!("{library_name} native ({})", path.display()));
                }
            }
            continue;
        }

        let library_name = library
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("library")
            .to_string();

        if let Some(path) = library_artifact_path(&root, library) {
            if path.exists() {
                classpath.push(path);
            } else {
                missing.push(format!("{library_name} ({})", path.display()));
            }
        }

        if let Some(path) = library_native_path(&root, library) {
            if path.exists() {
                native_jars.push((path, native_extract_excludes(library)));
            } else {
                missing.push(format!("{library_name} native ({})", path.display()));
            }
        }
    }
    let client_jar = primary_jar_path(version_id, version_json)?;
    if !client_jar.exists() {
        return Err(format!(
            "客户端 Jar 不存在：{}。请先下载原版父版本或补全该 .minecraft 的版本文件",
            client_jar.display()
        ));
    }
    if !missing.is_empty() {
        return Err(format!(
            "缺少 {} 个启动依赖，请先补全 .minecraft/libraries：{}",
            missing.len(),
            missing.into_iter().take(6).collect::<Vec<_>>().join("；")
        ));
    }
    if natives_dir.exists() {
        let _ = fs::remove_dir_all(&natives_dir);
    }
    fs::create_dir_all(&natives_dir).map_err(to_string)?;
    for (jar, excludes) in native_jars {
        extract_native_jar(&jar, &natives_dir, &excludes)?;
    }
    classpath.push(client_jar);

    let classpath_sep = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let classpath = classpath
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(classpath_sep);

    let asset_index = version_json
        .get("assets")
        .or_else(|| version_json.pointer("/assetIndex/id"))
        .and_then(Value::as_str)
        .unwrap_or(version_id)
        .to_string();

    let mut replacements = HashMap::new();
    replacements.insert("auth_player_name", account.name.clone());
    replacements.insert("version_name", version_id.to_string());
    replacements.insert("game_directory", game_dir.display().to_string());
    replacements.insert("assets_root", root.join("assets").display().to_string());
    replacements.insert(
        "game_assets",
        root.join("assets")
            .join("virtual")
            .join("legacy")
            .display()
            .to_string(),
    );
    replacements.insert("assets_index_name", asset_index);
    replacements.insert("auth_uuid", account.uuid.clone());
    replacements.insert("auth_access_token", account.access_token.clone());
    replacements.insert("auth_session", account.access_token.clone());
    replacements.insert("clientid", "coral-launcher".to_string());
    replacements.insert("auth_xuid", account.xuid.clone().unwrap_or_default());
    replacements.insert("user_properties", "{}".to_string());
    replacements.insert(
        "user_type",
        if account.account_type == "offline" {
            "legacy"
        } else {
            "msa"
        }
        .to_string(),
    );
    replacements.insert(
        "version_type",
        version_json
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("release")
            .to_string(),
    );
    replacements.insert("natives_directory", natives_dir.display().to_string());
    replacements.insert("launcher_name", "CoralLauncher".to_string());
    replacements.insert("launcher_version", "0.1.0".to_string());
    replacements.insert("classpath", classpath);
    replacements.insert(
        "primary_jar",
        primary_jar_path(version_id, version_json)?
            .display()
            .to_string(),
    );
    replacements.insert(
        "library_directory",
        root.join("libraries").display().to_string(),
    );
    replacements.insert("classpath_separator", classpath_sep.to_string());

    let mut args = Vec::new();
    if max_memory_mb > 0 {
        args.push(format!("-Xmx{max_memory_mb}M"));
    }

    let mut jvm_args = collect_arguments(version_json.pointer("/arguments/jvm"), &replacements);
    if jvm_args.is_empty() {
        jvm_args.extend([
            format!("-Djava.library.path={}", natives_dir.display()),
            "-cp".to_string(),
            replacements.get("classpath").cloned().unwrap_or_default(),
        ]);
    }
    args.extend(jvm_args);

    let main_class = version_json
        .get("mainClass")
        .and_then(Value::as_str)
        .ok_or("版本描述缺少 mainClass")?;
    args.push(main_class.to_string());

    let game_args = if version_json.pointer("/arguments/game").is_some() {
        collect_arguments(version_json.pointer("/arguments/game"), &replacements)
    } else {
        version_json
            .get("minecraftArguments")
            .and_then(Value::as_str)
            .unwrap_or("")
            .split_whitespace()
            .map(|arg| replace_tokens(arg, &replacements))
            .collect()
    };
    args.extend(game_args);

    let executable = if java_path.trim().is_empty() {
        "java".to_string()
    } else {
        java_path.trim().to_string()
    };
    Ok((executable, args, game_dir))
}

#[tauri::command]
async fn launch_game(
    app: AppHandle,
    version_id: String,
    java_path: String,
    max_memory_mb: i64,
    memory_mode: Option<String>,
    account: Option<LaunchAccount>,
) -> Result<LaunchResult, String> {
    let account = account.ok_or("请先完成正版登录或创建离线账号，再启动游戏")?;
    if account.account_type != "offline" && !account.owns_game {
        return Err("该账号未检测到 Minecraft Java Edition 授权，无法启动正版会话".to_string());
    }
    emit_launch_log(&app, "info", format!("准备启动版本: {version_id}"), None);
    emit_launch_log(
        &app,
        "info",
        format!(
            "账号: {} ({})",
            account.name,
            if account.account_type == "offline" {
                "offline"
            } else {
                "microsoft"
            }
        ),
        None,
    );
    let version_json = resolve_installed_version_json(&version_id)?;
    let (resolved_game_dir, isolated, isolation_reason) =
        isolated_game_dir(&version_id, &version_json)?;
    emit_launch_log(
        &app,
        "info",
        format!(
            "Java 要求: {}{}",
            version_json
                .pointer("/javaVersion/component")
                .and_then(Value::as_str)
                .unwrap_or("未声明"),
            version_json
                .pointer("/javaVersion/majorVersion")
                .and_then(Value::as_i64)
                .map(|major| format!(" / Java {major}+"))
                .unwrap_or_default()
        ),
        None,
    );
    emit_launch_log(
        &app,
        "info",
        format!(
            "主类: {}",
            version_json
                .get("mainClass")
                .and_then(Value::as_str)
                .unwrap_or("未知")
        ),
        None,
    );
    emit_launch_log(
        &app,
        "info",
        format!(
            "版本隔离: {}（{}）",
            if isolated { "已启用" } else { "未启用" },
            isolation_reason
        ),
        None,
    );
    emit_launch_log(
        &app,
        "info",
        format!("游戏目录: {}", resolved_game_dir.display()),
        None,
    );
    let java_executable = prepare_java_executable(&app, &version_json, &java_path).await?;
    let (memory_mb, memory_recommendation) = resolve_memory_mb(
        &version_id,
        &version_json,
        memory_mode.as_deref(),
        max_memory_mb,
    )?;
    let (executable, args, game_dir) = build_launch_arguments(
        &version_id,
        &version_json,
        &java_executable,
        memory_mb,
        &account,
    )?;
    let command_preview = sanitize_command_preview(&executable, &args, &account);
    emit_launch_log(
        &app,
        "info",
        format!("工作目录: {}", game_dir.display()),
        None,
    );
    emit_launch_log(&app, "info", format!("Java: {executable}"), None);
    if let Some(recommendation) = memory_recommendation {
        emit_launch_log(
            &app,
            "info",
            format!(
                "内存模式: 自动，分配 {} MB（可用 {} MB / 总计 {} MB；{}）",
                recommendation.recommended_mb,
                recommendation.available_mb,
                recommendation.total_mb,
                recommendation.reason
            ),
            None,
        );
    } else {
        emit_launch_log(&app, "info", format!("内存模式: 手动，分配 {memory_mb} MB"), None);
    }
    emit_launch_log(&app, "command", command_preview.clone(), None);
    let mut command = Command::new(&executable);
    command
        .args(&args)
        .current_dir(&game_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(to_string)?;
    let pid = child.id();
    emit_launch_log(
        &app,
        "info",
        format!("游戏进程已创建，PID: {pid}"),
        Some(pid),
    );
    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(app.clone(), pid, "stdout", stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(app.clone(), pid, "stderr", stderr);
    }
    thread::spawn(move || {
        let status = child.wait();
        match status {
            Ok(status) => emit_launch_log(
                &app,
                "exit",
                format!(
                    "游戏进程已退出，状态码: {}",
                    status
                        .code()
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "无状态码".to_string())
                ),
                Some(pid),
            ),
            Err(error) => emit_launch_log(
                &app,
                "exit",
                format!("等待游戏进程失败: {error}"),
                Some(pid),
            ),
        }
    });
    Ok(LaunchResult {
        pid: Some(pid),
        command_preview,
        game_directory: game_dir.display().to_string(),
    })
}

#[tauri::command]
async fn preview_launch_command(
    version_id: String,
    java_path: String,
    max_memory_mb: i64,
    memory_mode: Option<String>,
    account: Option<LaunchAccount>,
) -> Result<LaunchResult, String> {
    let fallback = LaunchAccount {
        name: "Player".to_string(),
        uuid: "00000000000000000000000000000000".to_string(),
        access_token: "offline-preview".to_string(),
        xuid: None,
        owns_game: false,
        account_type: "offline".to_string(),
    };
    let version_json = resolve_installed_version_json(&version_id)?;
    let java_executable = resolve_java_for_preview(&version_json, &java_path);
    let (memory_mb, _) = resolve_memory_mb(
        &version_id,
        &version_json,
        memory_mode.as_deref(),
        max_memory_mb,
    )?;
    let (executable, args, game_dir) = build_launch_arguments(
        &version_id,
        &version_json,
        &java_executable,
        memory_mb,
        account.as_ref().unwrap_or(&fallback),
    )?;
    let command_preview =
        sanitize_command_preview(&executable, &args, account.as_ref().unwrap_or(&fallback));
    Ok(LaunchResult {
        pid: None,
        command_preview,
        game_directory: game_dir.display().to_string(),
    })
}

fn normalized_project_type(project_type: &str) -> String {
    match project_type.trim().to_ascii_lowercase().as_str() {
        "modpack" => "modpack",
        "resourcepack" => "resourcepack",
        "shader" => "shader",
        _ => "mod",
    }
    .to_string()
}

fn project_type_target_folder(project_type: &str) -> Option<&'static str> {
    match normalized_project_type(project_type).as_str() {
        "resourcepack" => Some("resourcepacks"),
        "shader" => Some("shaderpacks"),
        "mod" => Some("mods"),
        "modpack" => None,
        _ => Some("mods"),
    }
}

fn loader_applies_to_project_type(project_type: &str) -> bool {
    matches!(
        normalized_project_type(project_type).as_str(),
        "mod" | "modpack"
    )
}

fn looks_like_minecraft_version(value: &str) -> bool {
    let value = value.trim();
    value
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false)
        && (value.contains('.') || value.contains('w'))
}

fn extract_minecraft_version_token(text: &str) -> Option<String> {
    let chars = text.chars().collect::<Vec<_>>();
    for start in 0..chars.len() {
        if !chars[start].is_ascii_digit() {
            continue;
        }
        let mut end = start;
        while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == '.' || chars[end] == 'w' || chars[end].is_ascii_alphabetic()) {
            end += 1;
        }
        let token = chars[start..end]
            .iter()
            .collect::<String>()
            .trim_matches('.')
            .to_string();
        if looks_like_minecraft_version(&token) {
            return Some(token);
        }
    }
    None
}

fn argument_strings(value: Option<&Value>) -> Vec<String> {
    let mut output = Vec::new();
    let Some(Value::Array(items)) = value else {
        return output;
    };
    for item in items {
        match item {
            Value::String(text) => output.push(text.to_string()),
            Value::Object(object) => match object.get("value") {
                Some(Value::String(text)) => output.push(text.to_string()),
                Some(Value::Array(values)) => output.extend(
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned),
                ),
                _ => {}
            },
            _ => {}
        }
    }
    output
}

fn argument_value_after(version_json: &Value, flag: &str) -> Option<String> {
    let args = argument_strings(version_json.pointer("/arguments/game"));
    args.windows(2).find_map(|window| {
        if window.first().map(String::as_str) == Some(flag) {
            window.get(1).cloned()
        } else {
            None
        }
    })
}

fn modrinth_game_version(game_version: &str) -> String {
    let trimmed = game_version.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(raw_json) = read_installed_version_json(trimmed) {
        for candidate in [
            raw_json.get("clientVersion").and_then(Value::as_str),
            raw_json.get("inheritsFrom").and_then(Value::as_str),
            raw_json.get("jar").and_then(Value::as_str),
        ]
        .into_iter()
        .flatten()
        {
            if looks_like_minecraft_version(candidate) {
                return candidate.to_string();
            }
        }
        if let Some(value) = argument_value_after(&raw_json, "--fml.mcVersion") {
            if looks_like_minecraft_version(&value) {
                return value;
            }
        }
        for library in version_libraries(&raw_json) {
            if let Some(name) = library.get("name").and_then(Value::as_str) {
                if name.starts_with("net.fabricmc:intermediary:") {
                    if let Some(value) = name.split(':').nth(2) {
                        return value.to_string();
                    }
                }
            }
        }
    }
    extract_minecraft_version_token(trimmed).unwrap_or_else(|| trimmed.to_string())
}

fn target_game_dir_for_version(version_id: &str) -> Result<PathBuf, String> {
    let trimmed = version_id.trim();
    if !trimmed.is_empty() && find_version_json_path(trimmed)?.is_some() {
        let version_json = resolve_installed_version_json(trimmed)?;
        return Ok(isolated_game_dir(trimmed, &version_json)?.0);
    }
    minecraft_root()
}

fn default_resource_target_dir(game_version: &str, project_type: &str) -> Result<PathBuf, String> {
    let project_type = normalized_project_type(project_type);
    if project_type == "modpack" {
        return target_game_dir_for_version(game_version);
    }
    let target_folder = project_type_target_folder(&project_type).unwrap_or("mods");
    if find_version_json_path(game_version.trim())?.is_some() {
        Ok(target_game_dir_for_version(game_version.trim())?.join(target_folder))
    } else {
        Ok(minecraft_root()?.join(target_folder))
    }
}

#[tauri::command]
async fn get_resource_default_target_folder(
    game_version: String,
    project_type: String,
) -> Result<String, String> {
    Ok(default_resource_target_dir(&game_version, &project_type)?
        .display()
        .to_string())
}

#[tauri::command]
async fn choose_resource_target_folder(
    game_version: String,
    project_type: String,
) -> Result<Option<String>, String> {
    let default_dir = default_resource_target_dir(&game_version, &project_type)?;
    fs::create_dir_all(&default_dir).map_err(to_string)?;
    let picked = rfd::FileDialog::new()
        .set_title("选择下载到的文件夹")
        .set_directory(default_dir)
        .pick_folder();
    Ok(picked.map(|path| path.display().to_string()))
}

fn safe_relative_path(relative: &str) -> Result<PathBuf, String> {
    let mut path = PathBuf::new();
    for part in relative.replace('\\', "/").split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return Err(format!("资源包路径不安全: {relative}"));
        }
        path.push(part);
    }
    if path.as_os_str().is_empty() {
        Err(format!("资源包路径为空: {relative}"))
    } else {
        Ok(path)
    }
}

async fn install_modrinth_modpack(
    client: &Client,
    pack_path: &Path,
    game_version: &str,
) -> Result<PathBuf, String> {
    install_modrinth_modpack_to_dir(client, pack_path, target_game_dir_for_version(game_version)?).await
}

async fn install_modrinth_modpack_to_dir(
    client: &Client,
    pack_path: &Path,
    game_dir: PathBuf,
) -> Result<PathBuf, String> {
    fs::create_dir_all(&game_dir).map_err(to_string)?;

    let file = fs::File::open(pack_path).map_err(to_string)?;
    let mut archive = zip::ZipArchive::new(file).map_err(to_string)?;
    let mut index_text = String::new();
    {
        let mut index_file = archive
            .by_name("modrinth.index.json")
            .map_err(|error| format!("整合包缺少 modrinth.index.json: {error}"))?;
        index_file
            .read_to_string(&mut index_text)
            .map_err(to_string)?;
    }

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(to_string)?;
        let Some(enclosed) = entry.enclosed_name().map(PathBuf::from) else {
            continue;
        };
        let relative = enclosed
            .strip_prefix("overrides")
            .or_else(|_| enclosed.strip_prefix("client-overrides"));
        let Ok(relative) = relative else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = game_dir.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(to_string)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(to_string)?;
            }
            let mut outfile = fs::File::create(&target).map_err(to_string)?;
            std::io::copy(&mut entry, &mut outfile).map_err(to_string)?;
        }
    }

    let index_json = serde_json::from_str::<Value>(&index_text).map_err(to_string)?;
    let files = index_json
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut downloads = Vec::<DownloadFile>::new();
    for file in files {
        if file
            .pointer("/env/client")
            .and_then(Value::as_str)
            .map(|value| value.eq_ignore_ascii_case("unsupported"))
            .unwrap_or(false)
        {
            continue;
        }
        let Some(path) = file.get("path").and_then(Value::as_str) else {
            continue;
        };
        let urls = file
            .get("downloads")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if urls.is_empty() {
            continue;
        }
        downloads.push(DownloadFile {
            urls,
            path: game_dir.join(safe_relative_path(path)?),
            sha1: file
                .pointer("/hashes/sha1")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            size: file.get("fileSize").and_then(Value::as_i64),
            label: path.to_string(),
            is_json: false,
        });
    }

    let total = downloads.len();
    let results = stream::iter(downloads.into_iter())
        .map(|file| {
            let client = client.clone();
            async move {
                download_to_path(
                    &client,
                    file.urls,
                    &file.path,
                    file.sha1.as_deref(),
                    file.size,
                    false,
                )
                .await
                .map_err(|error| format!("{}: {error}", file.label))
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;
    let failed = results.into_iter().filter_map(Result::err).collect::<Vec<_>>();
    if !failed.is_empty() {
        return Err(format!(
            "整合包中 {} 个文件下载失败：{}",
            failed.len(),
            failed.into_iter().take(4).collect::<Vec<_>>().join("; ")
        ));
    }
    if total == 0 {
        fs::create_dir_all(game_dir.join("mods")).map_err(to_string)?;
    }
    Ok(game_dir)
}

#[tauri::command]
async fn search_modrinth(
    query: String,
    game_version: String,
    loader: String,
    project_type: String,
    limit: usize,
) -> Result<Value, String> {
    let client = http_client()?;
    let project_type = normalized_project_type(&project_type);
    let game_version = modrinth_game_version(&game_version);
    let mut facets = vec![vec![format!("project_type:{project_type}")]];
    if !game_version.trim().is_empty() {
        facets.push(vec![format!("versions:{}", game_version.trim())]);
    }
    if loader_applies_to_project_type(&project_type) && !loader.trim().is_empty() {
        facets.push(vec![format!("categories:{}", loader.trim())]);
    }
    let facets = serde_json::to_string(&facets).map_err(to_string)?;
    let response = client
        .get(format!("{MODRINTH_API}/search"))
        .query(&[
            ("query", query.trim()),
            ("facets", facets.as_str()),
            ("limit", &limit.min(50).to_string()),
            ("index", "relevance"),
        ])
        .send()
        .await
        .map_err(to_string)?;
    if !response.status().is_success() {
        return Err(response.text().await.map_err(to_string)?);
    }
    response.json::<Value>().await.map_err(to_string)
}

async fn fetch_modrinth_project_versions(
    client: &Client,
    project_id: String,
    game_version: String,
    loader: String,
    project_type: String,
) -> Result<Value, String> {
    let project_type = normalized_project_type(&project_type);
    let query_game_version = modrinth_game_version(&game_version);
    let mut query = Vec::new();
    if !query_game_version.trim().is_empty() {
        query.push((
            "game_versions".to_string(),
            serde_json::to_string(&vec![query_game_version.trim()]).map_err(to_string)?,
        ));
    }
    if loader_applies_to_project_type(&project_type) && !loader.trim().is_empty() {
        query.push((
            "loaders".to_string(),
            serde_json::to_string(&vec![loader.trim()]).map_err(to_string)?,
        ));
    }
    query.push(("featured".to_string(), "false".to_string()));

    let encoded_project = urlencoding::encode(project_id.trim());
    let versions_response = client
        .get(format!("{MODRINTH_API}/project/{encoded_project}/version"))
        .query(&query)
        .send()
        .await
        .map_err(to_string)?;
    if !versions_response.status().is_success() {
        return Err(versions_response.text().await.map_err(to_string)?);
    }
    let mut versions = versions_response.json::<Value>().await.map_err(to_string)?;
    if versions.as_array().map(Vec::is_empty).unwrap_or(true)
        && loader_applies_to_project_type(&project_type)
    {
        let mut fallback_query = Vec::new();
        if !query_game_version.trim().is_empty() {
            fallback_query.push((
                "game_versions".to_string(),
                serde_json::to_string(&vec![query_game_version.trim()]).map_err(to_string)?,
            ));
        }
        fallback_query.push(("featured".to_string(), "false".to_string()));
        let fallback_response = client
            .get(format!("{MODRINTH_API}/project/{encoded_project}/version"))
            .query(&fallback_query)
            .send()
            .await
            .map_err(to_string)?;
        if fallback_response.status().is_success() {
            versions = fallback_response.json::<Value>().await.map_err(to_string)?;
        }
    }
    Ok(versions)
}

fn primary_modrinth_file(version: &Value) -> Result<&Value, String> {
    let files = version
        .get("files")
        .and_then(Value::as_array)
        .ok_or("Modrinth 版本缺少文件")?;
    files
        .iter()
        .find(|file| {
            file.get("primary")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| files.first())
        .ok_or_else(|| "Modrinth 版本没有可下载文件".to_string())
}

async fn install_modrinth_version_value(
    client: &Client,
    version: &Value,
    project_id: String,
    game_version: String,
    project_type: String,
    target_dir: Option<String>,
) -> Result<ModInstallResult, String> {
    let project_type = normalized_project_type(&project_type);
    let file = primary_modrinth_file(version)?;
    let url = file
        .get("url")
        .and_then(Value::as_str)
        .ok_or("Modrinth 文件缺少下载地址")?;
    let file_name = file
        .get("filename")
        .and_then(Value::as_str)
        .ok_or("Modrinth 文件缺少文件名")?
        .to_string();
    let sha1 = file.pointer("/hashes/sha1").and_then(Value::as_str);

    if project_type == "modpack" {
        let cache_dir = launcher_root()?.join("cache").join("modpacks");
        fs::create_dir_all(&cache_dir).map_err(to_string)?;
        let pack_path = cache_dir.join(&file_name);
        download_to_path(
            client,
            vec![url.to_string()],
            &pack_path,
            sha1,
            file.get("size").and_then(Value::as_i64),
            false,
        )
        .await?;
        let installed_dir = if let Some(target_dir) = target_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            install_modrinth_modpack_to_dir(client, &pack_path, PathBuf::from(target_dir)).await?
        } else {
            install_modrinth_modpack(client, &pack_path, &game_version).await?
        };
        return Ok(ModInstallResult {
            project_id,
            version_id: version
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            file_name,
            path: installed_dir.display().to_string(),
            project_type,
        });
    }

    let target_dir = target_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or(default_resource_target_dir(&game_version, &project_type)?);
    fs::create_dir_all(&target_dir).map_err(to_string)?;
    let path = target_dir.join(&file_name);
    download_to_path(
        client,
        vec![url.to_string()],
        &path,
        sha1,
        file.get("size").and_then(Value::as_i64),
        false,
    )
    .await?;

    Ok(ModInstallResult {
        project_id,
        version_id: version
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        file_name,
        path: path.display().to_string(),
        project_type,
    })
}

#[tauri::command]
async fn get_modrinth_project_versions(
    project_id: String,
    game_version: String,
    loader: String,
    project_type: String,
) -> Result<Value, String> {
    let client = http_client()?;
    fetch_modrinth_project_versions(&client, project_id, game_version, loader, project_type).await
}

#[tauri::command]
async fn install_modrinth_project_version(
    project_id: String,
    version_id: String,
    game_version: String,
    project_type: String,
    target_dir: Option<String>,
) -> Result<ModInstallResult, String> {
    let client = http_client()?;
    let encoded_version = urlencoding::encode(version_id.trim());
    let response = client
        .get(format!("{MODRINTH_API}/version/{encoded_version}"))
        .send()
        .await
        .map_err(to_string)?;
    if !response.status().is_success() {
        return Err(response.text().await.map_err(to_string)?);
    }
    let version = response.json::<Value>().await.map_err(to_string)?;
    install_modrinth_version_value(
        &client,
        &version,
        project_id,
        game_version,
        project_type,
        target_dir,
    )
    .await
}

#[tauri::command]
async fn install_modrinth_project(
    project_id: String,
    game_version: String,
    loader: String,
    project_type: Option<String>,
) -> Result<ModInstallResult, String> {
    let client = http_client()?;
    let project_type = normalized_project_type(project_type.as_deref().unwrap_or("mod"));
    let versions = fetch_modrinth_project_versions(
        &client,
        project_id.clone(),
        game_version.clone(),
        loader,
        project_type.clone(),
    )
    .await?;
    let version = versions
        .as_array()
        .and_then(|items| items.first())
        .ok_or("没有找到匹配的 Modrinth 版本")?;
    install_modrinth_version_value(
        &client,
        version,
        project_id,
        game_version,
        project_type,
        None,
    )
    .await
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_data_paths,
            set_minecraft_root,
            choose_minecraft_root,
            scan_java_installations,
            recommend_memory,
            choose_java_executable,
            get_version_manifest,
            list_installed_versions,
            delete_installed_version,
            get_version_summary,
            get_loader_versions,
            download_version,
            begin_microsoft_device_login,
            poll_microsoft_device_login,
            get_saved_minecraft_profile,
            create_offline_profile,
            refresh_minecraft_profile,
            logout_minecraft_profile,
            launch_game,
            preview_launch_command,
            search_modrinth,
            get_modrinth_project_versions,
            get_resource_default_target_folder,
            choose_resource_target_folder,
            install_modrinth_project,
            install_modrinth_project_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running Coral Launcher");
}
