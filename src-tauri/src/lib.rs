mod adb;
mod analyzer;
mod cleanup;
mod records;

use adb::AndroidDevice;
use analyzer::DeviceAnalysis;
use cleanup::{CleanupExecutionReport, CleanupRestoreReport};
use records::OperationHistoryEntry;

#[tauri::command]
fn scan_devices() -> Result<Vec<AndroidDevice>, String> {
    adb::scan_devices()
}

#[tauri::command]
fn analyze_device(serial: String) -> Result<DeviceAnalysis, String> {
    let devices = adb::scan_devices()?;
    let device = devices
        .into_iter()
        .find(|device| device.serial == serial)
        .ok_or_else(|| format!("未找到序列号为 {} 的设备", serial))?;

    let packages = adb::collect_package_inventory(&serial)?;
    let runtime_profile = adb::collect_runtime_profile(&serial);
    Ok(analyzer::analyze_device(
        &device,
        &packages,
        &runtime_profile,
    ))
}

#[tauri::command]
fn execute_cleanup(
    app: tauri::AppHandle,
    serial: String,
    package_names: Vec<String>,
) -> Result<CleanupExecutionReport, String> {
    cleanup::execute_cleanup(&app, &serial, &package_names)
}

#[tauri::command]
fn restore_cleanup(app: tauri::AppHandle, serial: String) -> Result<CleanupRestoreReport, String> {
    cleanup::restore_cleanup(&app, &serial)
}

#[tauri::command]
fn list_operation_history(app: tauri::AppHandle) -> Result<Vec<OperationHistoryEntry>, String> {
    records::list_operation_history(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            adb::configure_adb_candidates(&app.handle());
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_devices,
            analyze_device,
            execute_cleanup,
            restore_cleanup,
            list_operation_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
