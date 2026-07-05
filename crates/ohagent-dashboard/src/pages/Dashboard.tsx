import { useEffect, useState } from 'react';
import { api, Status } from '../api/client';

function fmtSecs(s: number): string {
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const rm = m % 60;
  if (h < 24) return `${h}h ${rm}m`;
  const d = Math.floor(h / 24);
  return `${d}d ${h % 24}h`;
}

export default function Dashboard() {
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.status().then(setStatus).catch((e) => setError(e.message));
  }, []);

  if (error) return <div className="text-red-600 p-4">Failed to load: {error}</div>;
  if (!status) return <div className="text-gray-400 p-4">Loading...</div>;

  return (
    <div>
      <h2 className="text-2xl font-semibold mb-6">Dashboard</h2>
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        <StatCard label="Service" value={status.service} />
        <StatCard label="Version" value={status.version} />
        <StatCard label="Uptime" value={fmtSecs(status.uptime_seconds)} />
        <StatCard label="Provider" value={status.provider} />
        <StatCard
          label="Skills"
          value={status.skills_enabled ? String(status.skills_count) : 'Disabled'}
        />
        <StatCard
          label="Memory"
          value={status.memory_enabled ? String(status.memory_count) : 'Disabled'}
        />
      </div>
    </div>
  );
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="bg-white rounded-lg border border-gray-200 p-4 shadow-sm">
      <div className="text-sm text-gray-500">{label}</div>
      <div className="text-xl font-semibold mt-1">{value}</div>
    </div>
  );
}
