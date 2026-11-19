export type Activity = 'idle' | 'working' | 'waiting' | 'completed' | 'cancelled' | 'failed';
export interface SessionActivity {
  id: string;
  status: Activity;
  updated_at: number;
  started_at: number;
  current_tool: string | null;
  tools: Record<string, number>;
  input_tokens: number;
  output_tokens: number;
  events: { at: number; kind: Activity; tool: string | null }[];
}
export interface ReceivedSnapshot {
  received_at: number;
  stale: boolean;
  snapshot: { source_id: string; boot_id: string; generated_at: number; sequence: number; sessions: SessionActivity[] };
}

export function isStale(source: ReceivedSnapshot, now: number): boolean {
  return source.stale || now - source.received_at > 90_000;
}

export async function fetchProgress(tenant: string, token: string, signal: AbortSignal): Promise<ReceivedSnapshot[]> {
  const response = await fetch(`/api/progress?${new URLSearchParams({ tenant_id: tenant })}`, {
    headers: { Authorization: `Bearer ${token}` }, signal, cache: 'no-store',
  });
  if (!response.ok) throw new Error(String(response.status));
  return response.json();
}
