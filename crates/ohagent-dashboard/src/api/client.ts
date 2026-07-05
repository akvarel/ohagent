// API client for ohAgent REST API.
// In dev, Vite proxies /api → localhost:9090.

const BASE = '';

export interface Status {
  service: string;
  version: string;
  uptime_seconds: number;
  provider: string;
  skills_count: number;
  memory_count: number;
  skills_enabled: boolean;
  memory_enabled: boolean;
}

export interface SkillSummary {
  id: string;
  name: string;
  status: string;
  version: string;
  quality_score: number;
  use_count: number;
  triggers: string[];
  tags: string[];
}

export interface SkillDetail {
  id: string;
  tenant_id: string;
  name: string;
  description: string;
  triggers: string[];
  instructions: string;
  version: string;
  status: string;
  origin: string;
  quality_score: number;
  use_count: number;
  success_count: number;
  failure_count: number;
  tags: string[];
  created_at: string;
  updated_at: string;
  last_used_at: string | null;
}

export interface MemorySummary {
  id: string;
  tenant_id: string;
  session_id: string;
  content: string;
  source: string;
  importance: number;
  created_at: string;
  access_count: number;
  tags: string[];
}

async function get<T>(url: string): Promise<T> {
  const res = await fetch(BASE + url);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

async function post<T>(url: string, body?: unknown): Promise<T> {
  const res = await fetch(BASE + url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

export const api = {
  health: () => get<{ status: string }>('/health'),
  status: () => get<Status>('/api/status'),
  skills: (tenant?: string, status?: string) => {
    const params = new URLSearchParams();
    if (tenant) params.set('tenant_id', tenant);
    if (status) params.set('status', status);
    return get<SkillSummary[]>(`/api/skills?${params}`);
  },
  skill: (id: string) => get<SkillDetail>(`/api/skills/${id}`),
  recordSkillUse: (id: string, success: boolean, tenant?: string) =>
    post<{ ok: boolean }>(`/api/skills/${id}/record`, { success, tenant_id: tenant }),
  memory: (tenant?: string, q?: string, limit?: number) => {
    const params = new URLSearchParams();
    if (tenant) params.set('tenant_id', tenant);
    if (q) params.set('q', q);
    if (limit) params.set('limit', String(limit));
    return get<MemorySummary[]>(`/api/memory?${params}`);
  },
  memoryEntry: (id: string) => get<MemorySummary>(`/api/memory/${id}`),
};
