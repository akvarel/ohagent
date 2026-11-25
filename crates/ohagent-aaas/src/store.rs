use crate::{model::*, BackendError};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex};

pub(crate) fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub(crate) fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
pub(crate) struct Store(pub Mutex<Connection>);
fn check_capacity(
    tx: &Transaction<'_>,
    table: &str,
    tenant: &str,
    per_tenant: u64,
    global: u64,
) -> Result<(), BackendError> {
    let sql = match table {
        "aaas_processes" => "SELECT COUNT(*), COALESCE(SUM(tenant=?),0) FROM aaas_processes",
        "aaas_runs" => "SELECT COUNT(*), COALESCE(SUM(tenant=?),0) FROM aaas_runs",
        _ => return Err(BackendError::Storage),
    };
    let (total, own): (u64, u64) = tx.query_row(sql, [tenant], |r| Ok((r.get(0)?, r.get(1)?)))?;
    if total >= global || own >= per_tenant {
        return Err(BackendError::Capacity);
    }
    Ok(())
}
impl Store {
    pub fn open(path: &Path) -> Result<Self, BackendError> {
        let mut c = Connection::open(path)?;
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS aaas_processes(tenant TEXT NOT NULL,id TEXT NOT NULL,version INTEGER NOT NULL,body TEXT NOT NULL,PRIMARY KEY(tenant,id,version));
        CREATE TABLE IF NOT EXISTS aaas_runs(tenant TEXT NOT NULL,id TEXT NOT NULL,epoch TEXT NOT NULL,body TEXT NOT NULL,artifact TEXT,PRIMARY KEY(tenant,id));
        CREATE TABLE IF NOT EXISTS aaas_keys(tenant TEXT NOT NULL,key TEXT NOT NULL,hash TEXT NOT NULL,run_id TEXT NOT NULL,PRIMARY KEY(tenant,key));
        CREATE TABLE IF NOT EXISTS aaas_instances(tenant TEXT NOT NULL,instance_id TEXT NOT NULL,run_id TEXT NOT NULL,task_id TEXT NOT NULL,PRIMARY KEY(tenant,instance_id));
        CREATE TABLE IF NOT EXISTS aaas_dispatches(tenant TEXT NOT NULL,run_id TEXT NOT NULL,task_id TEXT NOT NULL,epoch TEXT NOT NULL,PRIMARY KEY(tenant,run_id,task_id));
        CREATE TABLE IF NOT EXISTS aaas_events(tenant TEXT NOT NULL,run_id TEXT NOT NULL,seq INTEGER NOT NULL,body TEXT NOT NULL,PRIMARY KEY(tenant,run_id,seq));")?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows = {
            let mut q = tx.prepare("SELECT tenant,body FROM aaas_runs")?;
            let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (tenant, body) in rows {
            let mut run: Run = serde_json::from_str(&body)?;
            if !run.terminal() {
                run.status = "interrupted".into();
                for task in &mut run.tasks {
                    if task.status == "running" || task.status == "queued" {
                        task.status = "failed".into();
                        task.error_code = Some("runtime_interrupted".into());
                    }
                }
                append(&tx, &tenant, &mut run, "run.interrupted", None)?;
                tx.execute(
                    "UPDATE aaas_runs SET epoch=? WHERE tenant=? AND id=?",
                    params![id(), tenant, run.id],
                )?;
            }
        }
        tx.commit()?;
        Ok(Self(Mutex::new(c)))
    }
    pub fn process(
        &self,
        tenant: &str,
        input: ProcessInput,
        existing: Option<&str>,
    ) -> Result<Process, BackendError> {
        if !input.valid() {
            return Err(BackendError::Invalid);
        }
        let mut c = self.0.lock().map_err(|_| BackendError::Storage)?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (pid, version) = if let Some(pid) = existing {
            let version: Option<u64> = tx.query_row(
                "SELECT MAX(version) FROM aaas_processes WHERE tenant=? AND id=?",
                params![tenant, pid],
                |r| r.get(0),
            )?;
            (pid.to_owned(), version.ok_or(BackendError::NotFound)? + 1)
        } else {
            (id(), 1)
        };
        check_capacity(&tx, "aaas_processes", tenant, 256, 4096)?;
        let p = Process {
            id: pid,
            version,
            input,
        };
        tx.execute(
            "INSERT INTO aaas_processes VALUES(?,?,?,?)",
            params![tenant, p.id, p.version, serde_json::to_string(&p)?],
        )?;
        tx.commit()?;
        Ok(p)
    }
    pub fn processes(&self, tenant: &str) -> Result<Vec<Process>, BackendError> {
        let c = self.0.lock().map_err(|_| BackendError::Storage)?;
        let mut q = c.prepare("SELECT body FROM aaas_processes p WHERE tenant=? AND version=(SELECT MAX(version) FROM aaas_processes p2 WHERE p2.tenant=p.tenant AND p2.id=p.id) ORDER BY id LIMIT 100")?;
        let rows = q.query_map([tenant], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn repeat(
        &self,
        tenant: &str,
        key: &str,
        req: &StartRequest,
    ) -> Result<Option<Run>, BackendError> {
        if key.is_empty() || key.len() > 200 || req.process_id.len() > 100 {
            return Err(BackendError::Invalid);
        }
        let c = self.0.lock().map_err(|_| BackendError::Storage)?;
        let prior: Option<(String, String)> = c
            .query_row(
                "SELECT hash,run_id FROM aaas_keys WHERE tenant=? AND key=?",
                params![tenant, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((fingerprint, rid)) = prior {
            if fingerprint != hash(&serde_json::to_string(req)?) {
                return Err(BackendError::Conflict);
            }
            Ok(Some(load(&c, tenant, &rid)?.0))
        } else {
            Ok(None)
        }
    }
    pub fn start(
        &self,
        tenant: &str,
        key: &str,
        req: &StartRequest,
    ) -> Result<(Run, Option<RunExecution>), BackendError> {
        if key.is_empty() || key.len() > 200 || req.process_id.len() > 100 {
            return Err(BackendError::Invalid);
        }
        let fingerprint = hash(&serde_json::to_string(req)?);
        let mut c = self.0.lock().map_err(|_| BackendError::Storage)?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: Option<(String, String)> = tx
            .query_row(
                "SELECT hash,run_id FROM aaas_keys WHERE tenant=? AND key=?",
                params![tenant, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((h, rid)) = prior {
            if h != fingerprint {
                return Err(BackendError::Conflict);
            }
            return Ok((load(&tx, tenant, &rid)?.0, None));
        }
        check_capacity(&tx, "aaas_runs", tenant, 100, 1000)?;
        let body: String = tx
            .query_row(
                "SELECT body FROM aaas_processes WHERE tenant=? AND id=? AND version=?",
                params![tenant, req.process_id, req.process_version],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(BackendError::NotFound)?;
        let process: Process = serde_json::from_str(&body)?;
        let tasks: Vec<Task> = ["coordinator", "analyst", "evidence_checker", "reviewer"]
            .into_iter()
            .map(|role| Task {
                id: id(),
                agent_definition_id: role.into(),
                agent_instance_id: None,
                attempt_id: id(),
                status: "queued".into(),
                summary: String::new(),
                started_at: None,
                completed_at: None,
                error_code: None,
            })
            .collect();
        let edges = [(0, 1), (0, 2), (1, 3), (2, 3)]
            .into_iter()
            .map(|(from, to)| Edge {
                id: id(),
                from: tasks[from].id.clone(),
                to: tasks[to].id.clone(),
                status: "planned".into(),
            })
            .collect();
        let mut run = Run {
            id: id(),
            process_id: process.id.clone(),
            process_version: process.version,
            title: process.input.title.clone(),
            goal: process.input.goal.clone(),
            status: "queued".into(),
            version: 0,
            watermark: 0,
            created_at: now(),
            updated_at: now(),
            tasks,
            edges,
            artifact: None,
        };
        let epoch = id();
        tx.execute(
            "INSERT INTO aaas_runs(tenant,id,epoch,body) VALUES(?,?,?,?)",
            params![tenant, run.id, epoch, serde_json::to_string(&run)?],
        )?;
        append(&tx, tenant, &mut run, "run.created", None)?;
        tx.execute(
            "INSERT INTO aaas_keys VALUES(?,?,?,?)",
            params![tenant, key, fingerprint, run.id],
        )?;
        tx.commit()?;
        let execution = RunExecution {
            tenant_id: tenant.into(),
            run_id: run.id.clone(),
            process,
            producer_epoch: epoch,
        };
        Ok((run, Some(execution)))
    }
    pub fn run(&self, tenant: &str, rid: &str) -> Result<Run, BackendError> {
        let c = self.0.lock().map_err(|_| BackendError::Storage)?;
        Ok(load(&c, tenant, rid)?.0)
    }
    pub fn runs(&self, tenant: &str) -> Result<Vec<Run>, BackendError> {
        let c = self.0.lock().map_err(|_| BackendError::Storage)?;
        let mut q =
            c.prepare("SELECT body FROM aaas_runs WHERE tenant=? ORDER BY rowid DESC LIMIT 100")?;
        let rows = q.query_map([tenant], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn update<F>(
        &self,
        tenant: &str,
        rid: &str,
        epoch: Option<&str>,
        kind: &str,
        f: F,
    ) -> Result<Run, BackendError>
    where
        F: FnOnce(&mut Run) -> Result<Option<Task>, BackendError>,
    {
        let mut c = self.0.lock().map_err(|_| BackendError::Storage)?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (mut run, current) = load(&tx, tenant, rid)?;
        if let Some(epoch) = epoch {
            if current != epoch || run.terminal() {
                return Err(BackendError::Fenced);
            }
        }
        let task = f(&mut run)?;
        if let Some(task) = task.as_ref() {
            if kind == "task.dispatched" {
                let inserted = tx.execute(
                    "INSERT OR IGNORE INTO aaas_dispatches VALUES(?,?,?,?)",
                    params![tenant, rid, task.id, current],
                )?;
                if inserted != 1 {
                    return Err(BackendError::Conflict);
                }
            } else if kind == "task.started" {
                let dispatched:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM aaas_dispatches WHERE tenant=? AND run_id=? AND task_id=? AND epoch=?)",params![tenant,rid,task.id,current],|r|r.get(0))?;
                if !dispatched {
                    return Err(BackendError::Conflict);
                }
                let instance = task
                    .agent_instance_id
                    .as_ref()
                    .ok_or(BackendError::Invalid)?;
                let inserted = tx.execute(
                    "INSERT OR IGNORE INTO aaas_instances VALUES(?,?,?,?)",
                    params![tenant, instance, rid, task.id],
                )?;
                if inserted != 1 {
                    return Err(BackendError::Conflict);
                }
            }
        }
        append(&tx, tenant, &mut run, kind, task.as_ref())?;
        tx.commit()?;
        Ok(run)
    }
    pub fn complete(
        &self,
        tenant: &str,
        rid: &str,
        epoch: &str,
        artifact: &str,
    ) -> Result<(), BackendError> {
        let verification = verify_artifact(artifact)?;
        let mut c = self.0.lock().map_err(|_| BackendError::Storage)?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (mut run, current) = load(&tx, tenant, rid)?;
        if current != epoch || run.terminal() || run.status == "cancellation_requested" {
            return Err(BackendError::Fenced);
        }
        if !run.tasks.iter().all(|t| t.status == "completed") {
            return Err(BackendError::Conflict);
        }
        run.artifact = Some(Artifact {
            id: id(),
            verification: verification.into(),
        });
        tx.execute(
            "UPDATE aaas_runs SET artifact=? WHERE tenant=? AND id=?",
            params![artifact, tenant, rid],
        )?;
        append(&tx, tenant, &mut run, "artifact.verified", None)?;
        run.status = "completed".into();
        append(&tx, tenant, &mut run, "run.completed", None)?;
        tx.commit()?;
        Ok(())
    }
    pub fn artifact(&self, tenant: &str, rid: &str) -> Result<String, BackendError> {
        let c = self.0.lock().map_err(|_| BackendError::Storage)?;
        let run = load(&c, tenant, rid)?.0;
        if run.status != "completed" {
            return Err(BackendError::NotFound);
        }
        c.query_row(
            "SELECT artifact FROM aaas_runs WHERE tenant=? AND id=?",
            params![tenant, rid],
            |r| r.get(0),
        )
        .map_err(Into::into)
    }
    pub fn events(
        &self,
        tenant: &str,
        rid: &str,
        after: u64,
    ) -> Result<(Vec<Event>, bool), BackendError> {
        let c = self.0.lock().map_err(|_| BackendError::Storage)?;
        let run = load(&c, tenant, rid)?.0;
        if after > run.watermark {
            return Err(BackendError::Cursor);
        }
        let minimum: Option<u64> = c.query_row(
            "SELECT MIN(seq) FROM aaas_events WHERE tenant=? AND run_id=?",
            params![tenant, rid],
            |r| r.get(0),
        )?;
        if minimum.is_some_and(|m| after.saturating_add(1) < m) {
            return Err(BackendError::Cursor);
        }
        let mut q=c.prepare("SELECT body FROM aaas_events WHERE tenant=? AND run_id=? AND seq>? ORDER BY seq LIMIT 64")?;
        let rows = q.query_map(params![tenant, rid, after], |r| r.get::<_, String>(0))?;
        let events = rows
            .map(|r| Ok(serde_json::from_str(&r?)?))
            .collect::<Result<Vec<Event>, BackendError>>()?;
        let done = run.terminal() && events.last().map_or(after, |e| e.run_seq) >= run.watermark;
        Ok((events, done))
    }
}
/// Structural verification only, not a claim of factual/semantic correctness.
fn verify_artifact(artifact: &str) -> Result<&'static str, BackendError> {
    if artifact.trim().is_empty()
        || artifact.len() > 200_000
        || artifact
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(BackendError::Invalid);
    }
    Ok("nonempty_bounded_text_v1")
}
fn load(c: &Connection, tenant: &str, rid: &str) -> Result<(Run, String), BackendError> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT body,epoch FROM aaas_runs WHERE tenant=? AND id=?",
            params![tenant, rid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (body, epoch) = row.ok_or(BackendError::NotFound)?;
    Ok((serde_json::from_str(&body)?, epoch))
}
fn append(
    tx: &Transaction<'_>,
    tenant: &str,
    run: &mut Run,
    kind: &str,
    task: Option<&Task>,
) -> Result<(), BackendError> {
    run.version += 1;
    run.watermark += 1;
    run.updated_at = now();
    let event = Event {
        schema_version: 1,
        event_id: id(),
        kind: kind.into(),
        run_id: run.id.clone(),
        run_seq: run.watermark,
        occurred_at: now(),
        recorded_at: now(),
        task_id: task.map(|t| t.id.clone()),
        attempt_id: task.map(|t| t.attempt_id.clone()),
        agent_definition_id: task.map(|t| t.agent_definition_id.clone()),
        agent_instance_id: task.and_then(|t| t.agent_instance_id.clone()),
        payload: json!({"status":run.status,"version":run.version,"snapshot":run}),
    };
    tx.execute(
        "INSERT INTO aaas_events VALUES(?,?,?,?)",
        params![
            tenant,
            run.id,
            event.run_seq,
            serde_json::to_string(&event)?
        ],
    )?;
    tx.execute(
        "UPDATE aaas_runs SET body=? WHERE tenant=? AND id=?",
        params![serde_json::to_string(run)?, tenant, run.id],
    )?;
    Ok(())
}
