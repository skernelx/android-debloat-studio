export type AndroidDevice = {
  serial: string
  state: string
  product?: string | null
  model?: string | null
  deviceCode?: string | null
  transportId?: string | null
  brand?: string | null
  manufacturer?: string | null
  androidVersion?: string | null
  sdk?: string | null
  buildDisplayId?: string | null
  fingerprint?: string | null
}

export type CleanupMode = 'balanced' | 'minimalCore'

export type PackageSource = 'system' | 'user' | 'unknown'

export type RiskLevel = 'coreKeep' | 'safeRemove' | 'userInstalled'

export type RecommendedAction = 'keep' | 'uninstallUser0'

export type PackageAssessment = {
  packageName: string
  installPath?: string | null
  source: PackageSource
  visibleOnLauncher: boolean
  riskLevel: RiskLevel
  recommendedAction: RecommendedAction
  reasons: string[]
}

export type AnalysisSummary = {
  totalPackages: number
  systemPackages: number
  userPackages: number
  visiblePackages: number
  coreKeep: number
  safeRemove: number
}

export type DeviceAnalysis = {
  mode: CleanupMode
  device: AndroidDevice
  vendorFamily: string
  summary: AnalysisSummary
  packages: PackageAssessment[]
}

export type CheckStatus = 'passed' | 'failed' | 'warning'

export type HealthCheck = {
  key: string
  label: string
  status: CheckStatus
  detail: string
}

export type DeviceHealthReport = {
  checkedAtMs: number
  passed: boolean
  checks: HealthCheck[]
}

export type PackageOperationStatus = 'removed' | 'restored' | 'reverted' | 'failed'

export type PackageOperationResult = {
  packageName: string
  status: PackageOperationStatus
  detail: string
}

export type CleanupExecutionReport = {
  mode: CleanupMode
  serial: string
  startedAtMs: number
  finishedAtMs: number
  aborted: boolean
  removedCount: number
  failedCount: number
  rollbackReady: boolean
  results: PackageOperationResult[]
  healthReport: DeviceHealthReport
}

export type CleanupRestoreReport = {
  mode: CleanupMode
  serial: string
  startedAtMs: number
  finishedAtMs: number
  restoredCount: number
  failedCount: number
  results: PackageOperationResult[]
  healthReport: DeviceHealthReport
}

export type OperationKind = 'cleanup' | 'restore'

export type OperationHistoryEntry = {
  id: string
  kind: OperationKind
  mode: CleanupMode
  serial: string
  vendorFamily: string
  timestampMs: number
  packageCount: number
  successCount: number
  failedCount: number
  aborted: boolean
  healthPassed: boolean
  summary: string
}
