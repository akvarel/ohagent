import { useEffect, useState } from 'react';
import { api, KeyInfo } from '../api/client';

export default function ApiKeys() {
  const [keys, setKeys] = useState<KeyInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ text: string; ok: boolean } | null>(null);

  useEffect(() => {
    api.keys().then(setKeys).catch((e) => setError(e.message));
  }, []);

  const startEdit = (envVar: string) => {
    setEditing((prev) => ({ ...prev, [envVar]: '' }));
  };

  const cancelEdit = (envVar: string) => {
    setEditing((prev) => {
      const next = { ...prev };
      delete next[envVar];
      return next;
    });
  };

  const handleSave = async () => {
    const toSave: Record<string, string> = {};
    for (const [k, v] of Object.entries(editing)) {
      if (v.trim()) toSave[k] = v.trim();
    }
    if (Object.keys(toSave).length === 0) return;

    setSaving(true);
    setMsg(null);
    try {
      const result = await api.updateKeys(toSave);
      setMsg({ text: `Saved: ${result.updated.join(', ')}`, ok: true });
      setEditing({});
      // Refresh
      const fresh = await api.keys();
      setKeys(fresh);
    } catch (e: any) {
      setMsg({ text: `Error: ${e.message}`, ok: false });
    } finally {
      setSaving(false);
    }
  };

  if (error) return <div className="text-red-600 p-4">Failed to load: {error}</div>;
  if (keys.length === 0) return <div className="text-gray-400 p-4">Loading...</div>;

  return (
    <div>
      <h2 className="text-2xl font-semibold mb-2">API Keys</h2>
      <p className="text-gray-500 text-sm mb-6">
        Manage API keys for model providers. Keys are stored in{' '}
        <code className="bg-gray-100 px-1 rounded">~/.ohagent/keys.toml</code> and
        injected as environment variables at startup.
      </p>

      {msg && (
        <div
          className={`mb-4 p-3 rounded-lg text-sm ${
            msg.ok ? 'bg-green-50 text-green-700' : 'bg-red-50 text-red-700'
          }`}
        >
          {msg.text}
        </div>
      )}

      <div className="bg-white rounded-lg border border-gray-200 shadow-sm overflow-hidden">
        <table className="w-full">
          <thead className="bg-gray-50 border-b border-gray-200">
            <tr>
              <th className="text-left px-4 py-3 text-sm font-medium text-gray-500">
                Provider
              </th>
              <th className="text-left px-4 py-3 text-sm font-medium text-gray-500">
                Env Variable
              </th>
              <th className="text-left px-4 py-3 text-sm font-medium text-gray-500">
                Status
              </th>
              <th className="text-left px-4 py-3 text-sm font-medium text-gray-500">
                Key
              </th>
              <th className="px-4 py-3" />
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-100">
            {keys.map((k) => {
              const isEditing = k.env_var in editing;
              return (
                <tr key={k.env_var} className="hover:bg-gray-50">
                  <td className="px-4 py-3 font-medium">{k.display_name}</td>
                  <td className="px-4 py-3">
                    <code className="text-xs bg-gray-100 px-1 rounded">
                      {k.env_var}
                    </code>
                  </td>
                  <td className="px-4 py-3">
                    {k.set ? (
                      <span className="inline-flex items-center gap-1 text-green-600 text-sm">
                        <span className="w-2 h-2 rounded-full bg-green-500" />
                        Set ({k.prefix})
                      </span>
                    ) : (
                      <span className="inline-flex items-center gap-1 text-red-500 text-sm">
                        <span className="w-2 h-2 rounded-full bg-red-400" />
                        Not set
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    {isEditing ? (
                      <input
                        type="password"
                        autoFocus
                        placeholder="Enter API key..."
                        className="w-full border border-gray-300 rounded px-2 py-1 text-sm focus:outline-none focus:ring-2 focus:ring-orangehat-400"
                        value={editing[k.env_var]}
                        onChange={(e) =>
                          setEditing((prev) => ({
                            ...prev,
                            [k.env_var]: e.target.value,
                          }))
                        }
                        onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                      />
                    ) : (
                      <span className="text-gray-400 text-sm">
                        {k.set ? '••••••••' : '—'}
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-3 text-right">
                    {isEditing ? (
                      <div className="flex gap-2 justify-end">
                        <button
                          onClick={handleSave}
                          disabled={saving || !editing[k.env_var]?.trim()}
                          className="px-3 py-1 text-xs rounded bg-orangehat-600 text-white hover:bg-orangehat-700 disabled:opacity-50"
                        >
                          {saving ? 'Saving…' : 'Save'}
                        </button>
                        <button
                          onClick={() => cancelEdit(k.env_var)}
                          className="px-3 py-1 text-xs rounded border border-gray-300 text-gray-600 hover:bg-gray-100"
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <button
                        onClick={() => startEdit(k.env_var)}
                        className="px-3 py-1 text-xs rounded border border-gray-300 text-gray-600 hover:bg-gray-100"
                      >
                        {k.set ? 'Change' : 'Add Key'}
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
