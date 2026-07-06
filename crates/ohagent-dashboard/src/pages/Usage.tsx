import { useEffect, useState } from 'react';
import { api, UsageStats, UsageRecord } from '../api/client';

function fmtTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

function fmtCost(n: number): string {
  if (n < 0.01) return `$${n.toFixed(4)}`;
  return `$${n.toFixed(2)}`;
}

export default function Usage() {
  const [stats, setStats] = useState<UsageStats | null>(null);
  const [recent, setRecent] = useState<UsageRecord[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([
      api.usageStats().then(setStats),
      api.usageRecent(20).then(setRecent),
    ]).catch((e) => setError(e.message));
  }, []);

  if (error) return <div className="text-red-600 p-4">Failed to load: {error}</div>;
  if (!stats) return <div className="text-gray-400 p-4">Loading...</div>;

  return (
    <div>
      <h2 className="text-2xl font-semibold mb-6">Usage & Costs</h2>

      {/* Summary cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-8">
        <StatCard label="Total Calls" value={String(stats.total_calls)} />
        <StatCard label="Input Tokens" value={fmtTokens(stats.total_input_tokens)} />
        <StatCard label="Output Tokens" value={fmtTokens(stats.total_output_tokens)} />
        <StatCard label="Total Cost" value={fmtCost(stats.total_cost_usd)} highlight />
      </div>

      {/* By model */}
      {stats.by_model.length > 0 && (
        <div className="mb-8">
          <h3 className="text-lg font-medium mb-3">By Model</h3>
          <div className="bg-white rounded-lg border border-gray-200 shadow-sm overflow-hidden">
            <table className="w-full">
              <thead className="bg-gray-50 border-b border-gray-200">
                <tr>
                  <th className="text-left px-4 py-2 text-sm font-medium text-gray-500">Model</th>
                  <th className="text-right px-4 py-2 text-sm font-medium text-gray-500">Calls</th>
                  <th className="text-right px-4 py-2 text-sm font-medium text-gray-500">Tokens In</th>
                  <th className="text-right px-4 py-2 text-sm font-medium text-gray-500">Tokens Out</th>
                  <th className="text-right px-4 py-2 text-sm font-medium text-gray-500">Cost</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100">
                {stats.by_model.map((m) => (
                  <tr key={m.model_id} className="hover:bg-gray-50">
                    <td className="px-4 py-2 font-medium text-sm">{m.model_display}</td>
                    <td className="px-4 py-2 text-right text-sm">{m.calls}</td>
                    <td className="px-4 py-2 text-right text-sm">{fmtTokens(m.input_tokens)}</td>
                    <td className="px-4 py-2 text-right text-sm">{fmtTokens(m.output_tokens)}</td>
                    <td className="px-4 py-2 text-right text-sm font-medium">{fmtCost(m.cost_usd)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* By day */}
      {stats.by_day.length > 0 && (
        <div className="mb-8">
          <h3 className="text-lg font-medium mb-3">Daily Costs (last 30 days)</h3>
          <div className="bg-white rounded-lg border border-gray-200 shadow-sm p-4">
            <div className="flex items-end gap-1 h-32">
              {stats.by_day
                .slice()
                .reverse()
                .map((d) => {
                  const maxCost = Math.max(...stats.by_day.map((x) => x.cost_usd), 0.001);
                  const height = maxCost > 0 ? (d.cost_usd / maxCost) * 100 : 0;
                  return (
                    <div
                      key={d.date}
                      className="flex-1 relative group"
                      title={`${d.date}: ${d.calls} calls, ${fmtCost(d.cost_usd)}`}
                    >
                      <div
                        className="absolute bottom-0 left-0 right-0 bg-orangehat-500 rounded-t opacity-80 hover:opacity-100 transition-opacity"
                        style={{ height: `${Math.max(height, 1)}%` }}
                      />
                      <span className="absolute -bottom-5 left-0 right-0 text-center text-xs text-gray-400 hidden group-hover:block">
                        {d.date.slice(5)}
                      </span>
                    </div>
                  );
                })}
            </div>
            <div className="text-xs text-gray-400 mt-6 text-center">
              Hover over bars to see dates
            </div>
          </div>
        </div>
      )}

      {/* Recent calls */}
      {recent.length > 0 && (
        <div>
          <h3 className="text-lg font-medium mb-3">Recent Calls</h3>
          <div className="bg-white rounded-lg border border-gray-200 shadow-sm overflow-hidden">
            <table className="w-full">
              <thead className="bg-gray-50 border-b border-gray-200">
                <tr>
                  <th className="text-left px-4 py-2 text-sm font-medium text-gray-500">Time</th>
                  <th className="text-left px-4 py-2 text-sm font-medium text-gray-500">Model</th>
                  <th className="text-right px-4 py-2 text-sm font-medium text-gray-500">Tokens</th>
                  <th className="text-right px-4 py-2 text-sm font-medium text-gray-500">Cost</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100">
                {recent.map((r) => (
                  <tr key={r.id} className="hover:bg-gray-50">
                    <td className="px-4 py-2 text-xs text-gray-500">
                      {new Date(r.created_at).toLocaleString()}
                    </td>
                    <td className="px-4 py-2 text-sm">
                      {r.model_display}
                      <div className="text-xs text-gray-400">
                        {r.capabilities.join(', ')}
                      </div>
                    </td>
                    <td className="px-4 py-2 text-right text-xs">
                      <span className="text-green-600">{fmtTokens(r.input_tokens)}</span>
                      {' / '}
                      <span className="text-blue-600">{fmtTokens(r.output_tokens)}</span>
                    </td>
                    <td className="px-4 py-2 text-right text-xs font-medium">
                      {fmtCost(r.estimated_cost_usd)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

function StatCard({
  label,
  value,
  highlight,
}: {
  label: string;
  value: string;
  highlight?: boolean;
}) {
  return (
    <div className="bg-white rounded-lg border border-gray-200 p-4 shadow-sm">
      <div className="text-sm text-gray-500">{label}</div>
      <div
        className={`text-xl font-semibold mt-1 ${highlight ? 'text-orangehat-600' : ''}`}
      >
        {value}
      </div>
    </div>
  );
}
