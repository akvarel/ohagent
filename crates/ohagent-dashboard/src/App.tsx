import { Routes, Route, NavLink, useLocation } from 'react-router-dom';
import Progress from './pages/Progress';
import Dashboard from './pages/Dashboard';
import Skills from './pages/Skills';
import SkillDetail from './pages/SkillDetail';
import Memory from './pages/Memory';
import ApiKeys from './pages/ApiKeys';
import Usage from './pages/Usage';

function App() {
  const location = useLocation();
  if (location.pathname === '/progress') {
    return <main className="min-h-screen p-4 sm:p-8 bg-slate-50"><Progress /></main>;
  }
  const progressLabel = navigator.language.startsWith('ru') ? 'Ход работы' : navigator.language.startsWith('lv') ? 'Darba gaita' : 'Live activity';
  return (
    <div className="min-h-screen flex flex-col">
      <nav className="bg-white border-b border-gray-200 px-6 py-3 flex flex-wrap gap-6 items-center shadow-sm">
        <NavLink to="/progress" className="text-sm font-medium text-orangehat-600">{progressLabel}</NavLink>
        <h1 className="text-xl font-bold text-orangehat-600">ohAgent</h1>
        <NavLink
          to="/"
          end
          className={({ isActive }) =>
            `text-sm font-medium ${isActive ? 'text-orangehat-600' : 'text-gray-500 hover:text-gray-700'}`
          }
        >
          Dashboard
        </NavLink>
        <NavLink
          to="/skills"
          className={({ isActive }) =>
            `text-sm font-medium ${isActive ? 'text-orangehat-600' : 'text-gray-500 hover:text-gray-700'}`
          }
        >
          Skills
        </NavLink>
        <NavLink
          to="/memory"
          className={({ isActive }) =>
            `text-sm font-medium ${isActive ? 'text-orangehat-600' : 'text-gray-500 hover:text-gray-700'}`
          }
        >
          Memory
        </NavLink>
        <NavLink
          to="/keys"
          className={({ isActive }) =>
            `text-sm font-medium ${isActive ? 'text-orangehat-600' : 'text-gray-500 hover:text-gray-700'}`
          }
        >
          API Keys
        </NavLink>
        <NavLink
          to="/usage"
          className={({ isActive }) =>
            `text-sm font-medium ${isActive ? 'text-orangehat-600' : 'text-gray-500 hover:text-gray-700'}`
          }
        >
          Usage
        </NavLink>
      </nav>
      <main className="flex-1 p-6 max-w-6xl mx-auto w-full">
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/skills" element={<Skills />} />
          <Route path="/skills/:id" element={<SkillDetail />} />
          <Route path="/memory" element={<Memory />} />
          <Route path="/keys" element={<ApiKeys />} />
          <Route path="/usage" element={<Usage />} />
        </Routes>
      </main>
    </div>
  );
}

export default App;
