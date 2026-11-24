import { lazy, Suspense } from 'react';
import { Routes, Route, NavLink, Outlet } from 'react-router-dom';
import Progress from './pages/Progress';
import Dashboard from './pages/Dashboard';
import Skills from './pages/Skills';
import SkillDetail from './pages/SkillDetail';
import Memory from './pages/Memory';
import ApiKeys from './pages/ApiKeys';
import Usage from './pages/Usage';
import { LocaleProvider, useLocale } from './public/locale';
import Metadata from './public/Metadata';
import PublicLayout, { LanguagePicker, NotFound } from './public/PublicLayout';
import Home from './public/Home';
import Product from './public/Product';

const Demo = lazy(() => import('./public/Demo'));
// Keep customer code out of the public entry bundle. Missing required app
// modules are build errors, never a silently substituted unavailable product.
const AaasApp = lazy(() => import('./aaas/AaasApp'));

function Loading() {
  const { t } = useLocale();
  return <div className="oh-public oh-loading" role="status">{t.loading}</div>;
}
function OperatorLayout() {
  const { t } = useLocale();
  const links = [['/operator', t.dashboard], ['/progress', t.progress], ['/skills', t.skills], ['/memory', t.memory], ['/keys', t.keys], ['/usage', t.usage]];
  return <div className="min-h-screen flex flex-col bg-gray-50">
    <nav className="bg-white border-b border-gray-200 px-6 py-3 flex flex-wrap gap-5 items-center shadow-sm" aria-label={t.operator}>
      <NavLink to="/" className="text-xl font-bold text-orangehat-600">OrangeHat</NavLink>
      {links.map(([to, label]) => <NavLink key={to} to={to} end={to === '/operator'} className={({ isActive }) => `text-sm font-medium ${isActive ? 'text-orangehat-600' : 'text-gray-600 hover:text-gray-900'}`}>{label}</NavLink>)}
      <LanguagePicker />
    </nav>
    <main className="flex-1 p-4 sm:p-6 max-w-6xl mx-auto w-full"><p className="text-xs text-gray-500 mb-5">{t.operatorNote}</p><Outlet /></main>
  </div>;
}
function App() {
  return <LocaleProvider><Metadata /><Suspense fallback={<Loading />}><Routes>
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
    <Route element={<PublicLayout />}>
      <Route path="/" element={<Home />} />
      <Route path="/agent-as-a-service" element={<Product />} />
      <Route path="/agent-as-a-service/demo" element={<Demo />} />
      <Route path="*" element={<NotFound />} />
    </Route>
  </Routes></Suspense></LocaleProvider>;
}
export default App;
