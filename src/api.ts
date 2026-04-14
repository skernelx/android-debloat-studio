import { invoke } from '@tauri-apps/api/core'

import type {
  AndroidDevice,
  CleanupMode,
  CleanupExecutionReport,
  CleanupRestoreReport,
  DeviceAnalysis,
  OperationHistoryEntry,
} from './types'

export async function scanDevices() {
  return invoke<AndroidDevice[]>('scan_devices')
}

export async function analyzeDevice(serial: string, mode: CleanupMode) {
  return invoke<DeviceAnalysis>('analyze_device', { serial, mode })
}

export async function executeCleanup(
  serial: string,
  packageNames: string[],
  mode: CleanupMode,
) {
  return invoke<CleanupExecutionReport>('execute_cleanup', { serial, packageNames, mode })
}

export async function restoreCleanup(serial: string) {
  return invoke<CleanupRestoreReport>('restore_cleanup', { serial })
}

export async function listOperationHistory() {
  return invoke<OperationHistoryEntry[]>('list_operation_history')
}
