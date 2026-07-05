import { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { api, SkillDetail as SkillDetailType } from '../api/client';

const STATUS_COLORS: Record<string, string> = {
  active: 'bg-green-100 text-green-800',
  proposed: 'bg-yellow-100 text-yellow-800',
  disabled: 'bg-gray-100 text-gray-600',
  retired: 'bg-red-100 text-red-600',
};

export default function SkillDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [skill, setSkill] = useState<SkillDetailType | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);

  useEffect(() => {
    if (!id) return;
    api.skill(id).then(setSkill).catch((e) => setError(e.message));
  }, [id]);

  const handleRecordUse = async () => {
    if (!id) return;
    setRecording(true);
    try {
      await api.recordSkillUse(id, true);
      // Refresh
      const updated = await api.skill(id);
      setSkill(updated);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed');
    } finally {
      setRecording(false);
    }
  };

  if (error) return <div className="text-red-600 p-4">Failed to load: {error}</div>;
  if (!skill) return <div className="text-gray-400 p-4">Loading...</div>;

  return (
    <div>
      <button
        onClick={() => navigate('/skills')}
        className="text-sm text-orangehat-600 hover:underline mb-4 inline-block"
      >
        ← Back to Skills
      </button>

      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-2xl font-bold">{skill.name}</h2>
          <span
            className={`text-sm px-3 py-1 rounded-full font-medium ${STATUS_COLORS[skill.status] || 'bg-gray-100'}`}
          >
            {skill.status}
          </span>
        </div>

        <p className="text-gray-600 mb-6">{skill.description}</p>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
          <Stat label="Version" value={skill.version} />
          <Stat label="Origin" value={skill.origin} />
          <Stat label="Quality" value={`${Math.round(skill.quality_score * 100)}%`} />
          <Stat label="Uses" value={String(skill.use_count)} />
          <Stat label="Successes" value={String(skill.success_count)} />
          <Stat label="Failures" value={String(skill.failure_count)} />
          <Stat label="Created" value={new Date(skill.created_at).toLocaleDateString()} />
          <Stat
            label="Last Used"
            value={skill.last_used_at ? new Date(skill.last_used_at).toLocaleDateString() : 'Never'}
          />
        </div>

        {skill.instructions && (
          <div className="mb-6">
            <h3 className="font-semibold mb-2">Instructions</h3>
            <pre className="bg-gray-50 rounded-lg p-4 text-sm whitespace-pre-wrap">
              {skill.instructions}
            </pre>
          </div>
        )}

        <div className="flex gap-2 flex-wrap mb-4">
          {skill.triggers.map((t) => (
            <span key={t} className="text-sm bg-blue-50 text-blue-700 px-2 py-0.5 rounded">
              trigger: {t}
            </span>
          ))}
          {skill.tags.map((t) => (
            <span key={t} className="text-sm bg-gray-100 text-gray-600 px-2 py-0.5 rounded">
              #{t}
            </span>
          ))}
        </div>

        <button
          onClick={handleRecordUse}
          disabled={recording}
          className="bg-orangehat-500 text-white px-4 py-2 rounded-lg hover:bg-orangehat-600 disabled:opacity-50 transition-colors text-sm font-medium"
        >
          {recording ? 'Recording...' : 'Record Successful Use'}
        </button>
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs text-gray-400">{label}</div>
      <div className="font-medium">{value}</div>
    </div>
  );
}
