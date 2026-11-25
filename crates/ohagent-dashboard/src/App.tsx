import { lazy, Suspense, useEffect } from 'react';
import { Routes, Route, NavLink, Outlet, Navigate, useLocation } from 'react-router-dom';
import Progress from './pages/Progress';
import Dashboard from './pages/Dashboard';
import Skills from './pages/Skills';
import SkillDetail from './pages/SkillDetail';
import Memory from './pages/Memory';
import ApiKeys from './pages/ApiKeys';
import Usage from './pages/Usage';
import { useLanguage } from './aaas/i18n';
const AaasApp = lazy(() => import('./aaas/AaasApp'));
const labels = {
  en: { operator: 'Operator console', dashboard: 'Dashboard', skills: 'Skills', memory: 'Memory', keys: 'API keys', usage: 'Usage', progress: 'Live activity', language: 'Language', loading: 'Loading…', notice: 'Operator tools. This is not customer sign-in.', missing: 'Page not found' },
  lv: { operator: 'Operatora konsole', dashboard: 'Pārskats', skills: 'Prasmes', memory: 'Atmiņa', keys: 'API atslēgas', usage: 'Lietojums', progress: 'Darba gaita', language: 'Valoda', loading: 'Ielādē…', notice: 'Operatora rīki. Šī nav klientu pierakstīšanās.', missing: 'Lapa nav atrasta' },
  ru: { operator: 'Консоль оператора', dashboard: 'Обзор', skills: 'Навыки', memory: 'Память', keys: 'Ключи API', usage: 'Использование', progress: 'Ход работы', language: 'Язык', loading: 'Загрузка…', notice: 'Инструменты оператора. Это не вход для клиентов.', missing: 'Страница не найдена' },
};
function Metadata() {
  const { language } = useLanguage();
  const { pathname } = useLocation();
  useEffect(() => {
    document.documentElement.lang = language;
    document.title = 'ohAgent | ' + labels[language].operator;
    document.querySelector('meta[name="robots"]')?.setAttribute('content', 'noindex, nofollow');
  }, [language, pathname]);
  return null;
}
function OperatorLayout() {
  const { language, change } = useLanguage();
  const t = labels[language];
  const links = [['/operator', t.dashboard], ['/progress', t.progress], ['/skills', t.skills], ['/memory', t.memory], ['/keys', t.keys], ['/usage', t.usage]];
  return <div className="min-h-screen flex flex-col bg-gray-50">
    <nav className="bg-white border-b border-gray-200 px-6 py-3 flex flex-wrap gap-5 items-center shadow-sm" aria-label={t.operator}>
      <NavLink to="/operator" className="text-xl font-bold text-orangehat-600">ohAgent</NavLink>
      {links.map(([to, label]) => <NavLink key={to} to={to} end={to === '/operator'} className={({ isActive }) => `text-sm font-medium ${isActive ? 'text-orangehat-600' : 'text-gray-600 hover:text-gray-900'}`}>{label}</NavLink>)}
      <label>{t.language} <select aria-label={t.language} value={language} onChange={e => { const v = e.target.value; if (v === 'en' || v === 'lv' || v === 'ru') change(v); }}><option value="en">EN</option><option value="lv">LV</option><option value="ru">RU</option></select></label>
    </nav>
    <main className="flex-1 p-4 sm:p-6 max-w-6xl mx-auto w-full"><p className="text-xs text-gray-500 mb-5">{t.notice}</p><Outlet /></main>
  </div>;
}
export default function App() {
  const { language } = useLanguage();
  return <><Metadata /><Suspense fallback={<div role="status">{labels[language].loading}</div>}><Routes>
    <Route path="/" element={<Navigate to="/operator" replace />} />
    <Route path="/agent-as-a-service/app/*" element={<AaasApp />} />
    <Route path="/progress" element={<main className="min-h-screen p-4 sm:p-8 bg-slate-50"><Progress /></main>} />
    <Route element={<OperatorLayout />}>
      <Route path="/operator" element={<Dashboard />} />
      <Route path="/skills" element={<Skills />} />
      <Route path="/skills/:id" element={<SkillDetail />} />
      <Route path="/memory" element={<Memory />} />
      <Route path="/keys" element={<ApiKeys />} />
      <Route path="/usage" element={<Usage />} />
    </Route>
    <Route path="*" element={<main><h1>{labels[language].missing}</h1><NavLink to="/operator">ohAgent</NavLink></main>} />
  </Routes></Suspense></>;
}
