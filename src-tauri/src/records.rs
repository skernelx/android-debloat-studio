use crate::analyzer::CleanupMode;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const MAX_HISTORY_ENTRIES: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    Cleanup,
    Restore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationHistoryEntry {
    pub id: String,
    pub kind: OperationKind,
    #[serde(default)]
    pub mode: CleanupMode,
    pub serial: String,
    pub vendor_family: String,
    pub timestamp_ms: u64,
    pub package_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
    pub aborted: bool,
    pub health_passed: bool,
    pub summary: String,
}

pub fn append_history_entry(app: &AppHandle, entry: OperationHistoryEntry) -> Result<(), String> {
    let path = history_path(app)?;
    let mut history = if path.exists() {
        read_json_file::<Vec<OperationHistoryEntry>>(&path)?
    } else {
        Vec::new()
    };

    history.push(entry);
    history.sort_by(|left, right| right.timestamp_ms.cmp(&left.timestamp_ms));
    history.truncate(MAX_HISTORY_ENTRIES);
    write_json_file(&path, &history)
}

pub fn list_operation_history(app: &AppHandle) -> Result<Vec<OperationHistoryEntry>, String> {
    let path = history_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut history = read_json_file::<Vec<OperationHistoryEntry>>(&path)?;
    history.sort_by(|left, right| right.timestamp_ms.cmp(&left.timestamp_ms));
    Ok(history)
}

pub fn history_entry_id() -> String {
    crate::util::unique_id("op")
}

fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut base = app_data_dir(app)?;
    base.push("history.json");
    Ok(base)
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建目录失败: {error}"))?;
    }

    let payload =
        serde_json::to_string_pretty(value).map_err(|error| format!("序列化失败: {error}"))?;
    fs::write(path, payload).map_err(|error| format!("写入文件失败: {error}"))?;
    Ok(())
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let payload = fs::read_to_string(path).map_err(|error| format!("读取文件失败: {error}"))?;
    serde_json::from_str(&payload).map_err(|error| format!("解析文件失败: {error}"))
}
