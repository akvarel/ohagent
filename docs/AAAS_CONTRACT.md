# AaaS v1 integration contract (2026-11-24)

## Boundary
ohAgent remains orchestrator, JcodeBridge remains sole model/tool execution path. Standard Agent Specification 0.2.0 is target agent contract, not a conformance claim. Initial pilot: analytical brief from supplied text materials. Definitions: coordinator, analyst, evidence_checker, reviewer. The coordinator precedes two genuinely concurrent Jcode sessions, followed by a reviewer join after both complete. Each actual invocation records a distinct session instance. Any unsupported standard fields fail closed, never silently flatten hooks/tools.

## HTTP
New API prefix `/api/aaas`. Authenticated server-resolved tenant and subject only. Browser must not provide tenant identifiers. Existing operator middleware must not require the platform key on AaaS routes. Bearer OIDC JWT validation via configured issuer/audience and JWKS, with server-controlled configuration. No new passwords or operator API key in browser. OIDC public PKCE client configuration exposed by GET `/api/aaas/config` when configured. If unavailable, return explicit unavailable code.

- GET `/api/aaas/config`: `{auth:{issuer,client_id,authorization_endpoint,token_endpoint,logout_endpoint?},pilot_available:boolean}` or auth null. No secrets.
- GET `/api/aaas/processes`: `{processes:[{id,version,title,goal,expected_result,materials}],template:{id:"analytical-brief",roles:["coordinator","analyst","evidence_checker","reviewer"],timeout_seconds:300}}`
- POST `/api/aaas/processes`: `{title,goal,expected_result,materials}` -> saved process with id/version. Materials plain text, bounded. PUT `/api/aaas/processes/:id` updates version.
- POST `/api/aaas/runs` header `Idempotency-Key`: `{process_id,process_version}` -> Run snapshot, status 201 first, 200 repeat. Key scoped to tenant/subject, canonical payload hash rejects conflict 409.
- GET `/api/aaas/runs`: `{runs: Run[]}`
- GET `/api/aaas/runs/:id`: Run snapshot.
- POST `/api/aaas/runs/:id/cancel`: `{version:number}` -> Run snapshot with cancellation_requested until engine confirms cancellation.
- GET `/api/aaas/runs/:id/events?after=N`: authenticated SSE after durable cursor. JSON event envelope, ids sequence numbers; reset_required for invalid/expired cursor. Fetch streaming with Authorization, not token URL. Snapshot watermark and stream share same durable journal. Bounded delivery batches and heartbeat.
- GET `/api/aaas/runs/:id/artifact`: authenticated text/markdown artifact, never public object path.

Run: `{id,process_id,process_version,title,goal,status,version,watermark,created_at,updated_at,tasks:Task[],edges:Edge[],artifact?:{id,verification,status?},usage?:{tokens?:number,duration_ms?:number}}`. Unknown usage omitted/null, not zero.
Task: `{id,agent_definition_id,agent_instance_id?:string,attempt_id,status,summary,started_at?,completed_at?,error_code?}`. IDs are opaque server-generated. Task statuses queued/running/completed/failed/cancelled. Run statuses queued/running/cancellation_requested/cancelled/completed/failed/interrupted. Completion requires artifact and separate deterministic verification.
Edge: `{id,from,to,status:"planned"|"dispatched"|"completed"}`. Dispatch durable before calling recipient; actual started only after runtime instance creation. The pilot DAG is coordinator → {analyst, evidence_checker} → reviewer. Both incoming predecessors must complete before reviewer dispatch and start. Actual parallel session intervals were verified in the opt-in provider-backed acceptance test. Labelled fixtures separately exercise both completion orders, restart and cancellation fencing.
Event: `{schema_version:1,event_id,type,run_id,run_seq,occurred_at,recorded_at,task_id?,attempt_id?,agent_definition_id?,agent_instance_id?,payload:{...allowlisted}}`. No raw prompts, tools or reasoning in events. Client reducers deduplicate event_id and reject older watermarks. Replay uses stored events, never commands.

## Runtime boundary for backend crate
Runtime trait should accept a RunExecution input including server-scoped tenant/run and pilot inputs, plus a durable event sink with guarded updates. Coordinator integration owns JcodeBridge adapter. No new web agent loop, scheduler, provider or router in backend crate. Fail closed without configured runtime/auth.

## Routing
Public pages `/`, `/agent-as-a-service`, labelled demo `/agent-as-a-service/demo`. Client `/agent-as-a-service/app/*`. Preserve legacy operator dashboard under `/operator`, plus `/progress`, `/skills`, `/memory`, `/keys`, `/usage`. All nested routes SPA fallback. Backend AaaS API same origin.

## Standard specification gap
The trusted TypeScript package is checked against pinned official types and exported at build time to a strict text-only profile consumed by ohAgent. Unsupported fields fail closed. This supports the declared pilot, not arbitrary npm agent packages or complete specification conformance.
