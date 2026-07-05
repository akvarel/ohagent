import { useEffect, useState } from 'react';
import { api, MemorySummary } from '../api/client';

export default function Memory() {
  const [entries, setEntries] = useState<MemorySummary[]>([]);
  const [query, setQuery] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const doSearch = () => {
    setLoading(true);
    api
      .memory(undefined, query || undefined, 50)
      .then(setEntries)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    doSearch();
  }, []);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') doSearch();
  };

  return (
    <div>
      <h2 className="text-2xl font-semibold mb-6">Memory</h2>

      <div className="flex gap-2 mb-6">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Search memories..."
          className="flex-1 border border-gray-300 rounded-lg px-3 py-2 text-sm"
        />
        <button
          onClick={doSearch}
          disabled={loading}
          className="bg-orangehat-500 text-white px-4 py-2 rounded-lg hover:bg-orangehat-600 disabled:opacity-50 text-sm font-medium"
        >
          {loading ? 'Searching...' : 'Search'}
        </button>
      </div>

      {error && <div className="text-red-600 mb-4">Error: {error}</div>}

      {entries.length === 0 && !loading && (
        <p className="text-gray-400">No memories found.</p>
      )}

      <div className="grid gap-3">
        {entries.map((e) => (
          <div
            key={e.id}
            className="bg-white rounded-lg border border-gray-200 p-4 shadow-sm"
          >
            <div className="flex items-center justify-between mb-1">
              <span className="text-xs font-mono text-gray-400">{e.id.slice(0, 8)}</span>
              <span className="text-xs text-gray-400">
                {new Date(e.created_at).toLocaleDateString()}
              </span>
            </div>
            <p className="text-sm">{e.content}</p>
            <div className="flex gap-2 mt-2 flex-wrap">
              <span className="text-xs bg-gray-100 px-2 py-0.5 rounded">
                {e.source}
              </span>
              {e.tags.map((t) => (
                <span key={t} className="text-xs bg-blue-50 text-blue-600 px-2 py-0.5 rounded">
                  {t}
                </span>
              ))}
              <span className="text-xs text-gray-400">
                importance: {Math.round(e.importance * 100)}%
              </span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
