import { useCallback, useEffect, useState } from 'react'
import * as api from './api'
import { useI18n } from './I18nProvider'
import type { MessageKey } from './locales'
import type { EnvReport, InstallDto, LogReport, PatchState, ReportDto } from './types'

const BOTTLE = 'Steam'

type Tab = 'status' | 'diagnostics' | 'about'

const STATE_KEY: Record<PatchState, MessageKey> = {
  ORIGINAL: 'stateOriginal',
  PATCHED: 'statePatched',
  FOREIGN: 'stateForeign',
  UNRESOLVED: 'stateUnresolved',
}

function Field({ label, value, mono }: { label: string; value: React.ReactNode; mono?: boolean }) {
  return (
    <div className="field">
      <span className="field-label">{label}</span>
      <span className={mono ? 'field-value mono' : 'field-value'}>{value}</span>
    </div>
  )
}

function StateBadge({ state }: { state: PatchState }) {
  const { t } = useI18n()
  return (
    <span className={`badge badge-${state.toLowerCase()}`}>
      <b>{t(STATE_KEY[state])}</b>
    </span>
  )
}

export default function App() {
  const { locale, setLocale, t, formatNumber } = useI18n()
  const [tab, setTab] = useState<Tab>('status')
  const [installs, setInstalls] = useState<InstallDto[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [report, setReport] = useState<ReportDto | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [confirming, setConfirming] = useState(false)

  const [processes, setProcesses] = useState<string[] | null>(null)
  const [env, setEnv] = useState<EnvReport | null>(null)
  const [includeWine, setIncludeWine] = useState(false)
  const [log, setLog] = useState<LogReport | null>(null)
  const [statePath, setStatePath] = useState('')

  const guard = useCallback(async (label: string, action: () => Promise<void>) => {
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
  }, [])

  const refreshStatus = useCallback(async (root: string | null) => {
    const next = await api.status(root)
    setReport(next)
    if (next.install?.root) setSelected(next.install.root)
  }, [])

  const loadInstalls = useCallback(async () => {
    const found = await api.listInstalls()
    setInstalls(found)
    return found
  }, [])

  useEffect(() => {
    void guard(t('busyDetect'), async () => {
      await loadInstalls()
      await refreshStatus(null)
    })
    void api.statePath().then(setStatePath).catch(() => {})
    // Initial load only — do not re-run when locale/t changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [guard, loadInstalls, refreshStatus])

  const onSelectInstall = (root: string) =>
    void guard(t('busyStatus'), () => refreshStatus(root))

  const onPickFolder = () =>
    void guard(t('busyStatus'), async () => {
      const picked = await api.pickFolder(t('pickFolderPrompt'))
      if (!picked) return
      const install = await api.resolveInstall(picked)
      setSelected(install.root)
      await refreshStatus(install.root)
    })

  const onRefresh = () => void guard(t('busyStatus'), () => refreshStatus(selected))

  const onPatch = () =>
    void guard(t('busyPatch'), async () => {
      setConfirming(false)
      const outcome = await api.patch(selected, report?.target?.sha256 ?? '')
      await refreshStatus(selected)
      setNotice(t('noticePatched', { path: outcome.backup }))
    })

  const onRestore = () =>
    void guard(t('busyRestore'), async () => {
      const outcome = await api.restore(selected)
      await refreshStatus(selected)
      setNotice(t('noticeRestored', { hash: outcome.sha256_restored.slice(0, 16) }))
    })

  const onClean = () =>
    void guard(t('busyClean'), async () => {
      const outcome = await api.clean(selected)
      await refreshStatus(selected)
      setNotice(
        outcome.failures.length
          ? t('noticeCleanPartial', {
              removed: outcome.removed,
              failed: outcome.failures.length,
            })
          : t('noticeCleaned', { count: outcome.removed }),
      )
    })

  const onCheckProcesses = () =>
    void guard(t('busyProcesses'), async () => {
      setProcesses(await api.runningProcesses())
    })

  const onCheckEnv = (withWine: boolean) =>
    void guard(withWine ? t('busyWine') : t('busyEnv'), async () => {
      setEnv(await api.checkEnv(withWine, BOTTLE))
    })

  const onScanLog = () =>
    void guard(t('busyLog'), async () => {
      setLog(await api.scanLog(selected))
    })

  const site = report?.target?.site ?? null
  const sidecars = report?.sidecars ?? null
  const showStateHelp =
    report && (report.state === 'FOREIGN' || report.state === 'UNRESOLVED')

  const envDetailNotes = (reportEnv: EnvReport): string[] => {
    const notes: string[] = []
    if (!reportEnv.host_name.endsWith('.local')) {
      if (!reportEnv.local_host_name) notes.push(t('envNoteNoLocalHost'))
      else notes.push(t('envNoteNotLocal'))
    } else if (!reportEnv.resolved_address) {
      notes.push(t('envNoteNoResolve', { host: reportEnv.unix_hostname || reportEnv.host_name }))
    }
    notes.push(t('envDeadHosts'), t('envDeadWine'))
    return notes
  }

  const formatBytes = (size: number) =>
    size ? t('bytes', { n: formatNumber(size) }) : '—'

  return (
    <div className="app">
      <header className="topbar">
        <h1>{t('appTitle')}</h1>
        <div className="top-actions">
          <div className="lang-switch" role="group" aria-label="Language">
            <button
              type="button"
              className={locale === 'zh' ? 'lang active' : 'lang'}
              onClick={() => setLocale('zh')}
            >
              {t('langZh')}
            </button>
            <button
              type="button"
              className={locale === 'en' ? 'lang active' : 'lang'}
              onClick={() => setLocale('en')}
            >
              {t('langEn')}
            </button>
          </div>
          <select
            value={selected ?? ''}
            onChange={event => onSelectInstall(event.target.value)}
            disabled={!installs.length}
          >
            {!installs.length && <option value="">{t('noInstallOption')}</option>}
            {installs.map(install => (
              <option key={install.root} value={install.root}>
                {install.root}
              </option>
            ))}
          </select>
          <button className="ghost" onClick={onPickFolder} disabled={!!busy}>
            {t('pickFolder')}
          </button>
          <button className="secondary" onClick={onRefresh} disabled={!!busy}>
            {t('refresh')}
          </button>
        </div>
      </header>

      <nav className="tabs">
        {(
          [
            ['status', 'tabStatus'],
            ['diagnostics', 'tabDiagnostics'],
            ['about', 'tabAbout'],
          ] as [Tab, MessageKey][]
        ).map(([id, labelKey]) => (
          <button
            key={id}
            className={tab === id ? 'tab active' : 'tab'}
            onClick={() => setTab(id)}
          >
            {t(labelKey)}
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
              <h2>{t('emptyTitle')}</h2>
              <p>{t('emptyBody')}</p>
            </div>
          ) : (
            <div className="grid">
              <section className="panel">
                <div className="panel-head">
                  <StateBadge state={report.state} />
                  {showStateHelp && (
                    <span className="error-text">
                      {t(report.state === 'FOREIGN' ? 'stateHelpForeign' : 'stateHelpUnresolved')}
                    </span>
                  )}
                </div>

                {report.install && (
                  <>
                    <Field label={t('fieldInstall')} value={report.install.root} mono />
                    <Field
                      label={t('fieldBuild')}
                      value={report.install.version ?? report.install.build ?? '?'}
                    />
                  </>
                )}
                <Field
                  label={t('fieldBackup')}
                  value={
                    report.backup?.exists ? (
                      <span className="ok-text">{t('backupYes')}</span>
                    ) : (
                      <span className="muted">{t('backupNo')}</span>
                    )
                  }
                />

                {report.error && <p className="error-text">{report.error}</p>}

                <div className="actions">
                  <button
                    className="primary"
                    onClick={() => setConfirming(true)}
                    disabled={!report.can_patch || !!busy}
                  >
                    {t('actionPatch')}
                  </button>
                  <button
                    className="secondary"
                    onClick={onRestore}
                    disabled={!report.can_restore || !!busy}
                  >
                    {t('actionRestore')}
                  </button>
                  {report.needs_clean && (
                    <button className="ghost" onClick={onClean} disabled={!!busy}>
                      {t('actionClean', { count: sidecars?.count ?? 0 })}
                    </button>
                  )}
                </div>
              </section>

              {sidecars && sidecars.count > 0 && (
                <section className="panel">
                  <h2>{t('sidecarsTitle')}</h2>
                  <p>
                    {t('sidecarsCount', { count: formatNumber(sidecars.count) })}
                    {sidecars.in_live_build > 0 && (
                      <span className="warn-text">
                        {t('sidecarsLive', { count: formatNumber(sidecars.in_live_build) })}
                      </span>
                    )}
                  </p>
                  <ul className="folders">
                    {sidecars.by_folder.map(([label, count]) => (
                      <li key={label}>
                        <span className="mono">{label}</span>
                        <span>{formatNumber(count)}</span>
                      </li>
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
              <h2>{t('procTitle')}</h2>
              <button className="ghost" onClick={onCheckProcesses} disabled={!!busy}>
                {t('procCheck')}
              </button>
            </div>
            {processes === null ? (
              <p className="muted">{t('procIdle')}</p>
            ) : processes.length === 0 ? (
              <p className="ok-text">{t('procNone')}</p>
            ) : (
              <>
                <p className="warn-text">{t('procWarn')}</p>
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
              <h2>{t('envTitle')}</h2>
              <div className="panel-actions">
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={includeWine}
                    onChange={event => setIncludeWine(event.target.checked)}
                  />
                  {t('envWine')}
                </label>
                <button className="ghost" onClick={() => onCheckEnv(includeWine)} disabled={!!busy}>
                  {t('envRun')}
                </button>
              </div>
            </div>

            {!env ? (
              <p className="muted">{t('envIdle')}</p>
            ) : env.hostname_ok ? (
              <>
                <p className="ok-text">{t('envOk')}</p>
                <Field label="HostName" value={env.host_name} mono />
                {env.egress_interface && (
                  <Field
                    label={t('envEgress')}
                    value={
                      <>
                        {env.egress_interface}
                        {env.vpn_warning && <span className="warn-text">{t('envVpn')}</span>}
                      </>
                    }
                    mono
                  />
                )}
                {includeWine && env.wine_registry && (
                  <Field label={t('envWineReg')} value={env.wine_registry} mono />
                )}
              </>
            ) : (
              <>
                <Field
                  label="HostName"
                  value={
                    <>
                      {env.host_name || t('envUnset')}
                      <span className="warn-text">
                        {env.host_name.endsWith('.local') ? '' : t('envMustLocal')}
                      </span>
                    </>
                  }
                  mono
                />
                <Field
                  label={t('envResolved')}
                  value={
                    env.resolved_address ?? <span className="warn-text">{t('envUnresolved')}</span>
                  }
                  mono
                />
                {env.egress_interface && (
                  <Field
                    label={t('envEgress')}
                    value={
                      <>
                        {env.egress_interface}
                        {env.vpn_warning && <span className="warn-text">{t('envVpn')}</span>}
                      </>
                    }
                    mono
                  />
                )}
                {includeWine && (
                  <Field
                    label={t('envWineReg')}
                    value={env.wine_registry ?? <span className="muted">{t('envWineMissing')}</span>}
                    mono
                  />
                )}

                {env.fix_command && (
                  <div className="fix-card">
                    <p>{t('envFix')}</p>
                    <code>{env.fix_command}</code>
                    <div className="actions">
                      <button
                        className="secondary"
                        onClick={() => void navigator.clipboard.writeText(env.fix_command ?? '')}
                      >
                        {t('envCopy')}
                      </button>
                      <button
                        className="ghost"
                        onClick={() => void api.openTerminal(env.fix_command ?? '')}
                      >
                        {t('envOpenTerminal')}
                      </button>
                    </div>
                  </div>
                )}

                <details className="dead-ends">
                  <summary>{t('envDetails')}</summary>
                  <ul className="notes">
                    {envDetailNotes(env).map((item, index) => (
                      <li key={index}>{item}</li>
                    ))}
                  </ul>
                </details>
              </>
            )}
          </section>

          <section className="panel wide-panel">
            <div className="panel-head">
              <h2>{t('logTitle')}</h2>
              <button className="ghost" onClick={onScanLog} disabled={!!busy}>
                {t('logScan')}
              </button>
            </div>

            {!log ? (
              <p className="muted">{t('logIdle')}</p>
            ) : !log.exists ? (
              <p className="muted">{t('logMissing', { path: log.path })}</p>
            ) : (
              <>
                <p className="muted mono">
                  {log.path} · {formatBytes(log.size)}
                  {log.modified && ` · ${log.modified}`}
                </p>
                {log.sessions.length === 0 ? (
                  <p className="muted">{t('logEmpty')}</p>
                ) : (
                  <table className="table">
                    <thead>
                      <tr>
                        <th>{t('logColStart')}</th>
                        <th>{t('logColErrors')}</th>
                        <th>{t('logColLocal')}</th>
                        <th>{t('logColVerdict')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {log.sessions.map((session, index) => (
                        <tr key={index}>
                          <td className="mono">{session.started}</td>
                          <td>{session.errors}</td>
                          <td>{session.local_address_failures}</td>
                          <td className={session.healthy ? 'ok-text' : 'warn-text'}>
                            {session.healthy ? t('logHealthy') : t('logUnhealthy')}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </>
            )}
          </section>
        </div>
      )}

      {tab === 'about' && (
        <div className="grid">
          <section className="panel">
            <h2>{t('aboutTitle')}</h2>
            <p>{t('aboutBody')}</p>
            <Field label={t('aboutStateFile')} value={statePath || '—'} mono />
            <div className="actions">
              <button
                className="ghost"
                onClick={() => void api.revealInFinder(statePath)}
                disabled={!statePath}
              >
                {t('aboutReveal')}
              </button>
            </div>
          </section>
        </div>
      )}

      {confirming && report?.target && site && (
        <div className="overlay" onClick={() => setConfirming(false)}>
          <div className="modal" onClick={event => event.stopPropagation()}>
            <h2>{t('confirmTitle')}</h2>
            <Field label={t('confirmFile')} value={report.target.name} mono />
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
              <Field label={t('confirmBackup')} value={`→ ${report.backup.name}`} mono />
            )}
            {sidecars && sidecars.count > 0 && (
              <Field
                label={t('confirmCleanLabel')}
                value={t('confirmClean', { count: sidecars.count })}
              />
            )}
            <p className="warn-text">{t('confirmWarn')}</p>
            <div className="actions end">
              <button className="ghost" onClick={() => setConfirming(false)}>
                {t('confirmCancel')}
              </button>
              <button className="primary" onClick={onPatch} disabled={!!busy}>
                {t('confirmOk')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
