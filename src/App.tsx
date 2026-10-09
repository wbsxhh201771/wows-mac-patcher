import { useCallback, useEffect, useState } from 'react'
import * as api from './api'
import type { EnvReport, InstallDto, LogReport, PatchState, ReportDto } from './types'

const STATE_LABEL: Record<PatchState, string> = {
  ORIGINAL: '未打补丁',
  PATCHED: '已打补丁',
  FOREIGN: '无法处理',
  UNRESOLVED: '无法定位',
}

const BOTTLE = 'Steam'

type Tab = 'status' | 'diagnostics' | 'about'

function Field({ label, value, mono }: { label: string; value: React.ReactNode; mono?: boolean }) {
  return (
    <div className="field">
      <span className="field-label">{label}</span>
      <span className={mono ? 'field-value mono' : 'field-value'}>{value}</span>
    </div>
  )
}

function StateBadge({ state }: { state: PatchState }) {
  return (
    <span className={`badge badge-${state.toLowerCase()}`}>
      <b>{STATE_LABEL[state]}</b>
      <small>{state}</small>
    </span>
  )
}

/** Shorten a SHA-256 digest for display: first 6 + last 4. */
function shortHash(digest: string): string {
  if (!digest) return '—'
  return `${digest.slice(0, 6)}…${digest.slice(-4)}`
}

function bytes(size: number): string {
  if (!size) return '—'
  return `${size.toLocaleString('zh-CN')} 字节`
}

export default function App() {
  const [tab, setTab] = useState<Tab>('status')
  const [installs, setInstalls] = useState<InstallDto[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [report, setReport] = useState<ReportDto | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [confirming, setConfirming] = useState(false)

  const [processes, setProcesses] = useState<string[]>([])
  const [env, setEnv] = useState<EnvReport | null>(null)
  const [includeWine, setIncludeWine] = useState(false)
  const [log, setLog] = useState<LogReport | null>(null)
  const [statePath, setStatePath] = useState('')

  /** Runs an async action with the shared busy/error/notice plumbing. */
  const guard = useCallback(
    async (label: string, action: () => Promise<void>) => {
      setBusy(label)
      setError(null)
      setNotice(null)
      try {
        await action()
      } catch (exc) {
        setError(String(exc))
      } finally {
        setBusy(null)
      }
    },
    [],
  )

  const refreshStatus = useCallback(
    async (root: string | null) => {
      const next = await api.status(root)
      setReport(next)
      // The backend remembers the last install; keep the picker in sync with
      // whatever it actually resolved.
      if (next.install?.root) setSelected(next.install.root)
    },
    [],
  )

  const loadInstalls = useCallback(async () => {
    const found = await api.listInstalls()
    setInstalls(found)
    return found
  }, [])

  useEffect(() => {
    void guard('正在检测安装目录…', async () => {
      await loadInstalls()
      await refreshStatus(null)
    })
    void api.statePath().then(setStatePath).catch(() => {})
  }, [guard, loadInstalls, refreshStatus])

  const onSelectInstall = (root: string) =>
    void guard('正在读取状态…', () => refreshStatus(root))

  const onPickFolder = () =>
    void guard('正在读取状态…', async () => {
      const picked = await api.pickFolder('选择《战舰世界》安装目录')
      if (!picked) return
      const install = await api.resolveInstall(picked)
      setSelected(install.root)
      await refreshStatus(install.root)
    })

  const onRefresh = () => void guard('正在读取状态…', () => refreshStatus(selected))

  const onPatch = () =>
    void guard('正在打补丁…', async () => {
      setConfirming(false)
      const outcome = await api.patch(selected, report?.target?.sha256 ?? '')
      await refreshStatus(selected)
      setNotice(`已打补丁。备份位于 ${outcome.backup}`)
    })

  const onRestore = () =>
    void guard('正在还原…', async () => {
      const outcome = await api.restore(selected)
      await refreshStatus(selected)
      setNotice(`已从备份还原：${outcome.sha256_restored.slice(0, 16)}…`)
    })

  const onClean = () =>
    void guard('正在清理 ._ 文件…', async () => {
      const outcome = await api.clean(selected)
      await refreshStatus(selected)
      setNotice(
        outcome.failures.length
          ? `已删除 ${outcome.removed} 个，${outcome.failures.length} 个删除失败。`
          : `已删除 ${outcome.removed} 个 macOS '._' 元数据文件。`,
      )
    })

  const onCheckProcesses = () =>
    void guard('正在检查进程…', async () => {
      setProcesses(await api.runningProcesses())
    })

  const onCheckEnv = (withWine: boolean) =>
    void guard(withWine ? '正在查询 Wine 注册表…' : '正在检查环境…', async () => {
      setEnv(await api.checkEnv(withWine, BOTTLE))
    })

  const onScanLog = () =>
    void guard('正在分析游戏日志…', async () => {
      setLog(await api.scanLog(selected))
    })

  const site = report?.target?.site ?? null
  const sidecars = report?.sidecars ?? null

  return (
    <div className="app">
      <header className="topbar">
        <div>
          <h1>战舰世界 macOS 补丁工具</h1>
          <p>让《战舰世界》在 CrossOver 下正常登录。所有改动都可以一键还原。</p>
        </div>
        <div className="top-actions">
          <select
            value={selected ?? ''}
            onChange={event => onSelectInstall(event.target.value)}
            disabled={!installs.length}
          >
            {!installs.length && <option value="">未检测到安装</option>}
            {installs.map(install => (
              <option key={install.root} value={install.root}>
                {install.root}
              </option>
            ))}
          </select>
          <button className="ghost" onClick={onPickFolder} disabled={!!busy}>
            选择文件夹…
          </button>
          <button className="secondary" onClick={onRefresh} disabled={!!busy}>
            刷新
          </button>
        </div>
      </header>

      <nav className="tabs">
        {(
          [
            ['status', '补丁状态'],
            ['diagnostics', '诊断'],
            ['about', '关于'],
          ] as [Tab, string][]
        ).map(([id, label]) => (
          <button
            key={id}
            className={tab === id ? 'tab active' : 'tab'}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </nav>

      {busy && <div className="banner busy">{busy}</div>}
      {error && <div className="banner error">{error}</div>}
      {notice && <div className="banner ok">{notice}</div>}

      {tab === 'status' && (
        <>
          {!report ? (
            <div className="empty">
              <h2>没有可用的安装</h2>
              <p>请用「选择文件夹…」手动指定《战舰世界》的安装目录。</p>
            </div>
          ) : (
            <div className="grid">
              <section className="panel">
                <div className="panel-head">
                  <StateBadge state={report.state} />
                  <span className="muted">{report.state_help}</span>
                </div>

                {report.install && (
                  <>
                    <Field label="安装目录" value={report.install.root} mono />
                    <Field
                      label="构建"
                      value={
                        <>
                          {report.install.version ?? report.install.build ?? '?'}
                          <span className="muted"> · {report.install.build_source}</span>
                        </>
                      }
                    />
                  </>
                )}
                {report.target && (
                  <Field label="目标" value={report.target.name} mono />
                )}
                {report.target?.exists && (
                  <Field
                    label="SHA-256"
                    value={`${shortHash(report.target.sha256)} · ${bytes(report.target.size)}`}
                    mono
                  />
                )}
                <Field
                  label="备份"
                  value={
                    report.backup?.exists ? (
                      <span className="ok-text">{report.backup.name}（可还原）</span>
                    ) : (
                      <span className="muted">无</span>
                    )
                  }
                  mono
                />

                {report.error && <p className="error-text">{report.error}</p>}

                <div className="actions">
                  <button
                    className="primary"
                    onClick={() => setConfirming(true)}
                    disabled={!report.can_patch || !!busy}
                  >
                    打补丁
                  </button>
                  <button
                    className="secondary"
                    onClick={onRestore}
                    disabled={!report.can_restore || !!busy}
                  >
                    从备份还原
                  </button>
                  <button
                    className="ghost"
                    onClick={onClean}
                    disabled={!report.needs_clean || !!busy}
                  >
                    清理 ._ 文件{sidecars ? `（${sidecars.count}）` : ''}
                  </button>
                </div>
              </section>

              {site && (
                <section className="panel">
                  <h2>补丁位置</h2>
                  <Field label="导出函数" value={site.export_name} mono />
                  <Field
                    label="地址"
                    value={`RVA 0x${site.export_rva.toString(16)} → 文件偏移 0x${site.file_offset.toString(16)}`}
                    mono
                  />
                  <Field
                    label="所在节"
                    value={
                      <>
                        {site.section}
                        <span className={site.section_is_code ? 'ok-text' : 'warn-text'}>
                          {site.section_is_code ? ' · 可执行代码' : ' · 非可执行'}
                        </span>
                      </>
                    }
                  />
                  <Field label="当前字节" value={site.current_bytes || '（无法读取）'} mono />
                  <Field
                    label="补丁后"
                    value={`${report.patch_bytes}   ${report.patch_disasm}`}
                    mono
                  />
                  <Field
                    label="Wine 引用"
                    value={
                      site.wine_reference ?? (
                        <span className="muted">
                          未在此函数中找到证据（仅供参考，不影响打补丁）
                        </span>
                      )
                    }
                  />
                </section>
              )}

              {sidecars && (
                <section className="panel">
                  <h2>macOS 元数据文件</h2>
                  {sidecars.count === 0 ? (
                    <p className="muted">未发现 '._' 文件。</p>
                  ) : (
                    <>
                      <p>
                        共 <b>{sidecars.count.toLocaleString('zh-CN')}</b> 个
                        {sidecars.in_live_build > 0 && (
                          <span className="warn-text">
                            ，其中 {sidecars.in_live_build} 个位于当前构建内 —— 游戏会因此无法启动
                          </span>
                        )}
                      </p>
                      <ul className="folders">
                        {sidecars.by_folder.map(([label, count]) => (
                          <li key={label}>
                            <span className="mono">{label}</span>
                            <span>{count.toLocaleString('zh-CN')}</span>
                          </li>
                        ))}
                      </ul>
                    </>
                  )}
                </section>
              )}

              {report.notes.length > 0 && (
                <section className="panel wide-panel">
                  <h2>提示</h2>
                  <ul className="notes">
                    {report.notes.map((note, index) => (
                      <li key={index}>{note}</li>
                    ))}
                  </ul>
                </section>
              )}
            </div>
          )}
        </>
      )}

      {tab === 'diagnostics' && (
        <div className="grid">
          <section className="panel">
            <div className="panel-head">
              <h2>进程检查</h2>
              <button className="ghost" onClick={onCheckProcesses} disabled={!!busy}>
                检查
              </button>
            </div>
            {!processes.length ? (
              <p className="muted">未检测到游戏或启动器进程。</p>
            ) : (
              <>
                <p className="warn-text">以下进程正在运行，打补丁前建议先退出：</p>
                <ul className="notes">
                  {processes.map(name => (
                    <li key={name} className="mono">
                      {name}
                    </li>
                  ))}
                </ul>
              </>
            )}
          </section>

          <section className="panel">
            <div className="panel-head">
              <h2>环境检查</h2>
              <div className="panel-actions">
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={includeWine}
                    onChange={event => setIncludeWine(event.target.checked)}
                  />
                  查询 Wine 注册表
                </label>
                <button className="ghost" onClick={() => onCheckEnv(includeWine)} disabled={!!busy}>
                  运行检查
                </button>
              </div>
            </div>

            {!env ? (
              <p className="muted">
                尚未运行。主机名是第一嫌疑人：游戏靠它确定自己的本地 IP，
                而这正是登录失败最常见的原因。
              </p>
            ) : (
              <>
                <Field
                  label="HostName"
                  value={
                    <>
                      {env.host_name || '（未设置）'}
                      <span className={env.host_name.endsWith('.local') ? 'ok-text' : 'warn-text'}>
                        {env.host_name.endsWith('.local') ? ' · 正确' : ' · 必须以 .local 结尾'}
                      </span>
                    </>
                  }
                  mono
                />
                <Field label="LocalHostName" value={`${env.local_host_name}（不要修改）`} mono />
                <Field
                  label="解析结果"
                  value={
                    env.resolved_address ?? <span className="warn-text">无法解析 —— 这就是根因</span>
                  }
                  mono
                />
                {includeWine && (
                  <Field
                    label="Wine 注册表"
                    value={env.wine_registry ?? <span className="muted">查询失败或未找到 CrossOver</span>}
                    mono
                  />
                )}
                <Field
                  label="出站网卡"
                  value={
                    <>
                      {env.egress_interface ?? '未知'}
                      {env.vpn_warning && (
                        <span className="warn-text"> · 正在走 VPN 隧道</span>
                      )}
                    </>
                  }
                  mono
                />

                {env.fix_command && (
                  <div className="fix-card">
                    <p>在终端执行下面这一条即可修复：</p>
                    <code>{env.fix_command}</code>
                    <div className="actions">
                      <button
                        className="secondary"
                        onClick={() => void navigator.clipboard.writeText(env.fix_command ?? '')}
                      >
                        复制命令
                      </button>
                      <button
                        className="ghost"
                        onClick={() => void api.openTerminal(env.fix_command ?? '')}
                      >
                        在终端中打开
                      </button>
                    </div>
                  </div>
                )}

                {env.notes.length > 0 && (
                  <ul className="notes">
                    {env.notes.map((note, index) => (
                      <li key={index}>{note}</li>
                    ))}
                  </ul>
                )}

                <details className="dead-ends">
                  <summary>不要尝试这两种做法（已验证无效）</summary>
                  <ul className="notes">
                    {env.dead_ends.map((item, index) => (
                      <li key={index}>{item}</li>
                    ))}
                  </ul>
                </details>
              </>
            )}
          </section>

          <section className="panel wide-panel">
            <div className="panel-head">
              <h2>游戏日志诊断</h2>
              <button className="ghost" onClick={onScanLog} disabled={!!busy}>
                分析日志
              </button>
            </div>

            {!log ? (
              <p className="muted">尚未分析。逐会话统计网络错误，用于确认主机名修复是否真的生效。</p>
            ) : !log.exists ? (
              <p className="muted">找不到日志：{log.path}</p>
            ) : (
              <>
                <p className="muted mono">
                  {log.path} · {bytes(log.size)}
                  {log.modified && ` · 最后写入 ${log.modified}`}
                </p>
                {log.sessions.length === 0 ? (
                  <p className="muted">日志中还没有会话记录。</p>
                ) : (
                  <table className="table">
                    <thead>
                      <tr>
                        <th>启动</th>
                        <th>ERROR</th>
                        <th>本地地址失败</th>
                        <th>判定</th>
                      </tr>
                    </thead>
                    <tbody>
                      {log.sessions.map((session, index) => (
                        <tr key={index}>
                          <td className="mono">{session.started}</td>
                          <td>{session.errors}</td>
                          <td>{session.local_address_failures}</td>
                          <td className={session.healthy ? 'ok-text' : 'warn-text'}>
                            {session.verdict}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
                <p className="muted">最下面一行是最近一次启动。</p>
              </>
            )}
          </section>
        </div>
      )}

      {tab === 'about' && (
        <div className="grid">
          <section className="panel">
            <h2>关于</h2>
            <p>
              Rust 重写版。补丁偏移每次运行都从 DLL 的导出表重新解析，从不硬编码，
              因此游戏更新后不会写坏文件。
            </p>
            <Field label="状态文件" value={statePath} mono />
            <div className="actions">
              <button
                className="ghost"
                onClick={() => void api.revealInFinder(statePath)}
                disabled={!statePath}
              >
                在 Finder 中显示
              </button>
            </div>
            <p className="muted">
              记录每次补丁的偏移、哈希与时间戳；还原时据此核对备份是否仍属于当前构建。
            </p>
          </section>
        </div>
      )}

      {confirming && report?.target && site && (
        <div className="overlay" onClick={() => setConfirming(false)}>
          <div className="modal" onClick={event => event.stopPropagation()}>
            <h2>确认打补丁</h2>
            <Field label="修改" value={report.target.path} mono />
            <Field
              label="偏移"
              value={`0x${site.file_offset.toString(16).toUpperCase()}（3 字节）`}
              mono
            />
            <pre className="diff">
              <div>
                <span className="muted">before </span>
                {site.current_bytes}
              </div>
              <div>
                <span className="muted">after  </span>
                {report.patch_bytes}
                <span className="muted">   {report.patch_disasm}</span>
              </div>
            </pre>
            {report.backup && (
              <Field label="备份" value={`→ ${report.backup.name}`} mono />
            )}
            {sidecars && sidecars.count > 0 && (
              <Field label="清理" value={`同时删除 ${sidecars.count} 个 '._' 元数据文件`} />
            )}
            <p className="warn-text">
              请先退出游戏和它的启动器（Steam 或 Wargaming Game Center）。
            </p>
            <div className="actions end">
              <button className="ghost" onClick={() => setConfirming(false)}>
                取消
              </button>
              <button className="primary" onClick={onPatch} disabled={!!busy}>
                确认打补丁
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
