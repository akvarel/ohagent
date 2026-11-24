import { useEffect, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import { Link, NavLink, Navigate, Route, Routes, useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { beginLogin, clearSession, finishLogin, readSession } from './auth'
import { cancelRun, downloadArtifact, finishStartRequest, getConfig, listProcesses, listRuns, saveProcess, startRequestKey, startRun } from './api'
import { APP_ROOT, terminal } from './protocol'
import { asClientError, ClientError } from './errors'
import { statusText, useLanguage } from './i18n'
import type { Language, Translate } from './i18n'
import type { Process, ProcessInput, PublicConfig, Run } from './types'
import { useRunJournal } from './useRunJournal'
import Universe from './Universe'
import './aaas.css'

export function ErrorPanel({ error, t, retry }: { error: ClientError | null; t: Translate; retry?: () => void }) {
  if (!error) return null
  return <div className="aaas-error" role="alert"><strong>{t(error.key)}</strong>{(error.code || error.status > 0) && <p><span>{t('errorCode')}: </span><code>{error.status || ''} {error.code}</code></p>}{retry && <button onClick={retry}>{t('retry')}</button>}</div>
}
function Loading({ t }: { t: Translate }) { return <p className="aaas-loading" role="status">{t('loading')}</p> }
function Status({ value, t }: { value: string; t: Translate }) { return <span className={`aaas-badge aaas-status-${value}`}>{statusText(value, t)}</span> }
function Callback({ config, t }: { config: PublicConfig; t: Translate }) {
  const navigate = useNavigate()
  const [error, setError] = useState<ClientError | null>(null)
  useEffect(() => {
    let active = true
    if (!config.auth) { setError(new ClientError('authUnavailable')); return }
    void finishLogin(config.auth).then(route => { if (active) navigate(route, { replace: true }) }).catch(issue => { if (active) setError(asClientError(issue)) })
    return () => { active = false }
  }, [config, navigate])
  return error ? <><ErrorPanel error={error} t={t} /><Link className="aaas-button" to={`${APP_ROOT}/processes`}>{t('back')}</Link></> : <Loading t={t} />
}
function Processes({ t }: { t: Translate }) {
  const [processes, setProcesses] = useState<Process[] | null>(null)
  const [error, setError] = useState<ClientError | null>(null)
  const [revision, setRevision] = useState(0)
  useEffect(() => {
    const controller = new AbortController()
    setError(null); setProcesses(null)
    void listProcesses(controller.signal).then(data => { if (!controller.signal.aborted) { if (!Array.isArray(data.processes)) throw new ClientError('protocol'); setProcesses(data.processes) } }).catch(issue => { if (!controller.signal.aborted) setError(asClientError(issue)) })
    return () => controller.abort()
  }, [revision])
  return <><div className="aaas-page-heading"><div><p className="aaas-eyebrow">{t('brief')}</p><h1>{t('processes')}</h1><p>{t('savedProcesses')}</p></div><Link className="aaas-button aaas-primary" to={`${APP_ROOT}/new`}>{t('new')} +</Link></div><ErrorPanel error={error} t={t} retry={() => setRevision(value => value + 1)} />{!processes && !error && <Loading t={t} />}{processes?.length === 0 && <div className="aaas-empty">{t('emptyProcesses')}</div>}<div className="aaas-cards">{processes?.map(process => <Link key={process.id} className="aaas-card" to={`${APP_ROOT}/processes/${encodeURIComponent(process.id)}`}><span className="aaas-eyebrow">{t('version')} {process.version}</span><h2>{process.title}</h2><p>{process.goal}</p><span className="aaas-card-footer">{t('edit')} <span aria-hidden="true">↗</span></span></Link>)}</div></>
}
const blank: ProcessInput = { title: '', goal: '', expected_result: '', materials: '' }
const inputOf = (process: Process): ProcessInput => ({ title: process.title, goal: process.goal, expected_result: process.expected_result, materials: process.materials })
function ProcessEditor({ t, pilot }: { t: Translate; pilot: boolean }) {
  const { id } = useParams()
  const navigate = useNavigate()
  const [process, setProcess] = useState<Process | null>(null)
  const [input, setInput] = useState<ProcessInput>(blank)
  const [loading, setLoading] = useState(!!id)
  const [busy, setBusy] = useState(false)
  const busyRef = useRef(false)
  const [error, setError] = useState<ClientError | null>(null)
  const [key, setKey] = useState('')
  const [revision, setRevision] = useState(0)
  useEffect(() => {
    const controller = new AbortController()
    setProcess(null); setInput(blank); setKey(''); setError(null)
    if (!id) { setLoading(false); return }
    setLoading(true)
    void listProcesses(controller.signal).then(data => {
      if (controller.signal.aborted) return
      if (!Array.isArray(data.processes)) throw new ClientError('protocol')
      const found = data.processes.find(item => item.id === id)
      if (!found) throw new ClientError('notFound')
      setProcess(found); setInput(inputOf(found)); setLoading(false)
    }).catch(issue => { if (!controller.signal.aborted) { setError(asClientError(issue)); setLoading(false) } })
    return () => controller.abort()
  }, [id, revision])
  const dirty = !process || JSON.stringify(inputOf(process)) !== JSON.stringify(input)
  const save = async (event: FormEvent) => {
    event.preventDefault()
    if (busyRef.current) return
    if (Object.values(input).some(value => !value.trim())) { setError(new ClientError('invalidInput')); return }
    busyRef.current = true; setBusy(true); setError(null)
    try {
      const saved = await saveProcess(input, id)
      if (!saved.id || !Number.isSafeInteger(saved.version)) throw new ClientError('protocol')
      setProcess(saved); setInput(inputOf(saved)); setKey('')
      if (!id) navigate(`${APP_ROOT}/processes/${encodeURIComponent(saved.id)}`, { replace: true })
    } catch (issue) { setError(asClientError(issue)) } finally { busyRef.current = false; setBusy(false) }
  }
  const start = async () => {
    if (!process || dirty || !pilot || busyRef.current) return
    busyRef.current = true; setBusy(true); setError(null)
    try {
      const requestKey = startRequestKey(process)
      setKey(requestKey)
      const run = await startRun(process, requestKey)
      if (!run.id || run.process_id !== process.id || run.process_version !== process.version) throw new ClientError('protocol')
      finishStartRequest(process)
      navigate(`${APP_ROOT}/runs/${encodeURIComponent(run.id)}`)
    } catch (issue) { setError(asClientError(issue)) } finally { busyRef.current = false; setBusy(false) }
  }
  if (loading) return <Loading t={t} />
  return <><Link className="aaas-back" to={`${APP_ROOT}/processes`}>← {t('processes')}</Link><div className="aaas-page-heading"><div><p className="aaas-eyebrow">{t('brief')}</p><h1>{t(id ? 'edit' : 'new')}</h1><p>{t('limits')}</p></div>{process && <span className="aaas-badge">{t('version')} {process.version}</span>}</div><ErrorPanel error={error} t={t} retry={id && !process ? () => setRevision(value => value + 1) : undefined} />
    {(!id || process) && <div className="aaas-editor-layout"><form className="aaas-form" onSubmit={save}>
      <label><span>{t('title')}</span><input required maxLength={200} value={input.title} onChange={event => setInput(previous => ({ ...previous, title: event.target.value }))} disabled={busy} /></label>
      <label><span>{t('goal')}</span><textarea required maxLength={10000} rows={3} value={input.goal} onChange={event => setInput(previous => ({ ...previous, goal: event.target.value }))} disabled={busy} /></label>
      <label><span>{t('expected')}</span><textarea required maxLength={10000} rows={3} value={input.expected_result} onChange={event => setInput(previous => ({ ...previous, expected_result: event.target.value }))} disabled={busy} /></label>
      <label><span>{t('materials')}</span><textarea required maxLength={100000} rows={10} value={input.materials} onChange={event => setInput(previous => ({ ...previous, materials: event.target.value }))} disabled={busy} aria-describedby="aaas-materials-hint" /><small id="aaas-materials-hint">{t('materialsHint')} ({input.materials.length.toLocaleString()})</small></label>
      <div className="aaas-actions"><button type="submit" className="aaas-primary" disabled={busy || !dirty}>{busy ? t('loading') : t('save')}</button>{process && !dirty && <span role="status">✓ {t('saved')}</span>}</div>
    </form><aside className="aaas-start-panel"><h2>{t('start')}</h2><p>{t('startNotice')}</p>{!pilot && <p className="aaas-notice">{t('pilotUnavailable')}</p>}{dirty && <p>{t('saveFirst')}</p>}<button className="aaas-primary" onClick={() => void start()} disabled={busy || dirty || !process || !pilot}>{busy ? t('loading') : t(key ? 'startRetry' : 'start')} →</button>{key && <p className="aaas-request-key">{t('requestId')}: <code>{key}</code></p>}</aside></div>}
  </>
}
function Runs({ t, language }: { t: Translate; language: Language }) {
  const [runs, setRuns] = useState<Run[] | null>(null)
  const [error, setError] = useState<ClientError | null>(null)
  const [revision, setRevision] = useState(0)
  useEffect(() => {
    const controller = new AbortController()
    setError(null)
    void listRuns(controller.signal).then(data => { if (!controller.signal.aborted) { if (!Array.isArray(data.runs)) throw new ClientError('protocol'); setRuns(data.runs) } }).catch(issue => { if (!controller.signal.aborted) setError(asClientError(issue)) })
    return () => controller.abort()
  }, [revision])
  return <><div className="aaas-page-heading"><h1>{t('runs')}</h1><button onClick={() => setRevision(value => value + 1)}>{t('refresh')}</button></div><ErrorPanel error={error} t={t} retry={() => setRevision(value => value + 1)} />{!runs && !error && <Loading t={t} />}{runs?.length === 0 && <div className="aaas-empty">{t('emptyRuns')}</div>}<div className="aaas-run-list">{runs?.map(run => <Link key={run.id} to={`${APP_ROOT}/runs/${encodeURIComponent(run.id)}`} className="aaas-run-row"><div><h2>{run.title}</h2><small>{formatTime(run.created_at, language)}</small></div><Status value={run.status} t={t} /><span className="aaas-muted">{t('tokens')}: {usage(run.usage?.tokens, t)}</span><span aria-hidden="true">↗</span></Link>)}</div></>
}
const usage = (value: number | null | undefined, t: Translate) => typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value.toLocaleString() : t('unknown')
const formatTime = (value: string, language: Language) => Number.isFinite(Date.parse(value)) ? new Date(value).toLocaleString(language) : '…'
function useMotion() {
  const [manual, setManual] = useState(() => { try { return localStorage.getItem('ohagent-aaas-reduced-motion') === 'true' } catch { return false } })
  const [os, setOs] = useState(() => window.matchMedia('(prefers-reduced-motion: reduce)').matches)
  useEffect(() => { const query = window.matchMedia('(prefers-reduced-motion: reduce)'); const change = () => setOs(query.matches); query.addEventListener('change', change); return () => query.removeEventListener('change', change) }, [])
  return { reduced: manual || os, manual, os, change: (value: boolean) => { setManual(value); try { localStorage.setItem('ohagent-aaas-reduced-motion', String(value)) } catch {} } }
}
function RunDetail({ t, language }: { t: Translate; language: Language }) {
  const { id = '' } = useParams()
  const [params, setParams] = useSearchParams()
  const replay = params.get('mode') === 'replay'
  const [revision, setRevision] = useState(0)
  const { journal, error, transport, pulses, applySnapshot } = useRunJournal(id, replay, revision)
  const [actionError, setActionError] = useState<ClientError | null>(null)
  const [busy, setBusy] = useState(false)
  const busyRef = useRef(false)
  const motion = useMotion()
  const [position, setPosition] = useState<number | null>(null)
  const run = journal.snapshot
  const mode = (value: boolean) => { setPosition(null); setActionError(null); setParams(value ? { mode: 'replay' } : {}) }
  const action = async (callback: () => Promise<void>) => { if (busyRef.current) return; busyRef.current = true; setBusy(true); setActionError(null); try { await callback() } catch (issue) { setActionError(asClientError(issue)) } finally { busyRef.current = false; setBusy(false) } }
  const events = replay && position !== null ? journal.events.filter(event => event.run_seq <= position) : journal.events
  return <><Link className="aaas-back" to={`${APP_ROOT}/runs`}>← {t('runs')}</Link><div className="aaas-page-heading"><div><p className="aaas-eyebrow">{t(replay ? 'replay' : 'live')}</p><h1>{run?.title || t('loading')}</h1>{run && <p>{run.goal}</p>}</div><div className="aaas-mode" role="group" aria-label={t('status')}><button aria-pressed={!replay} onClick={() => mode(false)}>{t('live')}</button><button aria-pressed={replay} onClick={() => mode(true)}>{t('replay')}</button></div></div>
    {replay && <div className="aaas-notice">{t('replayHelp')}</div>}<ErrorPanel error={error} t={t} retry={() => setRevision(value => value + 1)} /><ErrorPanel error={actionError} t={t} />
    <div className="aaas-transport" role="status"><span className={`aaas-connection-dot ${transport === 'connected' ? 'aaas-is-connected' : ''}`} />{t('transport')}: {t(transport)}</div>
    {!run && !error && <Loading t={t} />}{run && <>
      <div className="aaas-run-summary"><Status value={run.status} t={t} /><dl><div><dt>{t('tokens')}</dt><dd>{usage(run.usage?.tokens, t)}</dd></div><div><dt>{t('duration')}</dt><dd>{usage(run.usage?.duration_ms, t)}</dd></div><div><dt>{t('version')}</dt><dd>{run.version}</dd></div><div><dt>{t('snapshot')}</dt><dd>{run.watermark}</dd></div></dl>
        {!replay && !terminal(run.status) && <button className="aaas-danger" disabled={busy || run.status === 'cancellation_requested'} onClick={() => void action(async () => { applySnapshot(await cancelRun(run)) })}>{t(run.status === 'cancellation_requested' ? 'cancellation_requested' : 'cancel')}</button>}
      </div>
      <div className="aaas-motion"><label><input type="checkbox" checked={motion.manual} onChange={event => motion.change(event.target.checked)} />{t('reduceMotion')}</label>{motion.os && <small>{t('osMotion')}</small>}</div>
      <Universe run={run} pulses={replay ? [] : pulses} reduced={motion.reduced} t={t} replay={replay} />
      {run.artifact && <section className="aaas-artifact"><div><h2>{t('artifact')}</h2><p>{run.artifact.verification === true || run.artifact.verification === 'passed' ? t('verified') : t('verificationPending')}</p></div><button disabled={busy} className="aaas-primary" onClick={() => void action(() => downloadArtifact(run))}>{t('download')} ↓</button></section>}
      <section className="aaas-journal"><div className="aaas-section-heading"><h2>{t('feed')}</h2><span className="aaas-badge">{t(replay ? 'readOnly' : 'live')}</span></div><p className="aaas-muted">{t('feedBound')}</p>
        {replay && journal.events.length > 0 && <label className="aaas-replay-range"><span>{t('replayPosition')}: {position ?? journal.events[journal.events.length - 1].run_seq}</span><input type="range" min={journal.events[0].run_seq} max={journal.events[journal.events.length - 1].run_seq} value={position ?? journal.events[journal.events.length - 1].run_seq} onChange={event => setPosition(Number(event.target.value))} /><small>{t('replayWindow')}</small></label>}
        {events.length === 0 ? <p>{t('noEvents')}</p> : <ol className="aaas-events">{events.slice().reverse().map(event => <li key={event.event_id}><span className="aaas-event-seq">#{event.run_seq}</span><div><strong>{event.type.match(/(?:^|[._])(?:edge_)?dispatched$/) ? t('dispatch') : t('event')}</strong><small>{event.task_id && <span>{t('stages')}: <code>{event.task_id}</code> </span>}{event.agent_instance_id && <span>{t('instance')}: <code>{event.agent_instance_id}</code></span>}</small></div><time dateTime={event.recorded_at}>{formatTime(event.recorded_at, language)}</time></li>)}</ol>}
      </section>
    </>}
  </>
}
export default function AaasApp() {
  const { language, change, t } = useLanguage()
  const [config, setConfig] = useState<PublicConfig | null>(null)
  const [configError, setConfigError] = useState<ClientError | null>(null)
  const [loginError, setLoginError] = useState<ClientError | null>(null)
  const [revision, setRevision] = useState(0)
  const [sessionVersion, setSessionVersion] = useState(0)
  const [signingIn, setSigningIn] = useState(false)
  const [signedOut, setSignedOut] = useState(false)
  const location = useLocation()
  useEffect(() => { const update = () => setSessionVersion(value => value + 1); window.addEventListener('ohagent-aaas-session', update); const timer = setInterval(update, 15000); return () => { window.removeEventListener('ohagent-aaas-session', update); clearInterval(timer) } }, [])
  useEffect(() => {
    const controller = new AbortController()
    setConfigError(null); setConfig(null)
    void getConfig(controller.signal).then(value => { if (!controller.signal.aborted) { if (!value || typeof value.pilot_available !== 'boolean' || (value.auth && (!value.auth.issuer || !value.auth.client_id || !value.auth.authorization_endpoint || !value.auth.token_endpoint))) throw new ClientError('protocol'); setConfig(value) } }).catch(issue => { if (!controller.signal.aborted) setConfigError(asClientError(issue)) })
    return () => controller.abort()
  }, [revision])
  const session = config?.auth ? readSession(config.auth) : null
  const callback = location.pathname.replace(/\/$/, '') === `${APP_ROOT}/callback`
  const login = async () => { if (!config?.auth || signingIn) return; setSigningIn(true); setLoginError(null); try { await beginLogin(config.auth, `${location.pathname}${location.search}`) } catch (issue) { setLoginError(asClientError(issue)); setSigningIn(false) } }
  return <div className="aaas-client" lang={language} data-session-version={sessionVersion}>
    <header className="aaas-header"><Link className="aaas-brand" to="/agent-as-a-service"><span className="aaas-brand-mark" aria-hidden="true">◈</span><span>OrangeHat<small>{t('brand')}</small></span></Link><nav aria-label={t('brand')}>{session && <><NavLink to={`${APP_ROOT}/processes`}>{t('processes')}</NavLink><NavLink to={`${APP_ROOT}/runs`}>{t('runs')}</NavLink></>}</nav><div className="aaas-header-controls"><label><span className="aaas-sr-only">{t('language')}</span><select value={language} onChange={event => change(event.target.value as Language)}><option value="en">EN</option><option value="lv">LV</option><option value="ru">RU</option></select></label>{session && <button onClick={() => { clearSession(); setSignedOut(true) }}>{t('logout')}</button>}</div></header>
    <main className="aaas-main"><ErrorPanel error={configError} t={t} retry={() => setRevision(value => value + 1)} />{!config && !configError && <Loading t={t} />}
      {config && (callback ? <Callback config={config} t={t} /> : !config.auth ? <section className="aaas-auth"><p className="aaas-eyebrow">{t('notConfigured')}</p><h1>{t('unavailable')}</h1><p>{t('authUnavailable')}</p><button onClick={() => setRevision(value => value + 1)}>{t('retry')}</button></section> : !session ? <section className="aaas-auth"><p className="aaas-eyebrow">OrangeHat / {t('brand')}</p><h1>{t('login')}</h1><p>{t('authIntro')}</p>{signedOut && <p role="status">{t('localSignout')}</p>}<ErrorPanel error={loginError} t={t} /><button className="aaas-primary" disabled={signingIn} onClick={() => void login()}>{t(signingIn ? 'loading' : 'login')} →</button></section> : <div key={`${session.issuer}:${session.subject}`}><Routes>
        <Route path="processes" element={<Processes t={t} />} /><Route path="new" element={<ProcessEditor key="new" t={t} pilot={config.pilot_available} />} /><Route path="processes/:id" element={<ProcessEditor t={t} pilot={config.pilot_available} />} /><Route path="runs" element={<Runs t={t} language={language} />} /><Route path="runs/:id" element={<RunDetailRoute t={t} language={language} />} /><Route path="*" element={<Navigate to={`${APP_ROOT}/processes`} replace />} />
      </Routes></div>)}
    </main>
  </div>
}
function RunDetailRoute(props: { t: Translate; language: Language }) { const { id } = useParams(); return <RunDetail key={id} {...props} /> }
