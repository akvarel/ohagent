import { FormEvent, useEffect, useState } from 'react';
import { Activity, fetchProgress, isStale, ReceivedSnapshot } from '../api/progress';
import './Progress.css';

const messages = {
  en: {
    title: 'Work in view', eyebrow: 'ohAgent / live activity', intro: 'Follow the work without opening the conversation.',
    tenant: 'Tenant ID', token: 'Read token', connect: 'Connect', disconnect: 'Disconnect', language: 'Language',
    privacy: 'Read-only. Credentials stay in this tab’s memory. Prompts, reasoning, file contents and tool arguments are never published.',
    scope: 'Shows sessions run through the ohAgent Jcode SDK bridge. Not a monitor of unrelated terminal sessions.',
    empty: 'No activity yet. Start an ohAgent session for this tenant, or check that its publisher is configured.',
    welcome: 'Connect to this server using your tenant ID and approved read token. Locally, use the daemon API key.',
    live: 'Connected', offline: 'Connection lost', connecting: 'Connecting', disconnected: 'Not connected',
    stale: 'Publisher offline', fresh: 'Receiving updates', updated: 'Last received', ago: 's ago',
    all: 'All sessions', active: 'Working', needs: 'Needs attention', finished: 'Finished',
    none: 'No sessions match this filter.', session: 'Session', activity: 'Activity trail', tools: 'Tool executions',
    input: 'Input', output: 'Output', tokens: 'Latest provider call tokens, not cumulative usage',
    noTools: 'No tools executed yet', details: 'Show activity', source: 'Source', expired: 'Access denied. Check the token and tenant ID.',
    network: 'Cannot reach the monitor. Last known data is kept below. Check the connection or server.',
    idle: 'Idle', working: 'Working', waiting: 'Waiting for permission', completed: 'Completed', cancelled: 'Interrupted', failed: 'Stopped with error',
    unsupported: 'Plan text and subagent hierarchy are not exposed by this activity projection. No completion percentage is inferred.',
  },
  ru: {
    title: 'Работа на виду', eyebrow: 'ohAgent / ход работы', intro: 'Следите за работой, не открывая переписку.',
    tenant: 'ID пользователя (tenant)', token: 'Токен просмотра', connect: 'Подключиться', disconnect: 'Отключиться', language: 'Язык',
    privacy: 'Только просмотр. Токен хранится в памяти этой вкладки. Запросы, рассуждения, файлы и аргументы инструментов не публикуются.',
    scope: 'Показаны сессии через SDK-мост ohAgent и Jcode. Посторонние терминальные сессии не отслеживаются.',
    empty: 'Пока нет активности. Запустите сессию ohAgent для этого tenant или проверьте настройку отправки обновлений.',
    welcome: 'Введите ID tenant и согласованный токен просмотра. Для локального сервера используйте API-ключ демона.',
    live: 'Подключено', offline: 'Связь потеряна', connecting: 'Подключение', disconnected: 'Не подключено',
    stale: 'Источник недоступен', fresh: 'Обновления поступают', updated: 'Получено', ago: 'с назад',
    all: 'Все сессии', active: 'В работе', needs: 'Требуют внимания', finished: 'Завершённые',
    none: 'Нет сессий с таким фильтром.', session: 'Сессия', activity: 'Ход работы', tools: 'Вызовы инструментов',
    input: 'Вход', output: 'Выход', tokens: 'Токены последнего вызова модели, не общий расход',
    noTools: 'Инструменты ещё не вызывались', details: 'Показать события', source: 'Источник', expired: 'Нет доступа. Проверьте токен и ID tenant.',
    network: 'Сервер недоступен. Ниже сохранены последние данные. Проверьте соединение или сервер.',
    idle: 'Ожидает задачи', working: 'Работает', waiting: 'Ждёт разрешения', completed: 'Завершено', cancelled: 'Прервано', failed: 'Остановлено с ошибкой',
    unsupported: 'Текст плана и дерево субагентов не входят в эту проекцию событий. Процент готовности не вычисляется.',
  },
  lv: {
    title: 'Darbs redzeslokā', eyebrow: 'ohAgent / darba gaita', intro: 'Sekojiet darbam, neatverot saraksti.',
    tenant: 'Nomnieka ID', token: 'Skatīšanas tokens', connect: 'Savienot', disconnect: 'Atvienot', language: 'Valoda',
    privacy: 'Tikai lasīšana. Tokens paliek šīs cilnes atmiņā. Pieprasījumi, spriešana, failu saturs un rīku argumenti netiek publicēti.',
    scope: 'Rāda sesijas, kas darbojas ar ohAgent Jcode SDK tiltu. Citas termināļa sesijas netiek uzraudzītas.',
    empty: 'Vēl nav aktivitātes. Sāciet šī nomnieka ohAgent sesiju vai pārbaudiet atjauninājumu sūtīšanu.',
    welcome: 'Ievadiet nomnieka ID un apstiprināto skatīšanas tokenu. Lokāli izmantojiet dēmona API atslēgu.',
    live: 'Savienots', offline: 'Savienojums pārtrūkst', connecting: 'Savienojas', disconnected: 'Nav savienots',
    stale: 'Avots nav pieejams', fresh: 'Saņem atjauninājumus', updated: 'Saņemts pirms', ago: 's',
    all: 'Visas sesijas', active: 'Darbā', needs: 'Jāpievērš uzmanība', finished: 'Pabeigtās',
    none: 'Nav sesiju šim filtram.', session: 'Sesija', activity: 'Darba gaita', tools: 'Rīku izpildes',
    input: 'Ievade', output: 'Izvade', tokens: 'Pēdējā modeļa izsaukuma tokeni, nevis kopējais patēriņš',
    noTools: 'Rīki vēl nav izpildīti', details: 'Rādīt notikumus', source: 'Avots', expired: 'Piekļuve liegta. Pārbaudiet tokenu un nomnieka ID.',
    network: 'Monitoram nevar piekļūt. Zemāk ir pēdējie dati. Pārbaudiet savienojumu vai serveri.',
    idle: 'Gaida uzdevumu', working: 'Darbojas', waiting: 'Gaida atļauju', completed: 'Pabeigts', cancelled: 'Pārtraukts', failed: 'Apturēts ar kļūdu',
    unsupported: 'Plāna teksts un apakšaģentu hierarhija šajā notikumu skatā nav pieejami. Gatavības procenti netiek pieņemti.',
  },
};
type Language = keyof typeof messages;
type Filter = 'all' | 'active' | 'needs' | 'finished';
const rank: Record<Activity, number> = { waiting: 0, working: 1, failed: 2, idle: 3, completed: 4, cancelled: 5 };

export default function Progress() {
  const [language, setLanguage] = useState<Language>(() => navigator.language.startsWith('ru') ? 'ru' : navigator.language.startsWith('lv') ? 'lv' : 'en');
  const t = messages[language];
  const [tenant, setTenant] = useState('');
  const [token, setToken] = useState('');
  const [connection, setConnection] = useState<{ tenant: string; token: string } | null>(null);
  const [sources, setSources] = useState<ReceivedSnapshot[]>([]);
  const [state, setState] = useState<'disconnected' | 'connecting' | 'live' | 'offline'>('disconnected');
  const [error, setError] = useState<'expired' | 'network' | null>(null);
  const [filter, setFilter] = useState<Filter>('all');
  const [now, setNow] = useState(Date.now());

  useEffect(() => { const timer = window.setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(timer); }, []);
  useEffect(() => {
    if (!connection) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    let controller: AbortController;
    const poll = async () => {
      controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 10_000);
      let denied = false;
      try {
        const data = await fetchProgress(connection.tenant, connection.token, controller.signal);
        if (!cancelled) { setSources(data); setState('live'); setError(null); }
      } catch (e) {
        denied = e instanceof Error && ['401', '403'].includes(e.message);
        if (!cancelled) { setState('offline'); setError(denied ? 'expired' : 'network'); }
      } finally {
        clearTimeout(timeout);
        if (!cancelled && !denied) timer = setTimeout(poll, 3000);
      }
    };
    void poll();
    return () => { cancelled = true; clearTimeout(timer); controller?.abort(); };
  }, [connection]);

  const connect = (event: FormEvent) => {
    event.preventDefault();
    setSources([]); setError(null); setState('connecting');
    setConnection({ tenant: tenant.trim(), token });
    setToken('');
  };
  const disconnect = () => { setConnection(null); setToken(''); setSources([]); setState('disconnected'); setError(null); };
  const visible = (status: Activity) => filter === 'all' || (filter === 'active' && status === 'working') || (filter === 'needs' && ['waiting', 'failed'].includes(status)) || (filter === 'finished' && ['completed', 'cancelled'].includes(status));
  const seconds = (date: number) => Math.max(0, Math.floor((now - date) / 1000));
  const number = (n: number) => new Intl.NumberFormat(language).format(n);
  const clock = (n: number) => new Date(n).toLocaleTimeString(language, { hour: '2-digit', minute: '2-digit', second: '2-digit' });

  return <section className="progress-panel" lang={language}>
    <header className="progress-heading">
      <div><p className="progress-eyebrow">{t.eyebrow}</p><h2>{t.title}</h2><p>{t.intro}</p></div>
      <label className="progress-language"><span>{t.language}</span><select value={language} onChange={e => setLanguage(e.target.value as Language)}><option value="en">English</option><option value="lv">Latviešu</option><option value="ru">Русский</option></select></label>
    </header>
    <div className={`progress-connection ${state}`} aria-live="polite"><span className="progress-signal" />{t[state]}{connection && <span className="progress-tenant">{connection.tenant}</span>}</div>
    {!connection ? <form className="progress-login" onSubmit={connect}>
      <label>{t.tenant}<input value={tenant} onChange={e => setTenant(e.target.value)} required maxLength={128} pattern={'[A-Za-z0-9_.:@\\-]+'} autoComplete="off" /></label>
      <label>{t.token}<input type="password" value={token} onChange={e => setToken(e.target.value)} required autoComplete="off" /></label>
      <button type="submit">{t.connect}</button><p>{t.welcome}</p>
    </form> : <div className="progress-toolbar"><div className="progress-filters" role="group" aria-label={t.all}>{(['all', 'active', 'needs', 'finished'] as Filter[]).map(f => <button key={f} aria-pressed={filter === f} onClick={() => setFilter(f)}>{t[f]}</button>)}</div><button className="progress-disconnect" onClick={disconnect}>{t.disconnect}</button></div>}
    {error && <p className="progress-error" role="alert">{t[error]}</p>}
    {state === 'live' && sources.length === 0 && <div className="progress-empty">{t.empty}</div>}
    {sources.map(source => {
      const stale = isStale(source, now);
      const sessions = [...source.snapshot.sessions].filter(s => visible(s.status)).sort((a, b) => rank[a.status] - rank[b.status] || b.updated_at - a.updated_at);
      return <section key={source.snapshot.source_id} className={`progress-source ${stale ? 'is-stale' : ''}`}>
        <header><div><span className="progress-eyebrow">{t.source}</span><h3>{source.snapshot.source_id}</h3></div><div className="progress-freshness"><strong>{stale ? t.stale : t.fresh}</strong><span>{t.updated} {seconds(source.received_at)} {t.ago}</span></div></header>
        {sessions.length === 0 && <p className="progress-empty">{source.snapshot.sessions.length ? t.none : t.empty}</p>}
        <div className="progress-sessions">{sessions.map(session => <article key={session.id} className={`progress-session status-${session.status}`}>
          <div className="progress-session-top"><div><span className="progress-eyebrow">{t.session}</span><h4>{session.id.slice(0, 10)}</h4></div><span className="progress-status">{t[session.status]}</span></div>
          <div className="progress-current"><span className="progress-signal" /><strong>{session.current_tool || t[session.status]}</strong><time>{clock(session.updated_at)}</time></div>
          <div className="progress-toolbox"><p>{t.tools}</p><div>{Object.entries(session.tools).sort((a, b) => b[1] - a[1]).map(([tool, count]) => <span className={tool === session.current_tool ? 'tool-current' : ''} key={tool}>{tool}<b>{number(count)}</b></span>)}{Object.keys(session.tools).length === 0 && <small>{t.noTools}</small>}</div></div>
          <div className="progress-usage"><span>{t.input} <b>{number(session.input_tokens)}</b></span><span>{t.output} <b>{number(session.output_tokens)}</b></span><small>{t.tokens}</small></div>
          <details><summary>{t.details} <span>{session.events.length}</span></summary><ol className="progress-trail" aria-label={t.activity}>{[...session.events].reverse().map((event, i) => <li key={`${event.at}-${i}`}><time>{clock(event.at)}</time><span>{t[event.kind]}{event.tool && <> / <code>{event.tool}</code></>}</span></li>)}</ol></details>
        </article>)}</div>
      </section>;
    })}
    <footer className="progress-notes"><p>{t.privacy}</p><p>{t.scope}</p><p>{t.unsupported}</p></footer>
  </section>;
}
