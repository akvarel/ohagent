import { useEffect, type ReactNode } from 'react';
import { Link, NavLink, Outlet, useLocation } from 'react-router-dom';
import { isLanguage, useLocale } from './locale';
import './public.css';

export function Arrow() {
  return <span aria-hidden="true">↗</span>;
}
export function LanguagePicker() {
  const { language, setLanguage, t } = useLocale();
  return <label className="oh-language"><span className="sr-only">{t.language}</span><select aria-label={t.language} value={language} onChange={event => { if (isLanguage(event.target.value)) setLanguage(event.target.value); }}><option value="en">EN</option><option value="lv">LV</option><option value="ru">RU</option></select></label>;
}
export function Brand() {
  return <span className="oh-brand"><svg aria-hidden="true" viewBox="0 0 36 30" fill="none"><path d="M10 19 13 5h10l3 14 7 4v3H3v-3l7-4Z" fill="currentColor"/><path d="M10 19h16" stroke="#1c1e21" strokeWidth="2"/></svg><span>orange<span className="oh-brand-light">hat</span><span className="oh-brand-dot">.</span></span></span>;
}
export default function PublicLayout() {
  const { t } = useLocale();
  const { pathname, hash } = useLocation();
  useEffect(() => {
    if (hash) {
      let id = hash.slice(1);
      try { id = decodeURIComponent(id); } catch { /* A malformed fragment must not break navigation. */ }
      document.getElementById(id)?.scrollIntoView({ behavior: 'auto' });
    } else { window.scrollTo(0, 0); }
  }, [pathname, hash]);
  return <div className="oh-public">
    <a className="oh-skip" href="#main-content">{t.skip}</a>
    <header className="oh-header"><div className="oh-wrap oh-header-inner">
      <Link to="/" aria-label="OrangeHat"><Brand /></Link>
      <nav className="oh-nav" aria-label={t.products}>
        <NavLink to="/agent-as-a-service">{t.product}</NavLink>
        <Link to="/#ecosystem">{t.products}</Link>
        <Link to="/#approach">{t.approach}</Link>
        <Link to="/#contact">{t.contact}</Link>
        <Link to="/agent-as-a-service/app">{t.openApp}</Link>
      </nav>
      <LanguagePicker />
    </div></header>
    <main id="main-content" tabIndex={-1}><Outlet /></main>
    <footer className="oh-footer"><div className="oh-wrap">
      <div className="oh-footer-top"><Link to="/" aria-label="OrangeHat"><Brand /></Link><p>{t.footer}</p><Link to="/operator">{t.operator} <Arrow /></Link></div>
      <div className="oh-footer-bottom"><span>© {new Date().getFullYear()} OrangeHat</span><span>{t.privacy}</span><span>EN / LV / RU</span></div>
    </div></footer>
  </div>;
}
export function SectionIntro({ eyebrow, title, body }: { eyebrow: string; title: string; body?: string }) {
  return <div className="oh-section-intro"><p className="oh-eyebrow">{eyebrow}</p><h2>{title}</h2>{body && <p className="oh-lead">{body}</p>}</div>;
}
export function Buttons({ product = false }: { product?: boolean }) {
  const { t } = useLocale();
  return <div className="oh-actions"><Link className="oh-button oh-button-primary" to={product ? '/agent-as-a-service/demo' : '/agent-as-a-service'}>{product ? t.demo : t.explore}<Arrow /></Link><Link className="oh-button oh-button-secondary" to={product ? '/agent-as-a-service/app' : '/agent-as-a-service/demo'}>{product ? t.openApp : t.demo}<Arrow /></Link></div>;
}
export function Architecture({ compact = false }: { compact?: boolean }) {
  const { t } = useLocale();
  return <figure className={`oh-architecture ${compact ? 'oh-architecture-compact' : ''}`} aria-label={t.architecture}>
    <div className="oh-architecture-head"><span>{t.architecture}</span><span className="oh-mono">ohAgent</span></div>
    <p className="oh-domain-label">{t.domainLabel}</p>
    <div className="oh-domains">{[t.domainOne, t.domainTwo, t.domainThree].map((domain, index) => <div className="oh-domain" key={domain}><svg viewBox="0 0 24 24" fill="none" aria-hidden="true"><path d={index === 0 ? 'm8 6-6 6 6 6m8-12 6 6-6 6m-2-15-4 18' : index === 1 ? 'M4 5h16v5H4zM4 14h16v5H4zM8 7v1m0 8v1' : 'M3 4h7l2 3 2-3h7v15h-7l-2 2-2-2H3V4Zm9 3v14'} stroke="currentColor" strokeWidth="1.5"/></svg><span>{domain}</span></div>)}</div>
    <div className="oh-connector" aria-hidden="true"><span /><span /><span /></div>
    <div className="oh-runtime"><span className="oh-runtime-symbol" aria-hidden="true">o<span>h</span></span><div><strong>{t.runtimeLabel}</strong><small>{t.runtimeSub}</small></div><span className="oh-runtime-corner" aria-hidden="true">↗</span></div>
    <div className="oh-connector oh-connector-bottom" aria-hidden="true"><span /></div>
    <div className="oh-instance"><span className="oh-instance-icon" aria-hidden="true">◇</span><div><strong>{t.instance}</strong><small>{t.instanceSub}</small></div></div>
    <figcaption>{t.diagramLabel}</figcaption>
  </figure>;
}
export function Notice({ children }: { children: ReactNode }) { return <p className="oh-notice"><span aria-hidden="true">i</span>{children}</p>; }
export function UnavailableWorkspace() {
  const { t } = useLocale();
  return <div className="oh-public oh-standalone"><section className="oh-wrap oh-section"><p className="oh-eyebrow">ohAgent</p><h1>{t.unavailableTitle}</h1><p className="oh-lead">{t.unavailableBody}</p><Link className="oh-button oh-button-primary" to="/agent-as-a-service/demo">{t.demo}<Arrow /></Link></section></div>;
}
export function NotFound() { const { t } = useLocale(); return <section className="oh-wrap oh-section"><p className="oh-eyebrow">404</p><h1>{t.notFound}</h1><p className="oh-lead">{t.notFoundBody}</p><Link className="oh-button oh-button-primary" to="/">{t.home}<Arrow /></Link></section>; }
