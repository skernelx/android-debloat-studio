use crate::adb::{self, PackageSource};
use crate::analyzer::{self, DeviceAnalysis, RecommendedAction};
use crate::records::{self, OperationHistoryEntry, OperationKind};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const SETTINGS_CANDIDATES: &[&str] = &["com.android.settings", "com.xiaomi.misettings"];
const SYSTEM_UI_CANDIDATES: &[&str] = &["com.android.systemui", "com.miui.systemui"];
const PHONE_CANDIDATES: &[&str] = &[
    "com.android.dialer",
    "com.google.android.dialer",
    "com.android.contacts",
    "com.huawei.contacts",
    "com.samsung.android.dialer",
    "com.samsung.android.contacts",
    "com.coloros.dialer",
    "com.android.server.telecom",
    "com.android.phone",
];
const CAMERA_CANDIDATES: &[&str] = &[
    "com.android.camera",
    "com.huawei.camera",
    "com.sec.android.app.camera",
    "com.oppo.camera",
    "com.coloros.camera",
    "com.vivo.camera",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationTargets {
    pub home_package: String,
    pub settings_package: Option<String>,
    pub system_ui_package: Option<String>,
    pub phone_package: Option<String>,
    pub camera_package: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlanEntry {
    pub package_name: String,
    pub install_path: Option<String>,
    pub visible_on_launcher: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub serial: String,
    pub vendor_family: String,
    pub generated_at_ms: u64,
    pub packages: Vec<CleanupPlanEntry>,
    pub verification_targets: VerificationTargets,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CheckStatus {
    Passed,
    Failed,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    pub key: String,
    pub label: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceHealthReport {
    pub checked_at_ms: u64,
    pub passed: bool,
    pub checks: Vec<HealthCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PackageOperationStatus {
    Removed,
    Restored,
    Reverted,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageOperationResult {
    pub package_name: String,
    pub status: PackageOperationStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExecutionReport {
    pub serial: String,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub aborted: bool,
    pub removed_count: usize,
    pub failed_count: usize,
    pub rollback_ready: bool,
    pub results: Vec<PackageOperationResult>,
    pub health_report: DeviceHealthReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupRestoreReport {
    pub serial: String,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub restored_count: usize,
    pub failed_count: usize,
    pub results: Vec<PackageOperationResult>,
    pub health_report: DeviceHealthReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RollbackRecord {
    serial: String,
    plan: CleanupPlan,
    executed_at_ms: u64,
    removed_packages: Vec<String>,
    health_report: DeviceHealthReport,
}

struct PreparedContext {
    analysis: DeviceAnalysis,
    verification_targets: VerificationTargets,
    warnings: Vec<String>,
}

pub fn generate_cleanup_plan(serial: &str) -> Result<CleanupPlan, String> {
    let context = prepare_context(serial)?;
    Ok(build_cleanup_plan(
        &context.analysis,
        context.verification_targets,
        context.warnings,
    ))
}

pub fn execute_cleanup(
    app: &AppHandle,
    serial: &str,
    requested_packages: &[String],
) -> Result<CleanupExecutionReport, String> {
    let plan = generate_cleanup_plan(serial)?;
    let requested_set: HashSet<&str> = requested_packages.iter().map(String::as_str).collect();
    let mut queue: Vec<String> = plan
        .packages
        .iter()
        .filter(|item| {
            requested_set.is_empty() || requested_set.contains(item.package_name.as_str())
        })
        .map(|item| item.package_name.clone())
        .collect();

    if queue.is_empty() {
        return Err("当前没有可执行的安全清理包".into());
    }

    let baseline = verify_device_health(serial, &plan.verification_targets);
    if !baseline.passed {
        return Err("执行前验活未通过，拒绝开始清理".into());
    }

    let started_at_ms = now_ms();
    let mut results = Vec::new();
    let mut removed_packages = Vec::new();
    let mut aborted = false;
    let mut last_health = baseline;

    for package_name in queue.drain(..) {
        match adb::uninstall_package_for_user_zero(serial, &package_name) {
            Ok(detail) => {
                let health = verify_device_health(serial, &plan.verification_targets);
                if health.passed {
                    removed_packages.push(package_name.clone());
                    last_health = health;
                    results.push(PackageOperationResult {
                        package_name,
                        status: PackageOperationStatus::Removed,
                        detail: normalize_detail(detail, "已为 user 0 卸载"),
                    });
                    continue;
                }

                let rollback_detail =
                    match adb::restore_package_for_user_zero(serial, &package_name) {
                        Ok(output) => format!(
                            "{}；验活异常：{}",
                            normalize_detail(output, "验活失败后已自动恢复"),
                            summarize_health_failures(&health)
                        ),
                        Err(error) => format!(
                            "验活失败（{}），且自动恢复也失败: {error}",
                            summarize_health_failures(&health)
                        ),
                    };

                results.push(PackageOperationResult {
                    package_name,
                    status: PackageOperationStatus::Reverted,
                    detail: rollback_detail,
                });
                last_health = verify_device_health(serial, &plan.verification_targets);
                aborted = true;
                break;
            }
            Err(error) => {
                results.push(PackageOperationResult {
                    package_name,
                    status: PackageOperationStatus::Failed,
                    detail: error,
                });
                aborted = true;
                last_health = verify_device_health(serial, &plan.verification_targets);
                break;
            }
        }
    }

    if !removed_packages.is_empty() {
        let record = RollbackRecord {
            serial: serial.to_string(),
            plan: plan.clone(),
            executed_at_ms: now_ms(),
            removed_packages: removed_packages.clone(),
            health_report: last_health.clone(),
        };
        persist_rollback_record(app, &record)?;
    }

    let finished_at_ms = now_ms();
    let removed_count = results
        .iter()
        .filter(|item| item.status == PackageOperationStatus::Removed)
        .count();
    let failed_count = results
        .iter()
        .filter(|item| {
            item.status == PackageOperationStatus::Failed
                || item.status == PackageOperationStatus::Reverted
        })
        .count();

    records::append_history_entry(
        app,
        OperationHistoryEntry {
            id: records::history_entry_id(),
            kind: OperationKind::Cleanup,
            serial: serial.to_string(),
            vendor_family: plan.vendor_family.clone(),
            timestamp_ms: finished_at_ms,
            package_count: results.len(),
            success_count: removed_count,
            failed_count,
            aborted,
            health_passed: last_health.passed,
            summary: format!("删除 {} 个包，失败/回退 {} 个", removed_count, failed_count),
        },
    )?;

    Ok(CleanupExecutionReport {
        serial: serial.to_string(),
        started_at_ms,
        finished_at_ms,
        aborted,
        removed_count,
        failed_count,
        rollback_ready: !removed_packages.is_empty(),
        results,
        health_report: last_health,
    })
}

pub fn restore_cleanup(app: &AppHandle, serial: &str) -> Result<CleanupRestoreReport, String> {
    let record_path = rollback_record_path(app, serial)?;
    let record = load_rollback_record(&record_path)?;

    if record.removed_packages.is_empty() {
        return Err("最近一次清理没有可恢复的包".into());
    }

    let started_at_ms = now_ms();
    let mut results = Vec::new();
    let mut remaining_packages = Vec::new();

    for package_name in record.removed_packages.iter().rev() {
        match adb::restore_package_for_user_zero(serial, package_name) {
            Ok(detail) => results.push(PackageOperationResult {
                package_name: package_name.clone(),
                status: PackageOperationStatus::Restored,
                detail: normalize_detail(detail, "已恢复到 user 0"),
            }),
            Err(error) => {
                remaining_packages.push(package_name.clone());
                results.push(PackageOperationResult {
                    package_name: package_name.clone(),
                    status: PackageOperationStatus::Failed,
                    detail: error,
                });
            }
        }
    }

    let health_report = verify_device_health(serial, &record.plan.verification_targets);
    let finished_at_ms = now_ms();
    let vendor_family = record.plan.vendor_family.clone();
    let restored_count = results
        .iter()
        .filter(|item| item.status == PackageOperationStatus::Restored)
        .count();
    let failed_count = results
        .iter()
        .filter(|item| item.status == PackageOperationStatus::Failed)
        .count();

    if remaining_packages.is_empty() {
        if record_path.exists() {
            fs::remove_file(&record_path).map_err(|error| format!("删除回滚记录失败: {error}"))?;
        }
    } else {
        let next_record = RollbackRecord {
            removed_packages: remaining_packages,
            health_report: health_report.clone(),
            ..record
        };
        persist_record_to_path(&record_path, &next_record)?;
    }

    records::append_history_entry(
        app,
        OperationHistoryEntry {
            id: records::history_entry_id(),
            kind: OperationKind::Restore,
            serial: serial.to_string(),
            vendor_family,
            timestamp_ms: finished_at_ms,
            package_count: results.len(),
            success_count: restored_count,
            failed_count,
            aborted: false,
            health_passed: health_report.passed,
            summary: format!("恢复 {} 个包，失败 {} 个", restored_count, failed_count),
        },
    )?;

    Ok(CleanupRestoreReport {
        serial: serial.to_string(),
        started_at_ms,
        finished_at_ms,
        restored_count,
        failed_count,
        results,
        health_report,
    })
}

fn prepare_context(serial: &str) -> Result<PreparedContext, String> {
    let devices = adb::scan_devices()?;
    let device = devices
        .into_iter()
        .find(|item| item.serial == serial)
        .ok_or_else(|| format!("未找到序列号为 {} 的设备", serial))?;

    if device.state != "device" {
        return Err(format!("设备 {} 当前状态不是 device", serial));
    }

    let packages = adb::collect_package_inventory(serial)?;
    let runtime_profile = adb::collect_runtime_profile(serial);
    let analysis = analyzer::analyze_device(&device, &packages, &runtime_profile);
    let (verification_targets, warnings) = resolve_verification_targets(serial, &analysis)?;

    Ok(PreparedContext {
        analysis,
        verification_targets,
        warnings,
    })
}

fn resolve_verification_targets(
    serial: &str,
    analysis: &DeviceAnalysis,
) -> Result<(VerificationTargets, Vec<String>), String> {
    let inventory_names: HashSet<&str> = analysis
        .packages
        .iter()
        .map(|item| item.package_name.as_str())
        .collect();

    let home_package = adb::resolve_home_package(serial)?;
    if home_package.is_empty() {
        return Err("无法解析当前 HOME 桌面包，拒绝生成执行计划".into());
    }

    let settings_package = pick_verification_target(
        analysis,
        &inventory_names,
        SETTINGS_CANDIDATES,
        &["settings"],
        true,
    )
    .ok_or_else(|| "当前设备未识别到 Settings 核心包，拒绝生成执行计划".to_string())?;
    let system_ui_package = pick_verification_target(
        analysis,
        &inventory_names,
        SYSTEM_UI_CANDIDATES,
        &["systemui"],
        false,
    )
    .ok_or_else(|| "当前设备未识别到 SystemUI 核心包，拒绝生成执行计划".to_string())?;

    let mut warnings = Vec::new();
    let phone_package = pick_first_present(&inventory_names, PHONE_CANDIDATES);
    if phone_package.is_none() {
        warnings.push("没有识别到标准电话组件，执行时将跳过该项验活".into());
    }

    let camera_package = pick_first_present(&inventory_names, CAMERA_CANDIDATES);
    if camera_package.is_none() {
        warnings.push("没有识别到标准相机组件，执行时将跳过该项验活".into());
    }

    Ok((
        VerificationTargets {
            home_package,
            settings_package: Some(settings_package),
            system_ui_package: Some(system_ui_package),
            phone_package,
            camera_package,
        },
        warnings,
    ))
}

fn build_cleanup_plan(
    analysis: &DeviceAnalysis,
    verification_targets: VerificationTargets,
    warnings: Vec<String>,
) -> CleanupPlan {
    let protected = protected_packages(analysis, &verification_targets);
    let mut dropped_packages = Vec::new();

    let packages: Vec<CleanupPlanEntry> = analysis
        .packages
        .iter()
        .filter_map(|item| {
            if item.recommended_action != RecommendedAction::UninstallUser0 {
                return None;
            }

            if protected.contains(item.package_name.as_str()) {
                dropped_packages.push(item.package_name.clone());
                return None;
            }

            Some(CleanupPlanEntry {
                package_name: item.package_name.clone(),
                install_path: item.install_path.clone(),
                visible_on_launcher: item.visible_on_launcher,
                reasons: item.reasons.clone(),
            })
        })
        .collect();

    let mut merged_warnings = warnings;
    if !dropped_packages.is_empty() {
        merged_warnings.push(format!(
            "为保证验活链路稳定，已自动排除 {} 个关键包",
            dropped_packages.len()
        ));
    }

    CleanupPlan {
        serial: analysis.device.serial.clone(),
        vendor_family: analysis.vendor_family.clone(),
        generated_at_ms: now_ms(),
        packages,
        verification_targets,
        warnings: merged_warnings,
    }
}

fn protected_packages(
    analysis: &DeviceAnalysis,
    verification_targets: &VerificationTargets,
) -> HashSet<String> {
    let mut protected: HashSet<&str> = analysis
        .packages
        .iter()
        .filter(|item| item.recommended_action == RecommendedAction::Keep)
        .map(|item| item.package_name.as_str())
        .collect();

    protected.insert(verification_targets.home_package.as_str());

    if let Some(settings_package) = verification_targets.settings_package.as_deref() {
        protected.insert(settings_package);
    }

    if let Some(system_ui_package) = verification_targets.system_ui_package.as_deref() {
        protected.insert(system_ui_package);
    }

    if let Some(phone_package) = verification_targets.phone_package.as_deref() {
        protected.insert(phone_package);
    }

    if let Some(camera_package) = verification_targets.camera_package.as_deref() {
        protected.insert(camera_package);
    }

    protected.into_iter().map(str::to_string).collect()
}

fn verify_device_health(
    serial: &str,
    verification_targets: &VerificationTargets,
) -> DeviceHealthReport {
    let mut checks = Vec::new();

    checks.push(device_connection_check(serial));

    let home_result = adb::resolve_home_package(serial);
    match home_result {
        Ok(package_name) if package_name == verification_targets.home_package => {
            checks.push(HealthCheck {
                key: "home".into(),
                label: "HOME 解析".into(),
                status: CheckStatus::Passed,
                detail: format!("当前桌面仍解析到 {}", package_name),
            })
        }
        Ok(package_name) if !package_name.is_empty() => checks.push(HealthCheck {
            key: "home".into(),
            label: "HOME 解析".into(),
            status: CheckStatus::Warning,
            detail: format!(
                "桌面可正常解析，但当前包从 {} 变成了 {}",
                verification_targets.home_package, package_name
            ),
        }),
        Ok(_) => checks.push(HealthCheck {
            key: "home".into(),
            label: "HOME 解析".into(),
            status: CheckStatus::Failed,
            detail: "没有解析到可用桌面".into(),
        }),
        Err(error) => checks.push(HealthCheck {
            key: "home".into(),
            label: "HOME 解析".into(),
            status: CheckStatus::Failed,
            detail: error,
        }),
    }

    checks.push(package_check(
        serial,
        "settings",
        "Settings",
        verification_targets.settings_package.as_deref(),
        true,
    ));
    checks.push(package_check(
        serial,
        "systemui",
        "SystemUI",
        verification_targets.system_ui_package.as_deref(),
        true,
    ));
    checks.push(package_check(
        serial,
        "phone",
        "电话组件",
        verification_targets.phone_package.as_deref(),
        false,
    ));
    checks.push(package_check(
        serial,
        "camera",
        "相机组件",
        verification_targets.camera_package.as_deref(),
        false,
    ));

    let passed = checks.iter().all(|item| item.status != CheckStatus::Failed);

    DeviceHealthReport {
        checked_at_ms: now_ms(),
        passed,
        checks,
    }
}

fn device_connection_check(serial: &str) -> HealthCheck {
    match adb::is_device_ready(serial) {
        Ok(true) => HealthCheck {
            key: "adb".into(),
            label: "ADB 连接".into(),
            status: CheckStatus::Passed,
            detail: "设备仍处于 device 状态".into(),
        },
        Ok(false) => HealthCheck {
            key: "adb".into(),
            label: "ADB 连接".into(),
            status: CheckStatus::Failed,
            detail: "设备已经掉线或状态异常".into(),
        },
        Err(error) => HealthCheck {
            key: "adb".into(),
            label: "ADB 连接".into(),
            status: CheckStatus::Failed,
            detail: error,
        },
    }
}

fn package_check(
    serial: &str,
    key: &str,
    label: &str,
    package_name: Option<&str>,
    required: bool,
) -> HealthCheck {
    let Some(package_name) = package_name else {
        return HealthCheck {
            key: key.to_string(),
            label: label.to_string(),
            status: if required {
                CheckStatus::Failed
            } else {
                CheckStatus::Warning
            },
            detail: if required {
                format!("没有可用于验活的 {} 目标包", label)
            } else {
                format!("未识别到 {} 候选包，已跳过", label)
            },
        };
    };

    match adb::is_package_available_for_user_zero(serial, package_name) {
        Ok(true) => HealthCheck {
            key: key.to_string(),
            label: label.to_string(),
            status: CheckStatus::Passed,
            detail: format!("{} 仍然可用: {}", label, package_name),
        },
        Ok(false) => HealthCheck {
            key: key.to_string(),
            label: label.to_string(),
            status: CheckStatus::Failed,
            detail: format!("{} 已不可用: {}", label, package_name),
        },
        Err(error) => HealthCheck {
            key: key.to_string(),
            label: label.to_string(),
            status: CheckStatus::Failed,
            detail: error,
        },
    }
}

fn pick_first_present(inventory_names: &HashSet<&str>, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| inventory_names.contains(**candidate))
        .map(|candidate| (*candidate).to_string())
}

fn pick_verification_target(
    analysis: &DeviceAnalysis,
    inventory_names: &HashSet<&str>,
    candidates: &[&str],
    keywords: &[&str],
    require_launcher_entry: bool,
) -> Option<String> {
    pick_first_present(inventory_names, candidates).or_else(|| {
        analysis
            .packages
            .iter()
            .filter(|item| item.source == PackageSource::System)
            .filter(|item| !require_launcher_entry || item.visible_on_launcher)
            .find(|item| {
                keywords.iter().any(|keyword| {
                    package_matches_keyword(
                        item.package_name.as_str(),
                        item.install_path.as_deref(),
                        keyword,
                    )
                })
            })
            .map(|item| item.package_name.clone())
    })
}

fn package_matches_keyword(package_name: &str, install_path: Option<&str>, keyword: &str) -> bool {
    package_name.contains(keyword)
        || install_path
            .map(|path| path.to_ascii_lowercase().contains(keyword))
            .unwrap_or(false)
}

fn summarize_health_failures(report: &DeviceHealthReport) -> String {
    report
        .checks
        .iter()
        .find(|item| item.status == CheckStatus::Failed)
        .map(|item| format!("{}：{}", item.label, item.detail))
        .unwrap_or_else(|| "未知验活失败".into())
}

fn persist_rollback_record(app: &AppHandle, record: &RollbackRecord) -> Result<(), String> {
    let path = rollback_record_path(app, &record.serial)?;
    persist_record_to_path(&path, record)
}

fn persist_record_to_path(path: &Path, record: &RollbackRecord) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建回滚目录失败: {error}"))?;
    }

    let payload = serde_json::to_string_pretty(record)
        .map_err(|error| format!("序列化回滚记录失败: {error}"))?;
    fs::write(path, payload).map_err(|error| format!("写入回滚记录失败: {error}"))?;
    Ok(())
}

fn load_rollback_record(path: &Path) -> Result<RollbackRecord, String> {
    let payload = fs::read_to_string(path).map_err(|error| format!("读取回滚记录失败: {error}"))?;
    serde_json::from_str(&payload).map_err(|error| format!("解析回滚记录失败: {error}"))
}

fn rollback_record_path(app: &AppHandle, serial: &str) -> Result<PathBuf, String> {
    let mut base = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    base.push("rollback");
    base.push(format!("{}.json", sanitize_serial(serial)));
    Ok(base)
}

fn sanitize_serial(serial: &str) -> String {
    serial
        .chars()
        .map(|item| {
            if item.is_ascii_alphanumeric() || item == '-' || item == '_' {
                item
            } else {
                '_'
            }
        })
        .collect()
}

fn normalize_detail(detail: String, fallback: &str) -> String {
    let trimmed = detail.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        build_cleanup_plan, persist_record_to_path, pick_verification_target, sanitize_serial,
        CleanupPlan, DeviceHealthReport, HealthCheck, PackageOperationResult,
        PackageOperationStatus, VerificationTargets,
    };
    use crate::adb::{AndroidDevice, PackageSource};
    use crate::analyzer::{DeviceAnalysis, PackageAssessment, RecommendedAction, RiskLevel};
    use std::fs;

    fn analysis_fixture() -> DeviceAnalysis {
        DeviceAnalysis {
            device: AndroidDevice {
                serial: "SER:IAL/01".into(),
                state: "device".into(),
                product: Some("test".into()),
                model: Some("test".into()),
                device_code: Some("test".into()),
                transport_id: Some("1".into()),
                brand: Some("HONOR".into()),
                manufacturer: Some("HUAWEI".into()),
                android_version: Some("10".into()),
                sdk: Some("29".into()),
                build_display_id: Some("build".into()),
                fingerprint: Some("fingerprint".into()),
            },
            vendor_family: "huawei".into(),
            summary: Default::default(),
            packages: vec![
                PackageAssessment {
                    package_name: "com.huawei.appmarket".into(),
                    install_path: Some("/system/app/HwAppMarket/HwAppMarket.apk".into()),
                    source: PackageSource::System,
                    visible_on_launcher: true,
                    risk_level: RiskLevel::SafeRemove,
                    recommended_action: RecommendedAction::UninstallUser0,
                    reasons: vec!["命中厂商可见预装清理规则".into()],
                },
                PackageAssessment {
                    package_name: "com.android.settings".into(),
                    install_path: Some("/system/priv-app/Settings/Settings.apk".into()),
                    source: PackageSource::System,
                    visible_on_launcher: true,
                    risk_level: RiskLevel::CoreKeep,
                    recommended_action: RecommendedAction::Keep,
                    reasons: vec!["命中核心系统保留规则".into()],
                },
                PackageAssessment {
                    package_name: "com.android.systemui".into(),
                    install_path: Some("/system/priv-app/SystemUI/SystemUI.apk".into()),
                    source: PackageSource::System,
                    visible_on_launcher: false,
                    risk_level: RiskLevel::CoreKeep,
                    recommended_action: RecommendedAction::Keep,
                    reasons: vec!["命中核心系统保留规则".into()],
                },
                PackageAssessment {
                    package_name: "com.huawei.camera".into(),
                    install_path: Some("/system/app/HwCamera/HwCamera.apk".into()),
                    source: PackageSource::System,
                    visible_on_launcher: true,
                    risk_level: RiskLevel::CoreKeep,
                    recommended_action: RecommendedAction::Keep,
                    reasons: vec!["命中核心系统保留规则".into()],
                },
            ],
        }
    }

    fn verification_targets() -> VerificationTargets {
        VerificationTargets {
            home_package: "com.huawei.android.launcher".into(),
            settings_package: Some("com.android.settings".into()),
            system_ui_package: Some("com.android.systemui".into()),
            phone_package: Some("com.android.phone".into()),
            camera_package: Some("com.huawei.camera".into()),
        }
    }

    fn xiaomi_analysis_fixture() -> DeviceAnalysis {
        DeviceAnalysis {
            device: AndroidDevice {
                serial: "XIAOMI".into(),
                state: "device".into(),
                product: Some("houji".into()),
                model: Some("23127PN0CC".into()),
                device_code: Some("houji".into()),
                transport_id: Some("1".into()),
                brand: Some("Xiaomi".into()),
                manufacturer: Some("Xiaomi".into()),
                android_version: Some("16".into()),
                sdk: Some("36".into()),
                build_display_id: Some("OS3.0.7.0".into()),
                fingerprint: Some("Xiaomi/houji/houji".into()),
            },
            vendor_family: "xiaomi".into(),
            summary: Default::default(),
            packages: vec![
                PackageAssessment {
                    package_name: "com.xiaomi.misettings".into(),
                    install_path: Some("/product/app/MiSettings/MiSettings.apk".into()),
                    source: PackageSource::System,
                    visible_on_launcher: true,
                    risk_level: RiskLevel::CoreKeep,
                    recommended_action: RecommendedAction::Keep,
                    reasons: vec!["命中核心系统保留规则".into()],
                },
                PackageAssessment {
                    package_name: "com.android.systemui".into(),
                    install_path: Some("/system_ext/priv-app/SystemUI/SystemUI.apk".into()),
                    source: PackageSource::System,
                    visible_on_launcher: false,
                    risk_level: RiskLevel::CoreKeep,
                    recommended_action: RecommendedAction::Keep,
                    reasons: vec!["命中核心系统保留规则".into()],
                },
            ],
        }
    }

    #[test]
    fn keeps_only_uninstallable_packages_in_cleanup_plan() {
        let plan = build_cleanup_plan(&analysis_fixture(), verification_targets(), vec![]);

        assert_eq!(plan.packages.len(), 1);
        assert_eq!(plan.packages[0].package_name, "com.huawei.appmarket");
    }

    #[test]
    fn sanitize_serial_replaces_unsupported_characters() {
        assert_eq!(sanitize_serial("SER:IAL/01"), "SER_IAL_01");
    }

    #[test]
    fn persists_rollback_record_as_json() {
        let dir = std::env::temp_dir().join(format!("debloat-test-{}", std::process::id()));
        let path = dir.join("rollback.json");

        let record = super::RollbackRecord {
            serial: "SERIAL".into(),
            plan: CleanupPlan {
                serial: "SERIAL".into(),
                vendor_family: "huawei".into(),
                generated_at_ms: 1,
                packages: vec![],
                verification_targets: verification_targets(),
                warnings: vec![],
            },
            executed_at_ms: 2,
            removed_packages: vec!["com.huawei.appmarket".into()],
            health_report: DeviceHealthReport {
                checked_at_ms: 3,
                passed: true,
                checks: vec![HealthCheck {
                    key: "adb".into(),
                    label: "ADB 连接".into(),
                    status: super::CheckStatus::Passed,
                    detail: "ok".into(),
                }],
            },
        };

        persist_record_to_path(&path, &record).expect("should persist rollback record");

        let payload = fs::read_to_string(&path).expect("should read record");
        assert!(payload.contains("com.huawei.appmarket"));

        if dir.exists() {
            fs::remove_dir_all(dir).expect("should cleanup temp dir");
        }
    }

    #[test]
    fn package_operation_status_serializes() {
        let result = PackageOperationResult {
            package_name: "pkg".into(),
            status: PackageOperationStatus::Removed,
            detail: "ok".into(),
        };

        let payload = serde_json::to_string(&result).expect("serialize package result");
        assert!(payload.contains("\"removed\""));
    }

    #[test]
    fn picks_oem_settings_package_from_analysis() {
        let analysis = xiaomi_analysis_fixture();
        let inventory_names: std::collections::HashSet<&str> = analysis
            .packages
            .iter()
            .map(|item| item.package_name.as_str())
            .collect();

        let settings_package = pick_verification_target(
            &analysis,
            &inventory_names,
            super::SETTINGS_CANDIDATES,
            &["settings"],
            true,
        );

        assert_eq!(settings_package.as_deref(), Some("com.xiaomi.misettings"));
    }
}
