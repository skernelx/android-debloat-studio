use crate::adb::{AndroidDevice, DeviceRuntimeProfile, InstalledPackage, PackageSource, Partition};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

const MINIMAL_CORE_EXACT: &[&str] = &[
    "android",
    "com.android.settings",
    "com.android.systemui",
    "com.xiaomi.misettings",
    "com.android.documentsui",
    "com.android.packageinstaller",
    "com.google.android.packageinstaller",
    "com.miui.packageinstaller",
    "com.android.permissioncontroller",
    "com.google.android.permissioncontroller",
    "com.android.shell",
    "com.android.keychain",
    "com.android.certinstaller",
    "com.android.defcontainer",
    "com.android.provision",
    "com.android.externalstorage",
    "com.android.webview",
    "com.google.android.webview",
    "com.google.android.ext.services",
    "com.google.android.ext.shared",
    "com.huawei.webview",
    "miui.systemui.plugin",
];

const MINIMAL_CORE_PREFIXES: &[&str] = &[
    "com.android.providers.",
    "com.android.externalstorage",
    "com.android.bluetooth.",
    "com.android.server.telecom",
    "com.android.networkstack",
    "com.android.wifi.",
    "com.android.connectivity",
    "com.android.ims.",
    "com.google.android.networkstack",
    "com.google.android.wifi",
    "com.google.android.connectivity",
    "com.google.android.permissioncontroller",
    "com.google.android.packageinstaller",
    "com.samsung.android.networkstack",
    "com.samsung.android.ConnectivityOverlay",
    "com.samsung.android.ConnectivityUxOverlay",
    "com.vivo.android.connectivity",
    "com.vivo.android.wifi",
];

const MINIMAL_CORE_KEYWORDS: &[&str] = &[
    "packageinstaller",
    "permissioncontroller",
    "webview",
    "externalstorage",
    "networkstack",
    "inputmethod",
    "keyboard",
    "documentsui",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    CoreKeep,
    SafeRemove,
    UserInstalled,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RecommendedAction {
    Keep,
    UninstallUser0,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum CleanupMode {
    #[default]
    Balanced,
    MinimalCore,
}

impl CleanupMode {
    pub fn label(self) -> &'static str {
        match self {
            CleanupMode::Balanced => "平衡模式",
            CleanupMode::MinimalCore => "极限精简",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageAssessment {
    pub package_name: String,
    pub install_path: Option<String>,
    pub source: PackageSource,
    pub visible_on_launcher: bool,
    pub risk_level: RiskLevel,
    pub recommended_action: RecommendedAction,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSummary {
    pub total_packages: usize,
    pub system_packages: usize,
    pub user_packages: usize,
    pub visible_packages: usize,
    pub core_keep: usize,
    pub safe_remove: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAnalysis {
    pub mode: CleanupMode,
    pub device: AndroidDevice,
    pub vendor_family: String,
    pub summary: AnalysisSummary,
    pub packages: Vec<PackageAssessment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleDatabase {
    generic: VendorRulePack,
    vendors: std::collections::HashMap<String, VendorRulePack>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct VendorRulePack {
    protected_exact: Vec<String>,
    protected_prefixes: Vec<String>,
    protected_keywords: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct EffectiveRules {
    protected_exact: HashSet<String>,
    protected_prefixes: Vec<String>,
    protected_keywords: Vec<String>,
}

const SHARED_UID_KEEP_PREFIXES: &[&str] = &[
    "android.uid.system",
    "android.uid.nfc",
    "android.uid.bluetooth",
    "android.uid.networkstack",
    "android.uid.se",
];

const BALANCED_ONLY_SHARED_UID_KEEP_PREFIXES: &[&str] = &["android.uid.phone"];

pub fn analyze_device(
    device: &AndroidDevice,
    packages: &[InstalledPackage],
    runtime_profile: &DeviceRuntimeProfile,
    mode: CleanupMode,
) -> DeviceAnalysis {
    let vendor_family = vendor_family(device);
    let rules = effective_rules(&vendor_family);

    let assessments: Vec<PackageAssessment> = packages
        .iter()
        .map(|package| assess_package(package, runtime_profile, &rules, mode))
        .collect();

    let mut summary = AnalysisSummary {
        total_packages: assessments.len(),
        system_packages: assessments
            .iter()
            .filter(|package| package.source == PackageSource::System)
            .count(),
        user_packages: assessments
            .iter()
            .filter(|package| package.source == PackageSource::User)
            .count(),
        visible_packages: assessments
            .iter()
            .filter(|package| package.visible_on_launcher)
            .count(),
        ..AnalysisSummary::default()
    };

    for package in &assessments {
        match package.risk_level {
            RiskLevel::CoreKeep => summary.core_keep += 1,
            RiskLevel::SafeRemove => summary.safe_remove += 1,
            RiskLevel::UserInstalled => {}
        }
    }

    DeviceAnalysis {
        mode,
        device: device.clone(),
        vendor_family,
        summary,
        packages: assessments,
    }
}

fn assess_package(
    package: &InstalledPackage,
    runtime_profile: &DeviceRuntimeProfile,
    rules: &EffectiveRules,
    mode: CleanupMode,
) -> PackageAssessment {
    if package.source == PackageSource::User && !package.is_updated_system_app {
        return PackageAssessment {
            package_name: package.package_name.clone(),
            install_path: package.install_path.clone(),
            source: package.source,
            visible_on_launcher: package.visible_on_launcher,
            risk_level: RiskLevel::UserInstalled,
            recommended_action: RecommendedAction::Keep,
            reasons: vec!["这是用户安装应用，默认不纳入预装精简范围".into()],
        };
    }

    let mut reasons = Vec::new();

    if let Some(reason) = runtime_protection_reason(package, runtime_profile, mode) {
        reasons.push(reason);
        return keep_assessment(package, reasons);
    }

    if is_hard_keep(package, rules, mode, &mut reasons) {
        return keep_assessment(package, reasons);
    }

    if reasons.is_empty() {
        reasons.push("未命中任何保护规则，标记为可安全清理".into());
    }
    safe_remove_assessment(package, reasons)
}

fn runtime_protection_reason(
    package: &InstalledPackage,
    runtime_profile: &DeviceRuntimeProfile,
    mode: CleanupMode,
) -> Option<String> {
    if runtime_profile
        .home_package
        .as_deref()
        .is_some_and(|home_package| home_package == package.package_name)
        || package.visible_as_home
    {
        return Some("当前设备的桌面 HOME 角色需要保留".into());
    }

    if matches!(mode, CleanupMode::Balanced)
        && runtime_profile
            .dialer_role_holders
            .contains(&package.package_name)
    {
        return Some("当前设备的默认电话角色需要保留".into());
    }

    if matches!(mode, CleanupMode::Balanced)
        && runtime_profile
            .sms_role_holders
            .contains(&package.package_name)
    {
        return Some("当前设备的默认短信角色需要保留".into());
    }

    None
}

fn is_hard_keep(
    package: &InstalledPackage,
    rules: &EffectiveRules,
    mode: CleanupMode,
    reasons: &mut Vec<String>,
) -> bool {
    if package.partition == Partition::Apex {
        reasons.push("位于 APEX 模块，属于系统核心组件".into());
        return true;
    }

    if package.is_overlay {
        reasons.push("属于 overlay/RRO 资源覆盖包，默认保留".into());
        return true;
    }

    if package.is_persistent {
        reasons.push("属于系统 persistent 常驻组件，默认保留".into());
        return true;
    }

    if package
        .shared_user_id
        .as_deref()
        .is_some_and(|shared_user_id| is_shared_uid_keep(shared_user_id, mode))
    {
        reasons.push("使用系统级 shared UID，默认保留".into());
        return true;
    }

    if package.package_name == "android" || package.package_name.starts_with("android.") {
        reasons.push("AOSP 核心命名空间包，默认保留".into());
        return true;
    }

    match mode {
        CleanupMode::Balanced => {
            if rules.protected_exact.contains(&package.package_name)
                || matches_any_prefix(&package.package_name, &rules.protected_prefixes)
                || contains_any(&package.package_name, &rules.protected_keywords)
            {
                reasons.push("命中平衡模式核心保留规则".into());
                return true;
            }
        }
        CleanupMode::MinimalCore => {
            if is_minimal_core_package(package) {
                reasons.push("命中极限模式系统骨架保留规则".into());
                return true;
            }
        }
    }

    false
}

fn keep_assessment(package: &InstalledPackage, reasons: Vec<String>) -> PackageAssessment {
    PackageAssessment {
        package_name: package.package_name.clone(),
        install_path: package.install_path.clone(),
        source: package.source,
        visible_on_launcher: package.visible_on_launcher,
        risk_level: RiskLevel::CoreKeep,
        recommended_action: RecommendedAction::Keep,
        reasons,
    }
}

fn safe_remove_assessment(package: &InstalledPackage, reasons: Vec<String>) -> PackageAssessment {
    PackageAssessment {
        package_name: package.package_name.clone(),
        install_path: package.install_path.clone(),
        source: package.source,
        visible_on_launcher: package.visible_on_launcher,
        risk_level: RiskLevel::SafeRemove,
        recommended_action: RecommendedAction::UninstallUser0,
        reasons,
    }
}

fn is_shared_uid_keep(shared_user_id: &str, mode: CleanupMode) -> bool {
    SHARED_UID_KEEP_PREFIXES
        .iter()
        .any(|prefix| shared_user_id.starts_with(prefix))
        || (matches!(mode, CleanupMode::Balanced)
            && BALANCED_ONLY_SHARED_UID_KEEP_PREFIXES
                .iter()
                .any(|prefix| shared_user_id.starts_with(prefix)))
}

fn is_minimal_core_package(package: &InstalledPackage) -> bool {
    MINIMAL_CORE_EXACT.contains(&package.package_name.as_str())
        || matches_any_prefix(&package.package_name, MINIMAL_CORE_PREFIXES)
        || contains_any_literal(&package.package_name, MINIMAL_CORE_KEYWORDS)
        || contains_any_literal(package.install_path.as_deref().unwrap_or_default(), MINIMAL_CORE_KEYWORDS)
}

fn contains_any(package_name: &str, keywords: &[String]) -> bool {
    keywords
        .iter()
        .any(|keyword| package_name.contains(keyword))
}

fn contains_any_literal(value: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| value.contains(keyword))
}

fn matches_any_prefix<T: AsRef<str>>(value: &str, prefixes: &[T]) -> bool {
    prefixes
        .iter()
        .any(|prefix| value.starts_with(prefix.as_ref()))
}

fn effective_rules(vendor_family: &str) -> EffectiveRules {
    let database = rule_database();
    let mut merged = EffectiveRules::default();
    merge_rule_pack(&mut merged, &database.generic);

    if let Some(vendor_pack) = database.vendors.get(vendor_family) {
        merge_rule_pack(&mut merged, vendor_pack);
    }

    merged
}

fn merge_rule_pack(target: &mut EffectiveRules, source: &VendorRulePack) {
    target
        .protected_exact
        .extend(source.protected_exact.iter().cloned());
    target
        .protected_prefixes
        .extend(source.protected_prefixes.iter().cloned());
    target
        .protected_keywords
        .extend(source.protected_keywords.iter().cloned());
}

fn rule_database() -> &'static RuleDatabase {
    static DATABASE: OnceLock<RuleDatabase> = OnceLock::new();

    DATABASE.get_or_init(|| {
        serde_json::from_str(include_str!("../rules/vendor_rules.json"))
            .expect("vendor rule database should be valid json")
    })
}

fn vendor_family(device: &AndroidDevice) -> String {
    let joined = [
        device.brand.as_deref().unwrap_or_default(),
        device.manufacturer.as_deref().unwrap_or_default(),
        device.fingerprint.as_deref().unwrap_or_default(),
    ]
    .join(" ")
    .to_lowercase();

    if joined.contains("huawei") || joined.contains("honor") || joined.contains("magicui") {
        "huawei".into()
    } else if joined.contains("xiaomi") || joined.contains("miui") || joined.contains("hyperos") {
        "xiaomi".into()
    } else if joined.contains("samsung") || joined.contains("oneui") {
        "samsung".into()
    } else if joined.contains("oppo") || joined.contains("realme") || joined.contains("coloros") {
        "coloros".into()
    } else if joined.contains("vivo") || joined.contains("iqoo") || joined.contains("funtouch") {
        "vivo".into()
    } else {
        "generic".into()
    }
}

#[cfg(test)]
mod tests {
    use super::{analyze_device, CleanupMode, DeviceAnalysis, RecommendedAction, RiskLevel};
    use crate::adb::{
        AndroidDevice, DeviceRuntimeProfile, InstalledPackage, PackageSource, Partition,
    };

    fn huawei_device() -> AndroidDevice {
        AndroidDevice {
            serial: "serial".into(),
            state: "device".into(),
            product: Some("COR-AL10".into()),
            model: Some("COR_AL10".into()),
            device_code: Some("HWCOR".into()),
            transport_id: Some("1".into()),
            brand: Some("HONOR".into()),
            manufacturer: Some("HUAWEI".into()),
            android_version: Some("9".into()),
            sdk: Some("28".into()),
            build_display_id: Some("COR-AL10 9.1.0.346".into()),
            fingerprint: Some("HUAWEI/COR".into()),
        }
    }

    fn xiaomi_device() -> AndroidDevice {
        AndroidDevice {
            serial: "serial".into(),
            state: "device".into(),
            product: Some("houji".into()),
            model: Some("23127PN0CC".into()),
            device_code: Some("houji".into()),
            transport_id: Some("2".into()),
            brand: Some("Xiaomi".into()),
            manufacturer: Some("Xiaomi".into()),
            android_version: Some("16".into()),
            sdk: Some("36".into()),
            build_display_id: Some("OS3.0.7.0".into()),
            fingerprint: Some("Xiaomi/houji/houji".into()),
        }
    }

    fn base_package(package_name: &str) -> InstalledPackage {
        InstalledPackage {
            package_name: package_name.into(),
            install_path: Some(format!("/product/app/{package_name}/{package_name}.apk")),
            source: PackageSource::System,
            visible_on_launcher: true,
            visible_as_home: false,
            partition: Partition::Product,
            is_privileged: false,
            is_overlay: false,
            is_apex: false,
            has_code: true,
            is_persistent: false,
            is_updated_system_app: false,
            shared_user_id: None,
        }
    }

    #[test]
    fn classifies_huawei_appmarket_as_safe_remove() {
        let analysis = analyze_device(
            &huawei_device(),
            &[base_package("com.huawei.appmarket")],
            &DeviceRuntimeProfile::default(),
            CleanupMode::Balanced,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::SafeRemove);
        assert_eq!(
            package.recommended_action,
            RecommendedAction::UninstallUser0
        );
    }

    #[test]
    fn protects_current_home_role_holder() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[InstalledPackage {
                package_name: "com.miui.home".into(),
                visible_as_home: true,
                ..base_package("com.miui.home")
            }],
            &DeviceRuntimeProfile {
                home_package: Some("com.miui.home".into()),
                ..DeviceRuntimeProfile::default()
            },
            CleanupMode::Balanced,
        );

        assert_eq!(analysis.packages[0].risk_level, RiskLevel::CoreKeep);
    }

    #[test]
    fn classifies_xiaomi_weather_as_safe_remove() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[base_package("com.miui.weather2")],
            &DeviceRuntimeProfile::default(),
            CleanupMode::Balanced,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::SafeRemove);
        assert_eq!(
            package.recommended_action,
            RecommendedAction::UninstallUser0
        );
    }

    #[test]
    fn keeps_overlay_package() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[InstalledPackage {
                package_name: "android.miui.overlay".into(),
                install_path: Some("/product/overlay/Foo.apk".into()),
                source: PackageSource::System,
                visible_on_launcher: false,
                visible_as_home: false,
                partition: Partition::Product,
                is_privileged: false,
                is_overlay: true,
                is_apex: false,
                has_code: false,
                is_persistent: false,
                is_updated_system_app: false,
                shared_user_id: None,
            }],
            &DeviceRuntimeProfile::default(),
            CleanupMode::Balanced,
        );

        assert_eq!(analysis.packages[0].risk_level, RiskLevel::CoreKeep);
    }

    #[test]
    fn keeps_persistent_system_package() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[InstalledPackage {
                package_name: "com.example.persistent".into(),
                is_persistent: true,
                ..base_package("com.example.persistent")
            }],
            &DeviceRuntimeProfile::default(),
            CleanupMode::Balanced,
        );

        assert_eq!(analysis.packages[0].risk_level, RiskLevel::CoreKeep);
        assert_eq!(
            analysis.packages[0].recommended_action,
            RecommendedAction::Keep
        );
    }

    #[test]
    fn user_installed_app_stays_outside_preinstall_scope() {
        let analysis = analyze_device(
            &huawei_device(),
            &[InstalledPackage {
                package_name: "com.ss.android.ugc.aweme".into(),
                install_path: Some("/data/app/aweme/base.apk".into()),
                source: PackageSource::User,
                visible_on_launcher: true,
                visible_as_home: false,
                partition: Partition::Data,
                is_privileged: false,
                is_overlay: false,
                is_apex: false,
                has_code: true,
                is_persistent: false,
                is_updated_system_app: false,
                shared_user_id: None,
            }],
            &DeviceRuntimeProfile::default(),
            CleanupMode::Balanced,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::UserInstalled);
        assert_eq!(package.recommended_action, RecommendedAction::Keep);
    }

    #[test]
    fn vendor_family_detects_xiaomi_hyperos() {
        let analysis: DeviceAnalysis = analyze_device(
            &xiaomi_device(),
            &[base_package("com.miui.video")],
            &DeviceRuntimeProfile::default(),
            CleanupMode::Balanced,
        );

        assert_eq!(analysis.vendor_family, "xiaomi");
    }

    #[test]
    fn non_protected_system_package_is_safe_remove() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[base_package("com.miui.daemon")],
            &DeviceRuntimeProfile::default(),
            CleanupMode::Balanced,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::SafeRemove);
        assert_eq!(
            package.recommended_action,
            RecommendedAction::UninstallUser0
        );
    }

    #[test]
    fn privileged_non_protected_package_is_safe_remove() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[InstalledPackage {
                is_privileged: true,
                install_path: Some("/system/priv-app/SomeApp/SomeApp.apk".into()),
                ..base_package("com.miui.someapp")
            }],
            &DeviceRuntimeProfile::default(),
            CleanupMode::Balanced,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::SafeRemove);
    }

    #[test]
    fn minimal_core_mode_allows_removing_default_dialer_shell() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[base_package("com.android.contacts")],
            &DeviceRuntimeProfile {
                dialer_role_holders: vec!["com.android.contacts".into()],
                ..DeviceRuntimeProfile::default()
            },
            CleanupMode::MinimalCore,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::SafeRemove);
        assert_eq!(
            package.recommended_action,
            RecommendedAction::UninstallUser0
        );
    }

    #[test]
    fn minimal_core_mode_keeps_package_installer() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[base_package("com.miui.packageinstaller")],
            &DeviceRuntimeProfile::default(),
            CleanupMode::MinimalCore,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::CoreKeep);
        assert_eq!(package.recommended_action, RecommendedAction::Keep);
    }

    #[test]
    fn minimal_core_mode_allows_removing_vendor_security_center() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[base_package("com.miui.securitycenter")],
            &DeviceRuntimeProfile::default(),
            CleanupMode::MinimalCore,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::SafeRemove);
        assert_eq!(
            package.recommended_action,
            RecommendedAction::UninstallUser0
        );
    }

    #[test]
    fn balanced_mode_keeps_android_uid_phone_packages() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[InstalledPackage {
                shared_user_id: Some("android.uid.phone".into()),
                ..base_package("com.android.contacts")
            }],
            &DeviceRuntimeProfile::default(),
            CleanupMode::Balanced,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::CoreKeep);
        assert_eq!(package.recommended_action, RecommendedAction::Keep);
    }

    #[test]
    fn minimal_core_mode_allows_android_uid_phone_packages() {
        let analysis = analyze_device(
            &xiaomi_device(),
            &[InstalledPackage {
                shared_user_id: Some("android.uid.phone".into()),
                ..base_package("com.android.contacts")
            }],
            &DeviceRuntimeProfile::default(),
            CleanupMode::MinimalCore,
        );

        let package = &analysis.packages[0];
        assert_eq!(package.risk_level, RiskLevel::SafeRemove);
        assert_eq!(
            package.recommended_action,
            RecommendedAction::UninstallUser0
        );
    }
}
