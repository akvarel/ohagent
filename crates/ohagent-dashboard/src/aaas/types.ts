export interface Process { id: string; version: number; title: string; goal: string; expected_result: string; materials: string }
export type ProcessInput = Pick<Process, 'title' | 'goal' | 'expected_result' | 'materials'>
export interface Task { id: string; agent_definition_id: string; agent_instance_id?: string; attempt_id: string; status: string; summary: string; started_at?: string; completed_at?: string; error_code?: string }
export interface Edge { id: string; from: string; to: string; status: 'planned' | 'dispatched' | 'completed' }
export interface Run { id: string; process_id: string; process_version: number; title: string; goal: string; status: string; version: number; watermark: number; created_at: string; updated_at: string; tasks: Task[]; edges: Edge[]; artifact?: { id: string; verification: unknown; status?: string }; usage?: { tokens?: number | null; duration_ms?: number | null } }
export interface DurableEvent { schema_version: 1; event_id: string; type: string; run_id: string; run_seq: number; occurred_at: string; recorded_at: string; task_id?: string; attempt_id?: string; agent_definition_id?: string; agent_instance_id?: string; payload: Record<string, unknown> }
export interface PublicConfig { auth: { issuer: string; client_id: string; authorization_endpoint: string; token_endpoint: string; logout_endpoint?: string } | null; pilot_available: boolean }
