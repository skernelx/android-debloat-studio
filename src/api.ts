import { invoke } from '@tauri-apps/api/core'

import type {
  AndroidDevice,
  CleanupExecutionReport,
  CleanupRestoreReport,
  DeviceAnalysis,
  OperationHistoryEntry,
} from './types'

export async function scanDevices() {
  return invoke<AndroidDevice[]>('scan_devices')
}

export async function analyzeDevice(serial: string) {
  return invoke<DeviceAnalysis>('analyze_device', { serial })
}

export async function executeCleanup(serial: string, packageNames: string[]) {
  return invoke<CleanupExecutionReport>('execute_cleanup', { serial, packageNames })
}

export async function restoreCleanup(serial: string) {
  return invoke<CleanupRestoreReport>('restore_cleanup', { serial })
}

export async function listOperationHistory() {
  return invoke<OperationHistoryEntry[]>('list_operation_history')
}
