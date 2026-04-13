use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use tauri::Manager;

static RESOLVED_ADB_CANDIDATES: OnceLock<Vec<String>> = OnceLock::new();
static CONFIGURED_ADB_CANDIDATES: OnceLock<Vec<String>> = OnceLock::new();
const DEFAULT_ADB_TIMEOUT: Duration = Duration::from_secs(15);
const DEVICE_SCAN_TIMEOUT: Duration = Duration::from_secs(5);
const DUMPSYS_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidDevice {
    pub serial: String,
    pub state: String,
    pub product: Option<String>,
    pub model: Option<String>,
    pub device_code: Option<String>,
    pub transport_id: Option<String>,
    pub brand: Option<String>,
    pub manufacturer: Option<String>,
    pub android_version: Option<String>,
    pub sdk: Option<String>,
    pub build_display_id: Option<String>,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PackageSource {
    System,
    User,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Partition {
    System,
    SystemExt,
    Product,
    Vendor,
    Odm,
    Apex,
    Data,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPackage {
    pub package_name: String,
    pub install_path: Option<String>,
    pub source: PackageSource,
    pub visible_on_launcher: bool,
    pub visible_as_home: bool,
    pub partition: Partition,
    pub is_privileged: bool,
    pub is_overlay: bool,
    pub is_apex: bool,
    pub has_code: bool,
    pub is_persistent: bool,
    pub is_updated_system_app: bool,
    pub shared_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRuntimeProfile {
    pub home_package: Option<String>,
    pub browser_role_holders: Vec<String>,
    pub dialer_role_holders: Vec<String>,
    pub sms_role_holders: Vec<String>,
    pub assistant_role_holders: Vec<String>,
    pub wallet_role_holders: Vec<String>,
}

const PROP_KEYS: [(&str, &str); 6] = [
    ("brand", "ro.product.brand"),
    ("manufacturer", "ro.product.manufacturer"),
    ("android_version", "ro.build.version.release"),
    ("sdk", "ro.build.version.sdk"),
    ("build_display_id", "ro.build.display.id"),
    ("fingerprint", "ro.build.fingerprint"),
];

const ROLE_NAMES: [(&str, &str); 5] = [
    ("browser", "android.app.role.BROWSER"),
    ("dialer", "android.app.role.DIALER"),
    ("sms", "android.app.role.SMS"),
    ("assistant", "android.app.role.ASSISTANT"),
    ("wallet", "android.app.role.WALLET"),
];

pub fn scan_devices() -> Result<Vec<AndroidDevice>, String> {
    let output = run_adb(&["devices", "-l"])?;
    let mut devices = Vec::new();

    for line in output.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut device = parse_device_line(trimmed)?;

        if device.state == "device" {
            hydrate_device_props(&mut device)?;
        }

        devices.push(device);
    }

    Ok(devices)
}

pub fn is_device_ready(serial: &str) -> Result<bool, String> {
    let output = run_adb(&["-s", serial, "get-state"])?;
    Ok(output.trim() == "device")
}

pub fn collect_package_inventory(serial: &str) -> Result<Vec<InstalledPackage>, String> {
    let system_packages = parse_package_list(
        &run_adb(&["-s", serial, "shell", "pm", "list", "packages", "-s", "-f"])?,
        PackageSource::System,
    );
    let user_packages = parse_package_list(
        &run_adb(&["-s", serial, "shell", "pm", "list", "packages", "-3", "-f"])?,
        PackageSource::User,
    );

    let mut merged = HashMap::new();

    for package in system_packages {
        merged.insert(package.package_name.clone(), package);
    }

    for user_package in user_packages {
        if let Some(existing) = merged.get_mut(&user_package.package_name) {
            existing.install_path = user_package.install_path;
            existing.partition = infer_partition(existing.install_path.as_deref());
            existing.is_privileged = path_contains(existing.install_path.as_deref(), "/priv-app/");
            existing.is_overlay = infer_overlay(
                &existing.package_name,
                existing.install_path.as_deref(),
                existing.has_code,
            );
            existing.is_apex = existing.partition == Partition::Apex;
            existing.is_updated_system_app = true;
        } else {
            merged.insert(user_package.package_name.clone(), user_package);
        }
    }

    let package_names: Vec<String> = merged.keys().cloned().collect();

    let launcher_packages = collect_launcher_packages(serial)?;
    let home_packages = collect_home_packages(serial)?;
    let batch_flags = collect_batch_flags(serial, &package_names)?;

    for package_name in &package_names {
        if let Some(package) = merged.get_mut(package_name) {
            package.visible_on_launcher = launcher_packages.contains(package_name);
            package.visible_as_home = home_packages.contains(package_name);

            if let Some(flags) = batch_flags.get(package_name) {
                package.has_code = flags.has_code;
                package.is_persistent = flags.is_persistent;
                package.shared_user_id = flags.shared_user_id.clone();
            }

            package.is_overlay = infer_overlay(
                &package.package_name,
                package.install_path.as_deref(),
                package.has_code,
            );
        }
    }

    let mut packages: Vec<InstalledPackage> = merged.into_values().collect();
    packages.sort_by(|left, right| left.package_name.cmp(&right.package_name));
    Ok(packages)
}

pub fn collect_runtime_profile(serial: &str) -> DeviceRuntimeProfile {
    let home_package = resolve_home_package(serial)
        .ok()
        .filter(|value| !value.is_empty());

    let mut roles = HashMap::new();
    for (field, role_name) in ROLE_NAMES {
        roles.insert(
            field,
            resolve_role_holders(serial, role_name).unwrap_or_default(),
        );
    }

    DeviceRuntimeProfile {
        home_package,
        browser_role_holders: roles.remove("browser").unwrap_or_default(),
        dialer_role_holders: roles.remove("dialer").unwrap_or_default(),
        sms_role_holders: roles.remove("sms").unwrap_or_default(),
        assistant_role_holders: roles.remove("assistant").unwrap_or_default(),
        wallet_role_holders: roles.remove("wallet").unwrap_or_default(),
    }
}

fn parse_device_line(line: &str) -> Result<AndroidDevice, String> {
    let mut columns = line.split_whitespace();
    let serial = columns
        .next()
        .ok_or_else(|| format!("无法解析设备序列号: {line}"))?;
    let state = columns
        .next()
        .ok_or_else(|| format!("无法解析设备状态: {line}"))?;

    let mut fields = HashMap::new();
    for part in columns {
        if let Some((key, value)) = part.split_once(':') {
            fields.insert(key.to_string(), value.to_string());
        }
    }

    Ok(AndroidDevice {
        serial: serial.to_string(),
        state: state.to_string(),
        product: fields.remove("product"),
        model: fields.remove("model"),
        device_code: fields.remove("device"),
        transport_id: fields.remove("transport_id"),
        brand: None,
        manufacturer: None,
        android_version: None,
        sdk: None,
        build_display_id: None,
        fingerprint: None,
    })
}

fn parse_package_list(output: &str, source: PackageSource) -> Vec<InstalledPackage> {
    output
        .lines()
        .filter_map(|line| parse_package_line(line, source))
        .collect()
}

fn parse_package_line(line: &str, source: PackageSource) -> Option<InstalledPackage> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let payload = trimmed.strip_prefix("package:")?;
    let (install_path, package_name) = if let Some((path, package)) = payload.rsplit_once('=') {
        (Some(path.to_string()), package.to_string())
    } else {
        (None, payload.to_string())
    };

    let partition = infer_partition(install_path.as_deref());
    let is_privileged = path_contains(install_path.as_deref(), "/priv-app/");
    let has_code = !path_contains(install_path.as_deref(), "/overlay/");
    let is_overlay = infer_overlay(&package_name, install_path.as_deref(), has_code);

    Some(InstalledPackage {
        package_name: package_name.clone(),
        install_path,
        source,
        visible_on_launcher: false,
        visible_as_home: false,
        partition,
        is_privileged,
        is_overlay,
        is_apex: partition == Partition::Apex,
        has_code,
        is_persistent: false,
        is_updated_system_app: false,
        shared_user_id: None,
    })
}

fn hydrate_device_props(device: &mut AndroidDevice) -> Result<(), String> {
    for (field, prop_key) in PROP_KEYS {
        let value = get_prop(&device.serial, prop_key)?;
        if value.is_empty() {
            continue;
        }

        match field {
            "brand" => device.brand = Some(value),
            "manufacturer" => device.manufacturer = Some(value),
            "android_version" => device.android_version = Some(value),
            "sdk" => device.sdk = Some(value),
            "build_display_id" => device.build_display_id = Some(value),
            "fingerprint" => device.fingerprint = Some(value),
            _ => {}
        }
    }

    Ok(())
}

fn get_prop(serial: &str, key: &str) -> Result<String, String> {
    run_adb(&["-s", serial, "shell", "getprop", key]).map(|value| value.trim().to_string())
}

fn collect_launcher_packages(serial: &str) -> Result<HashSet<String>, String> {
    let output = run_adb(&[
        "-s",
        serial,
        "shell",
        "cmd",
        "package",
        "query-activities",
        "--brief",
        "-a",
        "android.intent.action.MAIN",
        "-c",
        "android.intent.category.LAUNCHER",
    ])?;
    Ok(parse_activity_package_names(&output))
}

fn collect_home_packages(serial: &str) -> Result<HashSet<String>, String> {
    let output = run_adb(&[
        "-s",
        serial,
        "shell",
        "cmd",
        "package",
        "query-activities",
        "--brief",
        "-a",
        "android.intent.action.MAIN",
        "-c",
        "android.intent.category.HOME",
    ])?;
    Ok(parse_activity_package_names(&output))
}

fn parse_activity_package_names(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("priority=")
                || trimmed.contains("No activity")
                || trimmed.contains("cannot be resolved")
            {
                return None;
            }
            let package_name = trimmed.split('/').next()?.trim();
            if package_name.contains('.') {
                Some(package_name.to_string())
            } else {
                None
            }
        })
        .collect()
}

#[derive(Debug, Default)]
struct BatchFlags {
    has_code: bool,
    is_persistent: bool,
    shared_user_id: Option<String>,
}

fn collect_batch_flags(
    serial: &str,
    _package_names: &[String],
) -> Result<HashMap<String, BatchFlags>, String> {
    let output = match run_adb(&["-s", serial, "shell", "dumpsys", "package", "packages"]) {
        Ok(output) => output,
        Err(packages_error) => run_adb(&["-s", serial, "shell", "dumpsys", "package"]).map_err(
            |full_error| {
                format!(
                    "读取完整包详情失败；`dumpsys package packages` 报错: {packages_error}；`dumpsys package` 报错: {full_error}"
                )
            },
        )?,
    };
    Ok(parse_batch_flags(&output))
}

fn parse_batch_flags(output: &str) -> HashMap<String, BatchFlags> {
    let mut result: HashMap<String, BatchFlags> = HashMap::new();
    let mut current_pkg: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("Package [") {
            if let Some(end) = rest.find(']') {
                if let Some(pkg) = current_pkg.take() {
                    result.entry(pkg).or_insert(BatchFlags {
                        has_code: true,
                        ..BatchFlags::default()
                    });
                }
                current_pkg = Some(rest[..end].to_string());
            }
        }

        if let Some(ref pkg) = current_pkg {
            let flags = result.entry(pkg.clone()).or_insert(BatchFlags {
                has_code: true,
                ..BatchFlags::default()
            });

            if trimmed.contains("hasCode=false") {
                flags.has_code = false;
            }
            if trimmed.contains("PERSISTENT") || trimmed.contains("android:persistent=true") {
                flags.is_persistent = true;
            }
            if flags.shared_user_id.is_none() {
                if let Some(uid) = extract_shared_user_id(trimmed) {
                    flags.shared_user_id = Some(uid);
                }
            }
        }
    }

    if let Some(pkg) = current_pkg {
        result.entry(pkg).or_insert(BatchFlags {
            has_code: true,
            ..BatchFlags::default()
        });
    }

    result
}

pub fn resolve_home_package(serial: &str) -> Result<String, String> {
    let output = run_adb(&[
        "-s",
        serial,
        "shell",
        "cmd",
        "package",
        "resolve-activity",
        "--brief",
        "--user",
        "0",
        "-a",
        "android.intent.action.MAIN",
        "-c",
        "android.intent.category.HOME",
    ])?;

    Ok(parse_resolved_package(&output).unwrap_or_default())
}

pub fn is_package_available_for_user_zero(
    serial: &str,
    package_name: &str,
) -> Result<bool, String> {
    let output = run_adb(&[
        "-s",
        serial,
        "shell",
        "pm",
        "list",
        "packages",
        "--user",
        "0",
        package_name,
    ])?;

    Ok(output
        .lines()
        .map(str::trim)
        .any(|line| line == format!("package:{package_name}")))
}

pub fn uninstall_package_for_user_zero(serial: &str, package_name: &str) -> Result<String, String> {
    run_adb(&[
        "-s",
        serial,
        "shell",
        "pm",
        "uninstall",
        "--user",
        "0",
        package_name,
    ])
}

pub fn restore_package_for_user_zero(serial: &str, package_name: &str) -> Result<String, String> {
    run_adb(&[
        "-s",
        serial,
        "shell",
        "cmd",
        "package",
        "install-existing",
        "--user",
        "0",
        package_name,
    ])
}

pub fn run_adb(args: &[&str]) -> Result<String, String> {
    let timeout = adb_timeout_for(args);
    let mut spawn_failures = Vec::new();

    for adb_path in resolve_adb_candidates() {
        match run_adb_with_path(&adb_path, args, timeout) {
            Ok(output) => return Ok(output),
            Err(AdbRunError::Spawn(error)) => {
                spawn_failures.push(format!("{adb_path}: {error}"));
            }
            Err(AdbRunError::Command(error)) => return Err(error),
        }
    }

    if spawn_failures.is_empty() {
        Err("未找到可用的 adb，可检查安装包资源或系统是否已安装 adb。".to_string())
    } else {
        Err(format!(
            "执行 adb 失败：未找到可用的 adb。已尝试这些路径：{}",
            spawn_failures.join(" | ")
        ))
    }
}

fn run_adb_with_path(
    adb_path: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String, AdbRunError> {
    let mut child = Command::new(adb_path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AdbRunError::Spawn(error.to_string()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AdbRunError::Command("无法接管 adb stdout 管道".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AdbRunError::Command("无法接管 adb stderr 管道".into()))?;

    let stdout_reader = thread::spawn(move || -> Result<Vec<u8>, String> {
        let mut stdout = stdout;
        let mut buffer = Vec::new();
        stdout
            .read_to_end(&mut buffer)
            .map_err(|error| format!("读取 adb stdout 失败: {error}"))?;
        Ok(buffer)
    });

    let stderr_reader = thread::spawn(move || -> Result<Vec<u8>, String> {
        let mut stderr = stderr;
        let mut buffer = Vec::new();
        stderr
            .read_to_end(&mut buffer)
            .map_err(|error| format!("读取 adb stderr 失败: {error}"))?;
        Ok(buffer)
    });

    let started_at = Instant::now();
    let exit_status = loop {
        match child
            .try_wait()
            .map_err(|error| AdbRunError::Command(format!("等待 adb 进程状态失败: {error}")))?
        {
            Some(status) => break status,
            None => {}
        }

        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(AdbRunError::Command(format!(
                "adb {:?} 超时: {} 秒内未返回",
                args,
                timeout.as_secs()
            )));
        }

        thread::sleep(Duration::from_millis(50));
    };

    let stdout = stdout_reader
        .join()
        .map_err(|_| AdbRunError::Command("读取 adb stdout 线程异常退出".into()))?
        .map_err(AdbRunError::Command)?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| AdbRunError::Command("读取 adb stderr 线程异常退出".into()))?
        .map_err(AdbRunError::Command)?;

    if !exit_status.success() {
        let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(AdbRunError::Command(format!(
            "adb {:?} 失败: {}",
            args, detail
        )));
    }

    Ok(String::from_utf8_lossy(&stdout).to_string())
}

#[derive(Debug)]
enum AdbRunError {
    Spawn(String),
    Command(String),
}

fn resolve_adb_candidates() -> Vec<String> {
    RESOLVED_ADB_CANDIDATES
        .get_or_init(|| {
            let mut candidates = Vec::new();

            if let Some(configured) = CONFIGURED_ADB_CANDIDATES.get() {
                for candidate in configured {
                    push_candidate(&mut candidates, PathBuf::from(candidate));
                }
            }

            if let Ok(explicit_path) = std::env::var("ANDROID_DEBLOAT_STUDIO_ADB_PATH") {
                push_candidate(&mut candidates, PathBuf::from(explicit_path));
            }

            if cfg!(target_os = "macos") {
                collect_macos_bundle_adb_candidates(&mut candidates);
            } else if cfg!(target_os = "windows") {
                collect_windows_bundle_adb_candidates(&mut candidates);
            }

            let system_candidates = if cfg!(target_os = "macos") {
                vec![
                    "/opt/homebrew/bin/adb",
                    "/usr/local/bin/adb",
                    "/usr/bin/adb",
                ]
            } else if cfg!(target_os = "linux") {
                vec!["/usr/local/bin/adb", "/usr/bin/adb"]
            } else {
                Vec::new()
            };

            for candidate in system_candidates {
                push_candidate(&mut candidates, PathBuf::from(candidate));
            }

            let lookup_command = if cfg!(target_os = "windows") {
                ("where", "adb")
            } else {
                ("which", "adb")
            };

            if let Ok(output) = Command::new(lookup_command.0)
                .arg(lookup_command.1)
                .output()
            {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    for match_path in path.lines() {
                        push_candidate(&mut candidates, PathBuf::from(match_path.trim()));
                    }
                }
            }

            if candidates.is_empty() {
                candidates.push(if cfg!(target_os = "windows") {
                    "adb.exe".to_string()
                } else {
                    "adb".to_string()
                });
            }

            candidates
        })
        .clone()
}

pub fn configure_adb_candidates<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let mut candidates = Vec::new();

    if let Ok(resource_dir) = app.path().resource_dir() {
        push_bundle_adb_candidates_from_base(&mut candidates, &resource_dir);
    }

    if let Ok(executable_dir) = app.path().executable_dir() {
        push_bundle_adb_candidates_from_base(&mut candidates, &executable_dir);
        push_bundle_adb_candidates_from_base(&mut candidates, &executable_dir.join("../Resources"));
        push_bundle_adb_candidates_from_base(
            &mut candidates,
            &executable_dir.join("../Resources/resources"),
        );
    }

    let _ = CONFIGURED_ADB_CANDIDATES.set(candidates);
}

fn collect_macos_bundle_adb_candidates(candidates: &mut Vec<String>) {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            push_candidate(candidates, exe_dir.join("../Resources/resources/bin/adb"));
            push_candidate(candidates, exe_dir.join("../Resources/bin/adb"));
            push_candidate(candidates, exe_dir.join("../../resources/bin/adb"));
        }

        for ancestor in exe_path.ancestors() {
            push_candidate(candidates, ancestor.join("Resources/resources/bin/adb"));
            push_candidate(
                candidates,
                ancestor.join("Contents/Resources/resources/bin/adb"),
            );
            push_candidate(candidates, ancestor.join("resources/bin/adb"));
        }
    }
}

fn collect_windows_bundle_adb_candidates(candidates: &mut Vec<String>) {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            push_bundle_adb_candidates_from_base(candidates, exe_dir);
            push_bundle_adb_candidates_from_base(candidates, &exe_dir.join("resources"));
        }

        for ancestor in exe_path.ancestors() {
            push_bundle_adb_candidates_from_base(candidates, ancestor);
            push_bundle_adb_candidates_from_base(candidates, &ancestor.join("resources"));
        }
    }
}

fn push_bundle_adb_candidates_from_base(candidates: &mut Vec<String>, base: &Path) {
    for relative in [
        "resources/bin/adb",
        "resources/bin/adb.exe",
        "resources/bin/windows/adb",
        "resources/bin/windows/adb.exe",
        "bin/adb",
        "bin/adb.exe",
        "windows/adb",
        "windows/adb.exe",
        "adb",
        "adb.exe",
    ] {
        push_candidate(candidates, base.join(relative));
    }
}

fn push_candidate(candidates: &mut Vec<String>, path: PathBuf) {
    if let Some(normalized) = normalize_candidate(&path) {
        if !candidates.iter().any(|existing| existing == &normalized) {
            candidates.push(normalized);
        }
    }
}

fn normalize_candidate(path: &Path) -> Option<String> {
    if path.as_os_str().is_empty() {
        return None;
    }

    let exists = path.is_file();
    let normalized = if exists {
        std::fs::canonicalize(path)
            .ok()
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    };

    Some(normalized.to_string_lossy().to_string())
}

fn adb_timeout_for(args: &[&str]) -> Duration {
    if args == ["devices", "-l"] || args.ends_with(&["get-state"]) {
        DEVICE_SCAN_TIMEOUT
    } else if args
        .windows(2)
        .any(|window| window == ["dumpsys", "package"])
    {
        DUMPSYS_TIMEOUT
    } else {
        DEFAULT_ADB_TIMEOUT
    }
}

fn infer_partition(install_path: Option<&str>) -> Partition {
    let Some(install_path) = install_path else {
        return Partition::Unknown;
    };

    if install_path.starts_with("/system_ext/") {
        Partition::SystemExt
    } else if install_path.starts_with("/system/") {
        Partition::System
    } else if install_path.starts_with("/product/") {
        Partition::Product
    } else if install_path.starts_with("/vendor/") {
        Partition::Vendor
    } else if install_path.starts_with("/odm/") {
        Partition::Odm
    } else if install_path.starts_with("/apex/") {
        Partition::Apex
    } else if install_path.starts_with("/data/") {
        Partition::Data
    } else {
        Partition::Unknown
    }
}

fn infer_overlay(package_name: &str, install_path: Option<&str>, has_code: bool) -> bool {
    path_contains(install_path, "/overlay/")
        || package_name.contains(".overlay")
        || (!has_code && package_name.starts_with("android."))
}

fn path_contains(install_path: Option<&str>, needle: &str) -> bool {
    install_path.unwrap_or_default().contains(needle)
}

fn resolve_role_holders(serial: &str, role_name: &str) -> Result<Vec<String>, String> {
    let output = run_adb(&[
        "-s",
        serial,
        "shell",
        "cmd",
        "role",
        "get-role-holders",
        role_name,
    ])?;
    Ok(parse_role_holders(&output))
}

fn parse_role_holders(output: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut holders = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains('.') {
            continue;
        }

        let package_name = trimmed.split('/').next().unwrap_or_default().trim();
        if !package_name.is_empty() && seen.insert(package_name.to_string()) {
            holders.push(package_name.to_string());
        }
    }

    holders
}

fn extract_shared_user_id(line: &str) -> Option<String> {
    if let Some(start) = line.find("android.uid.") {
        let rest = &line[start..];
        let value = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '.' || *ch == '_')
            .collect::<String>();
        if !value.is_empty() {
            return Some(value);
        }
    }

    line.strip_prefix("sharedUserId=")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn parse_resolved_package(output: &str) -> Option<String> {
    output.lines().map(str::trim).rev().find_map(|line| {
        if line.is_empty()
            || line.starts_with("priority=")
            || line.contains("No activity found")
            || line.contains("cannot be resolved")
        {
            return None;
        }

        let package_name = line.split('/').next().unwrap_or_default().trim();
        if package_name.contains('.') {
            Some(package_name.to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        adb_timeout_for, extract_shared_user_id, infer_overlay, infer_partition, parse_batch_flags,
        parse_device_line, parse_package_line, parse_resolved_package, parse_role_holders,
        DeviceRuntimeProfile, PackageSource, Partition, DEFAULT_ADB_TIMEOUT, DEVICE_SCAN_TIMEOUT,
        DUMPSYS_TIMEOUT,
    };

    #[test]
    fn parses_connected_device_line() {
        let device = parse_device_line(
            "EWS4C18822013788 device usb:2-1 product:COR-AL10 model:COR_AL10 device:HWCOR transport_id:1",
        )
        .expect("should parse connected device");

        assert_eq!(device.serial, "EWS4C18822013788");
        assert_eq!(device.state, "device");
        assert_eq!(device.product.as_deref(), Some("COR-AL10"));
        assert_eq!(device.model.as_deref(), Some("COR_AL10"));
        assert_eq!(device.device_code.as_deref(), Some("HWCOR"));
        assert_eq!(device.transport_id.as_deref(), Some("1"));
    }

    #[test]
    fn parses_unauthorized_device_line() {
        let device = parse_device_line("emulator-5554 unauthorized transport_id:3")
            .expect("should parse unauthorized device");

        assert_eq!(device.serial, "emulator-5554");
        assert_eq!(device.state, "unauthorized");
        assert_eq!(device.transport_id.as_deref(), Some("3"));
        assert!(device.product.is_none());
    }

    #[test]
    fn parses_package_line_with_install_path() {
        let package = parse_package_line(
            "package:/system/priv-app/HwMediaCenter/HwMediaCenter.apk=com.android.mediacenter",
            PackageSource::System,
        )
        .expect("should parse package line");

        assert_eq!(package.package_name, "com.android.mediacenter");
        assert_eq!(
            package.install_path.as_deref(),
            Some("/system/priv-app/HwMediaCenter/HwMediaCenter.apk")
        );
        assert_eq!(package.source, PackageSource::System);
        assert_eq!(package.partition, Partition::System);
        assert!(package.is_privileged);
        assert!(!package.visible_on_launcher);
    }

    #[test]
    fn parses_package_line_without_install_path() {
        let package =
            parse_package_line("package:com.example.app", PackageSource::User).expect("package");

        assert_eq!(package.package_name, "com.example.app");
        assert!(package.install_path.is_none());
        assert_eq!(package.partition, Partition::Unknown);
        assert_eq!(package.source, PackageSource::User);
    }

    #[test]
    fn parses_resolved_home_package() {
        let package_name = parse_resolved_package(
            "priority=0 preferredOrder=0 match=0x108000 specificIndex=-1 isDefault=true\ncom.huawei.android.launcher/.unihome.UniHomeLauncher",
        )
        .expect("should parse resolved package");

        assert_eq!(package_name, "com.huawei.android.launcher");
    }

    #[test]
    fn parses_package_facts_from_dumpsys() {
        let flags = parse_batch_flags(
            r#"
Packages:
  Package [com.example.test] (abcdef):
            pkgFlags=[ SYSTEM UPDATED_SYSTEM_APP HAS_CODE ALLOW_CLEAR_USER_DATA ]
            sharedUser=SharedUserSetting{1234 android.uid.system/1000}
            hasCode=false
            Category: "android.intent.category.LAUNCHER"
            Category: "android.intent.category.HOME"
            android:persistent=true
            "#,
        );

        let facts = flags.get("com.example.test").expect("should find package");
        assert!(!facts.has_code);
        assert!(facts.is_persistent);
        assert_eq!(facts.shared_user_id.as_deref(), Some("android.uid.system"));
    }

    #[test]
    fn parses_role_holders() {
        let holders = parse_role_holders("com.miui.home\n\ncom.android.browser/.Main\n");
        assert_eq!(holders, vec!["com.miui.home", "com.android.browser"]);
    }

    #[test]
    fn infers_partition_and_overlay() {
        assert_eq!(
            infer_partition(Some("/product/overlay/Foo.apk")),
            Partition::Product
        );
        assert!(infer_overlay(
            "android.miui.overlay",
            Some("/product/overlay/Foo.apk"),
            false
        ));
    }

    #[test]
    fn extracts_shared_user_id() {
        assert_eq!(
            extract_shared_user_id("sharedUser=SharedUserSetting{ffff android.uid.phone/1001}")
                .as_deref(),
            Some("android.uid.phone")
        );
    }

    #[test]
    fn runtime_profile_default_is_empty() {
        let profile = DeviceRuntimeProfile::default();
        assert!(profile.home_package.is_none());
        assert!(profile.browser_role_holders.is_empty());
    }

    #[test]
    fn applies_expected_adb_timeouts() {
        assert_eq!(adb_timeout_for(&["devices", "-l"]), DEVICE_SCAN_TIMEOUT);
        assert_eq!(
            adb_timeout_for(&["-s", "serial", "shell", "dumpsys", "package"]),
            DUMPSYS_TIMEOUT
        );
        assert_eq!(
            adb_timeout_for(&["-s", "serial", "shell", "pm", "list", "packages"]),
            DEFAULT_ADB_TIMEOUT
        );
    }

    #[test]
    #[ignore = "requires a real adb-connected device"]
    fn smoke_scans_connected_devices() {
        let devices = super::scan_devices().expect("should scan adb devices");
        assert!(
            devices.iter().any(|device| device.state == "device"),
            "expected at least one connected adb device"
        );
    }
}
