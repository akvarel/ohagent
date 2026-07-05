import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { api, SkillSummary } from '../api/client';

const STATUS_COLORS: Record<string, string> = {
  active: 'bg-green-100 text-green-800',
  proposed: 'bg-yellow-100 text-yellow-800',
  disabled: 'bg-gray-100 text-gray-600',
  retired: 'bg-red-100 text-red-600',
};

export default function Skills() {
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [filter, setFilter] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.skills(undefined, filter || undefined)
      .then(setSkills)
      .catch((e) => setError(e.message));
  }, [filter]);

  if (error) return <div className="text-red-600 p-4">Failed to load: {error}</div>;

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-2xl font-semibold">Skills</h2>
        <select
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          className="border border-gray-300 rounded-lg px-3 py-1.5 text-sm"
        >
          <option value="">All Status</option>
          <option value="active">Active</option>
          <option value="proposed">Proposed</option>
          <option value="disabled">Disabled</option>
          <option value="retired">Retired</option>
        </select>
      </div>

      {skills.length === 0 && (
        <p className="text-gray-400">No skills yet. They'll appear as you use the agent.</p>
      )}

      <div className="grid gap-3">
        {skills.map((s) => (
          <Link
            key={s.id}
            to={`/skills/${s.id}`}
            className="bg-white rounded-lg border border-gray-200 p-4 hover:border-orangehat-300 transition-colors shadow-sm"
          >
            <div className="flex items-center justify-between">
              <div>
                <span className="font-semibold text-lg">{s.name}</span>
                <span className="text-gray-400 text-sm ml-2">v{s.version}</span>
              </div>
              <div className="flex items-center gap-3">
                <span className="text-sm text-gray-500">{s.use_count} uses</span>
                <span className="text-sm font-mono">{Math.round(s.quality_score * 100)}%</span>
                <span
                  className={`text-xs px-2 py-0.5 rounded-full font-medium ${STATUS_COLORS[s.status] || 'bg-gray-100'}`}
                >
                  {s.status}
                </span>
              </div>
            </div>
            {s.triggers.length > 0 && (
              <div className="flex gap-1 mt-2 flex-wrap">
                {s.triggers.map((t) => (
                  <span key={t} className="text-xs bg-gray-100 px-2 py-0.5 rounded">
                    {t}
                  </span>
                ))}
              </div>
            )}
          </Link>
        ))}
      </div>
    </div>
  );
}
