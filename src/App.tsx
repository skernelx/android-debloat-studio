import { startTransition, useEffect, useState } from 'react'

import {
  analyzeDevice,
  executeCleanup,
  listOperationHistory,
  restoreCleanup,
  scanDevices,
} from './api'
import type {
  AndroidDevice,
  CleanupExecutionReport,
  CleanupRestoreReport,
  DeviceAnalysis,
  DeviceHealthReport,
  OperationHistoryEntry,
  PackageAssessment,
  PackageOperationResult,
} from './types'
import './App.css'

function App() {
  const [devices, setDevices] = useState<AndroidDevice[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [lastRefreshedAt, setLastRefreshedAt] = useState<string | null>(null)
  const [analysis, setAnalysis] = useState<DeviceAnalysis | null>(null)
  const [executionReport, setExecutionReport] = useState<CleanupExecutionReport | null>(null)
  const [restoreReport, setRestoreReport] = useState<CleanupRestoreReport | null>(null)
  const [historyEntries, setHistoryEntries] = useState<OperationHistoryEntry[]>([])
  const [selectedPackageNames, setSelectedPackageNames] = useState<string[]>([])
  const [actionMessage, setActionMessage] = useState<string | null>(null)
  const [analyzingSerial, setAnalyzingSerial] = useState<string | null>(null)
  const [executingSerial, setExecutingSerial] = useState<string | null>(null)
  const [restoringSerial, setRestoringSerial] = useState<string | null>(null)

  async function refreshDevices() {
    setLoading(true)
    setError(null)

    try {
      const nextDevices = await scanDevices()
      startTransition(() => {
        setDevices(nextDevices)
        setLastRefreshedAt(new Date().toLocaleTimeString())
      })
    } catch (refreshError) {
      const message =
        refreshError instanceof Error ? refreshError.message : String(refreshError)
      setError(message)
    } finally {
      setLoading(false)
    }
  }

  async function refreshAnalysis(serial: string) {
    const nextAnalysis = await analyzeDevice(serial)

    startTransition(() => {
      setAnalysis(nextAnalysis)
      setSelectedPackageNames(defaultSelectedPackages(nextAnalysis))
    })

    return nextAnalysis
  }

  async function refreshLibrary() {
    const nextHistory = await listOperationHistory()

    startTransition(() => {
      setHistoryEntries(nextHistory)
    })
  }

  async function handleAnalyze(serial: string) {
    setAnalyzingSerial(serial)
    setError(null)

    try {
      const nextAnalysis = await refreshAnalysis(serial)
      startTransition(() => {
        setExecutionReport(null)
        setRestoreReport(null)
        setActionMessage(
          `已完成 ${serial} 的预装包分析，安全可删项已默认全选。`,
        )
      })

      if (nextAnalysis.packages.length === 0) {
        setActionMessage(`已完成 ${serial} 的预装包分析，但没有读到任何包。`)
      }
    } catch (analysisError) {
      const message =
        analysisError instanceof Error ? analysisError.message : String(analysisError)
      setError(message)
    } finally {
      setAnalyzingSerial(null)
    }
  }

  async function handleExecute(serial: string) {
    if (!analysis || analysis.device.serial !== serial) {
      setError('请先分析当前设备。')
      return
    }

    const nextSelection = selectedSafePackages(analysis, selectedPackageNames)
    if (nextSelection.length === 0) {
      setError('请先勾选至少一个“安全可删”包。')
      return
    }

    setExecutingSerial(serial)
    setError(null)

    try {
      const report = await executeCleanup(
        serial,
        nextSelection.map((item) => item.packageName),
      )

      try {
        await refreshAnalysis(serial)
      } catch (analysisError) {
        console.warn('cleanup finished but analysis refresh failed', analysisError)
      }

      try {
        await refreshLibrary()
      } catch (libraryError) {
        console.warn('cleanup finished but history refresh failed', libraryError)
      }

      startTransition(() => {
        setExecutionReport(report)
        setRestoreReport(null)
        setActionMessage(
          report.aborted
            ? '清理过程中触发了保护性中止，已停止继续删除。'
            : `清理完成，本轮共处理 ${report.results.length} 个包。`,
        )
      })
    } catch (executeError) {
      const message = executeError instanceof Error ? executeError.message : String(executeError)
      setError(message)
    } finally {
      setExecutingSerial(null)
    }
  }

  async function handleRestore(serial: string) {
    setRestoringSerial(serial)
    setError(null)

    try {
      const report = await restoreCleanup(serial)

      try {
        await refreshAnalysis(serial)
      } catch (analysisError) {
        console.warn('restore finished but analysis refresh failed', analysisError)
      }

      try {
        await refreshLibrary()
      } catch (libraryError) {
        console.warn('restore finished but history refresh failed', libraryError)
      }

      startTransition(() => {
        setRestoreReport(report)
        setExecutionReport(null)
        setActionMessage(`恢复完成，共恢复 ${report.restoredCount} 个包。`)
      })
    } catch (restoreError) {
      const message = restoreError instanceof Error ? restoreError.message : String(restoreError)
      setError(message)
    } finally {
      setRestoringSerial(null)
    }
  }

  function selectAllSafePackages() {
    if (!analysis) {
      return
    }

    setSelectedPackageNames(defaultSelectedPackages(analysis))
  }

  function clearSelectedPackages() {
    setSelectedPackageNames([])
  }

  function togglePackageSelection(packageName: string) {
    setSelectedPackageNames((current) =>
      current.includes(packageName)
        ? current.filter((item) => item !== packageName)
        : [...current, packageName],
    )
  }

  const safeRemovePackages = analysis?.packages.filter(
    (item) => item.riskLevel === 'safeRemove',
  ) ?? []
  const keepPackages = analysis?.packages.filter((item) => item.riskLevel === 'coreKeep') ?? []
  const userInstalledPackages =
    analysis?.packages.filter((item) => item.riskLevel === 'userInstalled') ?? []
  const selectedSafeEntries = selectedSafePackages(analysis, selectedPackageNames)
  const activeSerial = analysis?.device.serial ?? null
  const latestHealthReport = restoreReport?.healthReport ?? executionReport?.healthReport ?? null
  const deviceGuide = buildDeviceGuide(devices, loading, error)

  useEffect(() => {
    void refreshDevices()
  }, [])

  useEffect(() => {
    void refreshLibrary()
  }, [])

  return (
    <main className="app-shell">
      <section className="hero-panel">
        <p className="eyebrow">Android Debloat Studio</p>
        <h1>安全极限精简桌面程序</h1>
        <p className="intro">
          自动识别设备、分析预装包，并把安全可删项直接列成可勾选清理列表。
        </p>
        <div className="hero-actions">
          <button className="primary-button" onClick={() => void refreshDevices()}>
            {loading ? '正在刷新设备…' : '刷新设备'}
          </button>
          <span className="timestamp">
            {lastRefreshedAt ? `上次刷新 ${lastRefreshedAt}` : '尚未完成首次扫描'}
          </span>
        </div>
      </section>

      <section className="status-grid">
        <article className="status-card">
          <h2>当前阶段</h2>
          <p>设备识别、包分析、逐包验活、自动回退和最近一次恢复都已接上。</p>
        </article>
        <article className="status-card">
          <h2>执行策略</h2>
          <p>只允许勾选绿色安全项；真正执行前，后端还会再按安全规则过滤一遍。</p>
        </article>
      </section>

      <section className="device-panel">
        <div className="section-heading">
          <div>
            <p className="eyebrow">设备扫描</p>
            <h2>已识别设备</h2>
          </div>
          <span className="device-count">{devices.length} 台</span>
        </div>

        {error ? <p className="error-banner">{error}</p> : null}
        {actionMessage ? <p className="info-banner">{actionMessage}</p> : null}

        {devices.length === 0 && !loading ? (
          <p className="empty-state">当前没有识别到可用安卓设备。</p>
        ) : null}

        {deviceGuide ? <DeviceGuidePanel guide={deviceGuide} /> : null}

        <div className="device-list">
          {devices.map((device) => (
            <article className="device-card" key={device.serial}>
              <div className="device-card-header">
                <div>
                  <p className="device-name">
                    {device.brand ?? device.manufacturer ?? '未知品牌'} ·{' '}
                    {device.model ?? device.product ?? '未知型号'}
                  </p>
                  <p className="device-meta">{device.serial}</p>
                </div>
                <span
                  className={
                    device.state === 'device' ? 'device-state ready' : 'device-state'
                  }
                >
                  {deviceStateText(device.state)}
                </span>
              </div>

              <dl className="device-specs">
                <div>
                  <dt>系统版本</dt>
                  <dd>{device.androidVersion ?? '-'}</dd>
                </div>
                <div>
                  <dt>SDK</dt>
                  <dd>{device.sdk ?? '-'}</dd>
                </div>
                <div>
                  <dt>设备代号</dt>
                  <dd>{device.deviceCode ?? '-'}</dd>
                </div>
                <div>
                  <dt>Build</dt>
                  <dd>{device.buildDisplayId ?? '-'}</dd>
                </div>
              </dl>

              {device.state !== 'device' ? (
                <p className="device-state-hint">{deviceStateHint(device.state)}</p>
              ) : null}

              <div className="device-actions">
                <button
                  className="secondary-button"
                  disabled={device.state !== 'device' || analyzingSerial === device.serial}
                  onClick={() => void handleAnalyze(device.serial)}
                >
                  {analyzingSerial === device.serial ? '分析中…' : '分析预装包'}
                </button>
              </div>
            </article>
          ))}
        </div>
      </section>

      <section className="analysis-panel">
        <div className="section-heading">
          <div>
            <p className="eyebrow">风险分析</p>
            <h2>预装包分层结果</h2>
          </div>
          <span className="device-count">
            {analysis ? `${analysis.vendorFamily} 规则` : '等待分析'}
          </span>
        </div>

        {!analysis ? (
          <p className="empty-state">
            先让设备处于 <code>device</code> 状态。若还没看到设备，先在手机里打开
            <code>USB 调试</code>，并允许当前电脑的调试授权。
          </p>
        ) : (
          <>
            <div className="summary-grid">
              <SummaryCard label="总包数" value={analysis.summary.totalPackages} />
              <SummaryCard label="系统包" value={analysis.summary.systemPackages} />
              <SummaryCard label="用户/已更新包" value={analysis.summary.userPackages} />
              <SummaryCard label="桌面入口" value={analysis.summary.visiblePackages} />
              <SummaryCard label="核心保留" value={analysis.summary.coreKeep} />
              <SummaryCard label="安全可删" value={analysis.summary.safeRemove} />
            </div>

            <p className="summary-note">
              “用户/已更新包” 既包含你自己安装的软件，也包含已经升级到
              <code>/data/app</code>
              的系统预装。
            </p>

            <section className="operations-panel">
              <div className="operations-header">
                <div>
                  <p className="eyebrow">清理执行</p>
                  <h3>勾选后直接清除</h3>
                </div>
                <span className="device-count">
                  {selectedSafeEntries.length}/{safeRemovePackages.length} 已勾选
                </span>
              </div>

              <p className="operations-note">
                只会执行绿色“安全可删”列表里的勾选项；后端仍会再次校验安全范围，
                并在每删一个包后立刻做验活。
              </p>

              <div className="operation-actions">
                <button
                  className="secondary-button"
                  disabled={safeRemovePackages.length === 0}
                  onClick={selectAllSafePackages}
                >
                  全选安全可删
                </button>
                <button
                  className="secondary-button"
                  disabled={selectedSafeEntries.length === 0}
                  onClick={clearSelectedPackages}
                >
                  清空勾选
                </button>
                <button
                  className="primary-button"
                  disabled={
                    !activeSerial ||
                    selectedSafeEntries.length === 0 ||
                    executingSerial === activeSerial
                  }
                  onClick={() => activeSerial && void handleExecute(activeSerial)}
                >
                  {executingSerial === activeSerial
                    ? '清理中…'
                    : `一键清除 ${selectedSafeEntries.length} 项`}
                </button>
                <button
                  className="secondary-button"
                  disabled={!activeSerial || restoringSerial === activeSerial}
                  onClick={() => activeSerial && void handleRestore(activeSerial)}
                >
                  {restoringSerial === activeSerial
                    ? '恢复中…'
                    : '恢复最近一次清理'}
                </button>
              </div>
            </section>

            {executionReport ? (
              <OperationReport
                title="最近一次清理结果"
                subtitle={`开始 ${formatDateTime(executionReport.startedAtMs)} · 结束 ${formatDateTime(executionReport.finishedAtMs)}`}
                summary={`已删除 ${executionReport.removedCount} 个包，失败或回退 ${executionReport.failedCount} 个。`}
                results={executionReport.results}
              />
            ) : null}

            {restoreReport ? (
              <OperationReport
                title="最近一次恢复结果"
                subtitle={`开始 ${formatDateTime(restoreReport.startedAtMs)} · 结束 ${formatDateTime(restoreReport.finishedAtMs)}`}
                summary={`已恢复 ${restoreReport.restoredCount} 个包，恢复失败 ${restoreReport.failedCount} 个。`}
                results={restoreReport.results}
              />
            ) : null}

            {latestHealthReport ? (
              <HealthPanel title="最近一次验活结果" report={latestHealthReport} />
            ) : null}

            <HistoryPanel entries={historyEntries} />

            <div className="analysis-columns">
              <SelectablePackageList
                title="安全可删"
                tone="safe"
                items={safeRemovePackages}
                selectedPackageNames={selectedPackageNames}
                emptyText="当前没有命中绿色安全清理包。"
                onToggle={togglePackageSelection}
              />
              <PackageList
                title="核心保留"
                tone="keep"
                items={keepPackages}
                emptyText="当前没有识别到核心包。"
              />
              <PackageList
                title="用户安装"
                tone="user"
                items={userInstalledPackages}
                emptyText="当前没有识别到用户安装包。"
              />
            </div>
          </>
        )}
      </section>
    </main>
  )
}

function SummaryCard({ label, value }: { label: string; value: number }) {
  return (
    <article className="summary-card">
      <p className="summary-label">{label}</p>
      <p className="summary-value">{value}</p>
    </article>
  )
}

type DeviceGuide = {
  title: string
  tone: 'info' | 'warning'
  body: string
  steps: string[]
}

function DeviceGuidePanel({ guide }: { guide: DeviceGuide }) {
  return (
    <section className={`device-guide ${guide.tone}`}>
      <div className="report-heading">
        <div>
          <p className="eyebrow">连接引导</p>
          <h3>{guide.title}</h3>
        </div>
      </div>
      <p className="health-detail">{guide.body}</p>
      <ol className="guide-steps">
        {guide.steps.map((step) => (
          <li key={step}>{step}</li>
        ))}
      </ol>
    </section>
  )
}

function HealthPanel({
  title,
  report,
}: {
  title: string
  report: DeviceHealthReport
}) {
  return (
    <section className="report-panel">
      <div className="report-heading">
        <div>
          <p className="eyebrow">验活</p>
          <h3>{title}</h3>
        </div>
        <span className={report.passed ? 'pill success' : 'pill danger'}>
          {report.passed ? '通过' : '失败'}
        </span>
      </div>

      <p className="report-meta">检查时间 {formatDateTime(report.checkedAtMs)}</p>

      <div className="health-grid">
        {report.checks.map((item) => (
          <article className="health-card" key={item.key}>
            <div className="health-card-header">
              <p className="summary-label">{item.label}</p>
              <span className={`pill ${statusTone(item.status)}`}>
                {statusText(item.status)}
              </span>
            </div>
            <p className="health-detail">{item.detail}</p>
          </article>
        ))}
      </div>
    </section>
  )
}

function OperationReport({
  title,
  subtitle,
  summary,
  results,
}: {
  title: string
  subtitle: string
  summary: string
  results: PackageOperationResult[]
}) {
  return (
    <section className="report-panel">
      <div className="report-heading">
        <div>
          <p className="eyebrow">执行日志</p>
          <h3>{title}</h3>
        </div>
      </div>

      <p className="report-meta">{subtitle}</p>
      <p className="report-summary">{summary}</p>

      <div className="result-list">
        {results.map((item) => (
          <article className="result-card" key={`${item.packageName}-${item.status}`}>
            <div className="health-card-header">
              <div className="result-title-block">
                <p className="package-title">{buildDisplayName(item.packageName)}</p>
                <p className="package-code">{item.packageName}</p>
              </div>
              <span className={`pill ${resultTone(item.status)}`}>
                {resultText(item.status)}
              </span>
            </div>
            <p className="health-detail">{item.detail}</p>
          </article>
        ))}
      </div>
    </section>
  )
}

function SelectablePackageList({
  title,
  tone,
  items,
  selectedPackageNames,
  emptyText,
  onToggle,
}: {
  title: string
  tone: 'safe'
  items: PackageAssessment[]
  selectedPackageNames: string[]
  emptyText: string
  onToggle: (packageName: string) => void
}) {
  const selectedSet = new Set(selectedPackageNames)

  return (
    <article className={`package-list ${tone}`}>
      <div className="package-list-heading">
        <h3>{title}</h3>
        <span className="selection-summary">
          {items.length === 0 ? '暂无条目' : `${selectedSet.size}/${items.length} 已勾选`}
        </span>
      </div>
      <div className="package-list-scroll">
        {items.length === 0 ? (
          <p className="package-empty">{emptyText}</p>
        ) : (
          <ul>
            {items.map((item) => (
              <SelectablePackageEntry
                key={item.packageName}
                item={item}
                checked={selectedSet.has(item.packageName)}
                onToggle={onToggle}
              />
            ))}
          </ul>
        )}
      </div>
    </article>
  )
}

function PackageList({
  title,
  tone,
  items,
  emptyText,
}: {
  title: string
  tone: 'keep' | 'user'
  items: PackageAssessment[]
  emptyText: string
}) {
  return (
    <article className={`package-list ${tone}`}>
      <div className="package-list-heading">
        <h3>{title}</h3>
        <span className="selection-summary">{items.length} 项</span>
      </div>
      <div className="package-list-scroll">
        {items.length === 0 ? (
          <p className="package-empty">{emptyText}</p>
        ) : (
          <ul>
            {items.map((item) => (
              <PackageEntry
                key={item.packageName}
                packageName={item.packageName}
                installPath={item.installPath}
                reason={item.reasons[0]}
              />
            ))}
          </ul>
        )}
      </div>
    </article>
  )
}

function SelectablePackageEntry({
  item,
  checked,
  onToggle,
}: {
  item: PackageAssessment
  checked: boolean
  onToggle: (packageName: string) => void
}) {
  return (
    <li>
      <label className={`package-entry selectable ${checked ? 'checked' : ''}`}>
        <input
          className="package-checkbox"
          type="checkbox"
          checked={checked}
          onChange={() => onToggle(item.packageName)}
        />
        <div className="package-copy">
          <p className="package-title">{buildDisplayName(item.packageName, item.installPath)}</p>
          <p className="package-code">{item.packageName}</p>
          <p className="package-purpose">{buildPurposeText(item.packageName, item.installPath)}</p>
          <p className="package-reason">{item.reasons[0]}</p>
        </div>
      </label>
    </li>
  )
}

function PackageEntry({
  packageName,
  installPath,
  reason,
}: {
  packageName: string
  installPath?: string | null
  reason: string
}) {
  return (
    <li className="package-entry">
      <p className="package-title">{buildDisplayName(packageName, installPath)}</p>
      <p className="package-code">{packageName}</p>
      <p className="package-purpose">{buildPurposeText(packageName, installPath)}</p>
      <p className="package-reason">{reason}</p>
    </li>
  )
}

function HistoryPanel({ entries }: { entries: OperationHistoryEntry[] }) {
  return (
    <section className="report-panel">
      <div className="report-heading">
        <div>
          <p className="eyebrow">批次历史</p>
          <h3>最近执行记录</h3>
        </div>
        <span className="device-count">{entries.length} 条</span>
      </div>

      {entries.length === 0 ? (
        <p className="empty-state">当前还没有清理或恢复历史。</p>
      ) : (
        <div className="result-list">
          {entries.slice(0, 8).map((item) => (
            <article className="result-card" key={item.id}>
              <div className="health-card-header">
                <div className="result-title-block">
                  <p className="package-title">
                    {item.kind === 'cleanup' ? '清理批次' : '恢复批次'}
                  </p>
                  <p className="package-code">{item.serial}</p>
                </div>
                <span
                  className={`pill ${
                    item.healthPassed && item.failedCount === 0 ? 'success' : 'warning'
                  }`}
                >
                  {item.kind === 'cleanup' && item.aborted ? '中止' : '已记录'}
                </span>
              </div>
              <p className="health-detail">
                {formatDateTime(item.timestampMs)} · {item.summary}
              </p>
            </article>
          ))}
        </div>
      )}
    </section>
  )
}

function defaultSelectedPackages(analysis: DeviceAnalysis) {
  return analysis.packages
    .filter((item) => item.riskLevel === 'safeRemove')
    .map((item) => item.packageName)
}

function selectedSafePackages(
  analysis: DeviceAnalysis | null,
  selectedPackageNames: string[],
) {
  if (!analysis) {
    return []
  }

  const selectedSet = new Set(selectedPackageNames)
  return analysis.packages.filter(
    (item) => item.riskLevel === 'safeRemove' && selectedSet.has(item.packageName),
  )
}

const DISPLAY_NAME_MAP: Record<string, string> = {
  'com.android.browser': '浏览器',
  'com.android.fileexplorer': '文件管理',
  'com.android.thememanager': '主题商店',
  'com.android.vending': 'Google Play 商店',
  'com.miui.analytics': '小米数据分析',
  'com.miui.contentextension': '内容推荐',
  'com.miui.greenguard': '家长守护',
  'com.miui.hybrid': '小米快应用',
  'com.miui.nextpay': '小米支付',
  'com.miui.player': '音乐',
  'com.miui.voiceassist': '小爱同学',
  'com.miui.yellowpage': '黄页/生活服务',
  'com.xiaomi.gamecenter.sdk.service': '游戏中心服务',
  'com.xiaomi.ugd': '广告推荐服务',
  'com.android.contacts': '联系人/拨号',
  'com.android.mms': '短信',
  'com.android.systemui': '系统界面',
  'com.android.settings': '系统设置',
  'com.miui.home': '系统桌面',
}

const NAME_KEYWORDS: Array<[string, string]> = [
  ['browser', '浏览器'],
  ['fileexplorer', '文件管理'],
  ['theme', '主题'],
  ['wallpaper', '壁纸'],
  ['voiceassist', '语音助手'],
  ['assistant', '助手'],
  ['yellowpage', '黄页/服务'],
  ['hybrid', '快应用'],
  ['analytics', '数据分析'],
  ['contentextension', '内容推荐'],
  ['market', '应用商店'],
  ['store', '应用商店'],
  ['vending', 'Google Play 商店'],
  ['music', '音乐'],
  ['player', '音乐'],
  ['video', '视频'],
  ['gallery', '相册'],
  ['photo', '相册'],
  ['weather', '天气'],
  ['search', '搜索'],
  ['calendar', '日历'],
  ['notes', '便签'],
  ['backup', '备份'],
  ['cloud', '云服务'],
  ['wallet', '钱包'],
  ['pay', '支付'],
  ['game', '游戏服务'],
]

const PURPOSE_MAP: Record<string, string> = {
  'com.android.browser': '用途：网页浏览',
  'com.android.fileexplorer': '用途：文件管理',
  'com.android.thememanager': '用途：主题、壁纸和外观资源',
  'com.android.vending': '用途：Google 应用商店',
  'com.miui.analytics': '用途：统计与行为分析',
  'com.miui.contentextension': '用途：内容推荐与信息流扩展',
  'com.miui.greenguard': '用途：家长控制/未成年人守护',
  'com.miui.hybrid': '用途：快应用/网页容器',
  'com.miui.nextpay': '用途：支付服务',
  'com.miui.player': '用途：本地和在线音乐播放',
  'com.miui.voiceassist': '用途：语音助手',
  'com.miui.yellowpage': '用途：号码黄页和生活服务',
  'com.xiaomi.gamecenter.sdk.service': '用途：游戏中心相关服务',
  'com.xiaomi.ugd': '用途：广告或内容分发',
}

const PURPOSE_KEYWORDS: Array<[string, string]> = [
  ['browser', '用途：浏览器'],
  ['fileexplorer', '用途：文件管理'],
  ['file', '用途：文件管理'],
  ['explorer', '用途：文件管理'],
  ['theme', '用途：主题和壁纸'],
  ['wallpaper', '用途：主题和壁纸'],
  ['voice', '用途：语音助手'],
  ['assistant', '用途：语音助手'],
  ['pay', '用途：支付/钱包'],
  ['wallet', '用途：支付/钱包'],
  ['game', '用途：游戏相关服务'],
  ['weather', '用途：天气'],
  ['video', '用途：视频'],
  ['music', '用途：音乐'],
  ['player', '用途：音乐'],
  ['gallery', '用途：相册/图片'],
  ['photo', '用途：相册/图片'],
  ['analytics', '用途：统计/上报'],
  ['report', '用途：统计/上报'],
  ['content', '用途：内容推荐/资讯扩展'],
  ['yellowpage', '用途：内容推荐/生活服务'],
  ['market', '用途：应用商店'],
  ['store', '用途：应用商店'],
  ['vending', '用途：应用商店'],
  ['search', '用途：搜索入口'],
  ['calendar', '用途：日历'],
  ['notes', '用途：便签/笔记'],
  ['backup', '用途：备份与迁移'],
  ['cloud', '用途：云同步/云服务'],
]

const GENERIC_PACKAGE_PARTS = new Set([
  'com',
  'android',
  'google',
  'xiaomi',
  'miui',
  'redmi',
  'huawei',
  'honor',
  'oppo',
  'vivo',
  'samsung',
  'sec',
  'oneplus',
  'realme',
  'coloros',
  'app',
])

function buildDisplayName(packageName: string, installPath?: string | null) {
  const directName = DISPLAY_NAME_MAP[packageName]
  if (directName) {
    return directName
  }

  const normalized = `${packageName} ${installPath ?? ''}`.toLowerCase()
  for (const [keyword, label] of NAME_KEYWORDS) {
    if (normalized.includes(keyword)) {
      return label
    }
  }

  const pathToken = extractPathToken(installPath)
  const pathName = prettifyToken(pathToken)
  if (pathName) {
    return pathName
  }

  return prettifyPackageTail(packageName)
}

function buildPurposeText(packageName: string, installPath?: string | null) {
  const directPurpose = PURPOSE_MAP[packageName]
  if (directPurpose) {
    return directPurpose
  }

  const normalized = `${packageName} ${installPath ?? ''}`.toLowerCase()
  for (const [keyword, purpose] of PURPOSE_KEYWORDS) {
    if (normalized.includes(keyword)) {
      return purpose
    }
  }

  return '用途：待识别'
}

function extractPathToken(installPath?: string | null) {
  if (!installPath) {
    return null
  }

  const segments = installPath.split('/').filter(Boolean)
  const fileName = segments.at(-1)?.replace(/\.apk$/i, '') ?? null

  if (fileName && fileName.toLowerCase() !== 'base') {
    return fileName
  }

  return segments.at(-2) ?? fileName
}

function prettifyToken(value?: string | null) {
  if (!value) {
    return null
  }

  let token = value
    .replace(/\.apk$/i, '')
    .replace(/@[A-Za-z0-9.\-_]+$/g, '')
    .replace(/-[A-Za-z0-9_=+]{6,}$/g, '')
    .replace(/^~~[A-Za-z0-9_=+]+$/g, '')
    .trim()

  if (!token || token.toLowerCase() === 'base') {
    return null
  }

  if (token.includes('.')) {
    const parts = token
      .split('.')
      .filter(Boolean)
      .filter((item) => !GENERIC_PACKAGE_PARTS.has(item.toLowerCase()))

    if (parts.length > 0) {
      token = parts.slice(-2).join(' ')
    }
  }

  token = token
    .replace(/\b(miui|apk)\b/gi, ' ')
    .replace(/\s+/g, ' ')
    .trim()

  return token ? humanizeToken(token) : null
}

function prettifyPackageTail(packageName: string) {
  const parts = packageName.split('.').filter(Boolean)
  const filtered = parts.filter((item) => !GENERIC_PACKAGE_PARTS.has(item.toLowerCase()))
  const token = filtered.slice(-2).join(' ') || parts.slice(-2).join(' ') || packageName
  return humanizeToken(token)
}

function humanizeToken(value: string) {
  return value
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2')
    .replace(/[_-]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

function formatDateTime(value: number) {
  return new Date(value).toLocaleString()
}

function buildDeviceGuide(
  devices: AndroidDevice[],
  loading: boolean,
  error: string | null,
): DeviceGuide | null {
  if (loading) {
    return null
  }

  if (error && error.toLowerCase().includes('adb')) {
    return {
      title: 'ADB 当前不可用',
      tone: 'warning',
      body: '应用已经尝试调用 adb，但本轮扫描没有成功完成。',
      steps: [
        '先确认电脑上的 adb 能正常运行，再回到这里点“刷新设备”。',
        '如果手机已连接但仍失败，重新插拔数据线或换一个 USB 口。',
        '确认手机已开启 USB 调试，并保持亮屏解锁状态。',
      ],
    }
  }

  if (devices.length === 0) {
    return {
      title: '还没有拿到可操作的安卓设备',
      tone: 'info',
      body: '这通常是手机没开 USB 调试，或者虽然插上了线，但电脑还没真正拿到 adb 连接。',
      steps: [
        '通用路径：设置 → 关于手机 → 连续点击“版本号 / 内部版本号”7 次，看到“你现在处于开发者模式”，再返回设置搜索“开发者选项”并打开“USB 调试”。',
        '小米 / Redmi：设置 → 我的设备 → 全部参数与信息 → 连续点“MIUI 版本 / Xiaomi HyperOS 版本”7 次 → 更多设置 → 开发者选项 → 打开“USB 调试”。',
        '华为 / 荣耀：设置 → 关于手机 → 连续点“版本号”7 次 → 系统和更新 → 开发人员选项 → 打开“USB 调试”。',
        'OPPO / realme / OnePlus：设置 → 关于本机 → 版本信息 → 连续点“版本号”7 次 → 其他设置 → 开发者选项 → 打开“USB 调试”。',
        'vivo / iQOO：设置 → 系统管理与升级 → 关于手机 → 软件版本号 → 连续点 7 次 → 系统管理与升级 → 开发者选项 → 打开“USB 调试”。',
        '三星：设置 → 关于手机 → 软件信息 → 连续点“版本号 / 内部版本号”7 次 → 开发者选项 → 打开“USB 调试”。',
        '用支持数据传输的线连接电脑，不要只用充电线。',
        '连接后保持手机亮屏，注意是否弹出“允许 USB 调试”，点“允许”。',
        '如果仍然没有设备，切换一次 USB 模式或重新插拔后再点“刷新设备”。',
      ],
    }
  }

  if (devices.some((device) => device.state === 'unauthorized')) {
    return {
      title: '设备已连上，但还没授权这台电脑',
      tone: 'warning',
      body: '现在 adb 已经看到手机了，但手机端还没有允许当前电脑进行 USB 调试。',
      steps: [
        '保持手机解锁并亮屏，查看是否有“允许 USB 调试”弹窗。',
        '点击“允许”，如果你常用这台电脑，可以勾选“总是允许”。',
        '如果一直不弹窗，先在开发者选项里撤销 USB 调试授权，再重新插拔数据线。',
        '如果发现手机根本没有“开发者选项”，先按上面的品牌路径把开发者模式和 USB 调试打开。',
        '授权完成后回到应用里点“刷新设备”。',
      ],
    }
  }

  if (devices.some((device) => device.state === 'offline')) {
    return {
      title: '设备处于离线状态',
      tone: 'warning',
      body: '电脑和手机都能彼此看到，但当前 adb 会话没有真正建立成功。',
      steps: [
        '重新插拔数据线，优先换一个 USB 口或换线。',
        '保持手机亮屏，确认 USB 模式不是只充电。',
        '如有调试授权弹窗，重新点一次“允许”。',
        '回到应用后再次点“刷新设备”。',
      ],
    }
  }

  return null
}

function deviceStateText(state: string) {
  if (state === 'device') {
    return '已就绪'
  }

  if (state === 'unauthorized') {
    return '待授权'
  }

  if (state === 'offline') {
    return '离线'
  }

  return state
}

function deviceStateHint(state: string) {
  if (state === 'unauthorized') {
    return '手机还没允许当前电脑进行 USB 调试。请解锁手机并点击“允许 USB 调试”。'
  }

  if (state === 'offline') {
    return '设备已被识别，但当前 adb 会话没连通。通常重新插拔数据线或重试授权就能恢复。'
  }

  return '当前不是可分析状态。先让设备恢复到 device 状态，再继续后续操作。'
}

function statusTone(status: DeviceHealthReport['checks'][number]['status']) {
  if (status === 'passed') {
    return 'success'
  }

  if (status === 'warning') {
    return 'warning'
  }

  return 'danger'
}

function statusText(status: DeviceHealthReport['checks'][number]['status']) {
  if (status === 'passed') {
    return '通过'
  }

  if (status === 'warning') {
    return '提示'
  }

  return '失败'
}

function resultTone(status: PackageOperationResult['status']) {
  if (status === 'removed' || status === 'restored') {
    return 'success'
  }

  if (status === 'reverted') {
    return 'warning'
  }

  return 'danger'
}

function resultText(status: PackageOperationResult['status']) {
  if (status === 'removed') {
    return '已删除'
  }

  if (status === 'restored') {
    return '已恢复'
  }

  if (status === 'reverted') {
    return '已回退'
  }

  return '失败'
}

export default App
