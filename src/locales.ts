export type Locale = 'zh' | 'en'

const STORAGE_KEY = 'wows-mac-patcher.locale'

type Vars = Record<string, string | number>

const zh = {
  appTitle: '战舰世界 macOS 补丁工具',
  tabStatus: '补丁状态',
  tabDiagnostics: '诊断',
  tabAbout: '关于',
  langZh: '中文',
  langEn: 'EN',

  noInstallOption: '未检测到安装',
  pickFolder: '选择文件夹…',
  pickFolderPrompt: '选择《战舰世界》安装目录',
  refresh: '刷新',

  busyDetect: '正在检测安装目录…',
  busyStatus: '正在读取状态…',
  busyPatch: '正在打补丁…',
  busyRestore: '正在还原…',
  busyClean: '正在清理 ._ 文件…',
  busyProcesses: '正在检查进程…',
  busyEnv: '正在检查环境…',
  busyWine: '正在查询 Wine 注册表…',
  busyLog: '正在分析游戏日志…',

  noticePatched: '已打补丁。备份位于 {path}',
  noticeRestored: '已从备份还原：{hash}…',
  noticeCleaned: "已删除 {count} 个 macOS '._' 元数据文件。",
  noticeCleanPartial: '已删除 {removed} 个，{failed} 个删除失败。',

  emptyTitle: '没有可用的安装',
  emptyBody: '请用「选择文件夹…」指定《战舰世界》安装目录。',

  stateOriginal: '未打补丁',
  statePatched: '已打补丁',
  stateForeign: '无法处理',
  stateUnresolved: '无法定位',
  stateHelpForeign: '文件已被修改，本工具无法安全处理。请先验证/修复游戏文件后再试。',
  stateHelpUnresolved: '找不到要打补丁的函数，可能是尚未支持的游戏版本。',

  fieldInstall: '安装目录',
  fieldBuild: '构建',
  fieldBackup: '备份',
  backupYes: '已有，可还原',
  backupNo: '无',

  actionPatch: '打补丁',
  actionRestore: '从备份还原',
  actionClean: '清理 ._ 文件（{count}）',

  sidecarsTitle: '需清理的 ._ 文件',
  sidecarsCount: '共 {count} 个',
  sidecarsLive: '，其中 {count} 个在当前构建内，可能导致无法启动',

  procTitle: '进程检查',
  procCheck: '检查',
  procIdle: '点击检查',
  procNone: '未检测到游戏或启动器进程',
  procWarn: '请先退出以下进程再打补丁：',

  envTitle: '环境检查',
  envWine: '查询 Wine 注册表',
  envRun: '运行检查',
  envIdle: '点击运行检查',
  envOk: '主机名正常',
  envUnset: '（未设置）',
  envMustLocal: ' · 必须以 .local 结尾',
  envResolved: '解析结果',
  envUnresolved: '无法解析',
  envEgress: '出站网卡',
  envVpn: ' · 正在走 VPN 隧道',
  envWineReg: 'Wine 注册表',
  envWineMissing: '未找到',
  envFix: '在终端执行以修复：',
  envCopy: '复制命令',
  envOpenTerminal: '在终端中打开',
  envDetails: '说明与无效做法',
  envNoteNoLocalHost: '读不到 LocalHostName，无法自动推导修复命令。请先确认 `scutil --get LocalHostName` 能返回一个名字。',
  envNoteNotLocal:
    'HostName 不以 .local 结尾。游戏会调用 gethostname() 拿到这个名字，再把它解析成自己的本地 IP——而 macOS 解析不了一个裸主机名，于是 socket 绑定失败，游戏报网络错误。',
  envNoteNoResolve: 'HostName 看起来是对的，但 {host} 解析不出地址。请确认 mDNS 正常。',
  envDeadHosts:
    '在 /etc/hosts 里写死「当前 IP + 主机名」：确实能用，但依赖 DHCP，每次换网络就失效，而且这台机器上有东西会重写 /etc/hosts。',
  envDeadWine:
    '改 Wine 的 Hostname 注册表项：Wine 每次启动 wineserver 都会从 Unix 主机名重新生成这个值，永远不会持久化。',

  logTitle: '游戏日志',
  logScan: '分析日志',
  logIdle: '点击分析日志',
  logMissing: '找不到日志：{path}',
  logEmpty: '暂无会话记录',
  logColStart: '启动',
  logColErrors: 'ERROR',
  logColLocal: '本地地址失败',
  logColVerdict: '判定',
  logHealthy: '正常',
  logUnhealthy: '网络不可用',
  bytes: '{n} 字节',

  aboutTitle: '关于',
  aboutBody: '补丁偏移每次从 DLL 导出表重新解析，游戏更新后也不会写坏文件。所有改动可一键还原。',
  aboutStateFile: '状态文件',
  aboutReveal: '在 Finder 中显示',

  confirmTitle: '确认打补丁',
  confirmFile: '文件',
  confirmBackup: '备份',
  confirmCleanLabel: '清理',
  confirmClean: "同时删除 {count} 个 '._' 文件",
  confirmWarn: '请先退出游戏和启动器。',
  confirmCancel: '取消',
  confirmOk: '确认打补丁',
} as const

type MessageKey = keyof typeof zh

const en: Record<MessageKey, string> = {
  appTitle: 'WoWs macOS Patcher',
  tabStatus: 'Status',
  tabDiagnostics: 'Diagnostics',
  tabAbout: 'About',
  langZh: '中文',
  langEn: 'EN',

  noInstallOption: 'No install detected',
  pickFolder: 'Choose folder…',
  pickFolderPrompt: 'Select the World of Warships install folder',
  refresh: 'Refresh',

  busyDetect: 'Detecting installs…',
  busyStatus: 'Reading status…',
  busyPatch: 'Applying patch…',
  busyRestore: 'Restoring…',
  busyClean: 'Cleaning ._ files…',
  busyProcesses: 'Checking processes…',
  busyEnv: 'Checking environment…',
  busyWine: 'Querying Wine registry…',
  busyLog: 'Analyzing game log…',

  noticePatched: 'Patched. Backup at {path}',
  noticeRestored: 'Restored from backup: {hash}…',
  noticeCleaned: "Removed {count} macOS '._' metadata files.",
  noticeCleanPartial: 'Removed {removed}; {failed} failed to delete.',

  emptyTitle: 'No install available',
  emptyBody: 'Use “Choose folder…” to select your World of Warships install.',

  stateOriginal: 'Not patched',
  statePatched: 'Patched',
  stateForeign: 'Cannot handle',
  stateUnresolved: 'Unresolved',
  stateHelpForeign:
    'The file was modified in a way this tool cannot safely handle. Verify/repair game files, then try again.',
  stateHelpUnresolved:
    'Could not find the function to patch. This game build may be unsupported.',

  fieldInstall: 'Install',
  fieldBuild: 'Build',
  fieldBackup: 'Backup',
  backupYes: 'Present, can restore',
  backupNo: 'None',

  actionPatch: 'Patch',
  actionRestore: 'Restore backup',
  actionClean: 'Clean ._ files ({count})',

  sidecarsTitle: '._ files to clean',
  sidecarsCount: '{count} found',
  sidecarsLive: ', including {count} inside the live build (may prevent launch)',

  procTitle: 'Processes',
  procCheck: 'Check',
  procIdle: 'Click to check',
  procNone: 'No game or launcher processes detected',
  procWarn: 'Quit these processes before patching:',

  envTitle: 'Environment',
  envWine: 'Query Wine registry',
  envRun: 'Run check',
  envIdle: 'Click to run check',
  envOk: 'Hostname looks good',
  envUnset: '(unset)',
  envMustLocal: ' · must end with .local',
  envResolved: 'Resolved',
  envUnresolved: 'Could not resolve',
  envEgress: 'Egress NIC',
  envVpn: ' · traffic is on a VPN tunnel',
  envWineReg: 'Wine registry',
  envWineMissing: 'Not found',
  envFix: 'Run this in Terminal to fix:',
  envCopy: 'Copy command',
  envOpenTerminal: 'Open in Terminal',
  envDetails: 'Notes and dead ends',
  envNoteNoLocalHost:
    'LocalHostName is empty, so a fix command cannot be derived. Confirm `scutil --get LocalHostName` returns a name.',
  envNoteNotLocal:
    'HostName does not end with .local. The game calls gethostname() and resolves that name to its local IP — macOS cannot resolve a bare hostname, so the socket bind fails.',
  envNoteNoResolve: 'HostName looks fine, but {host} does not resolve. Check that mDNS is working.',
  envDeadHosts:
    'Hard-coding “current IP + hostname” in /etc/hosts can work, but breaks after DHCP changes and something on this machine rewrites /etc/hosts.',
  envDeadWine:
    'Editing Wine’s Hostname registry value does not stick — wineserver regenerates it from the Unix hostname on every start.',

  logTitle: 'Game log',
  logScan: 'Analyze log',
  logIdle: 'Click to analyze',
  logMissing: 'Log not found: {path}',
  logEmpty: 'No sessions in the log yet',
  logColStart: 'Started',
  logColErrors: 'ERROR',
  logColLocal: 'Local address failures',
  logColVerdict: 'Verdict',
  logHealthy: 'OK',
  logUnhealthy: 'Network down',
  bytes: '{n} bytes',

  aboutTitle: 'About',
  aboutBody:
    'Patch offsets are resolved from the DLL export table every run, so game updates will not corrupt the file. Every change can be restored in one click.',
  aboutStateFile: 'State file',
  aboutReveal: 'Show in Finder',

  confirmTitle: 'Confirm patch',
  confirmFile: 'File',
  confirmBackup: 'Backup',
  confirmCleanLabel: 'Clean',
  confirmClean: "Also delete {count} '._' files",
  confirmWarn: 'Quit the game and its launcher first.',
  confirmCancel: 'Cancel',
  confirmOk: 'Confirm patch',
}

const DICTS: Record<Locale, Record<MessageKey, string>> = { zh, en }

export type { MessageKey }

export function detectLocale(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved === 'zh' || saved === 'en') return saved
  } catch {
    /* ignore */
  }
  const lang = typeof navigator !== 'undefined' ? navigator.language.toLowerCase() : 'zh'
  return lang.startsWith('zh') ? 'zh' : 'en'
}

export function persistLocale(locale: Locale): void {
  try {
    localStorage.setItem(STORAGE_KEY, locale)
  } catch {
    /* ignore */
  }
}

export function translate(locale: Locale, key: MessageKey, vars?: Vars): string {
  let text = DICTS[locale][key] ?? DICTS.zh[key] ?? key
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      text = text.replaceAll(`{${name}}`, String(value))
    }
  }
  return text
}

export function numberLocale(locale: Locale): string {
  return locale === 'zh' ? 'zh-CN' : 'en-US'
}
