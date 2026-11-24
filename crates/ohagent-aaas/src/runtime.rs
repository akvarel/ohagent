use crate::{
    model::*,
    store::{now, Store},
    BackendError,
};
use std::sync::Arc;

#[async_trait::async_trait]
pub trait Runtime: Send + Sync {
    async fn execute(&self, execution: RunExecution, sink: EventSink) -> Result<(), String>;
}
#[derive(Clone)]
pub struct EventSink {
    pub(crate) store: Arc<Store>,
    pub(crate) tenant: String,
    pub(crate) run: String,
    pub(crate) epoch: String,
}
impl EventSink {
    /// Persist dispatch before invoking the receiving runtime. Start is recorded separately.
    pub fn dispatch(&self, role: &str) -> Result<(), BackendError> {
        self.store.update(
            &self.tenant,
            &self.run,
            Some(&self.epoch),
            "task.dispatched",
            |run| {
                active(run)?;
                let index = run
                    .tasks
                    .iter()
                    .position(|t| t.agent_definition_id == role)
                    .ok_or(BackendError::Invalid)?;
                if run.tasks[index].status != "queued"
                    || !predecessors_completed(run, &run.tasks[index].id)
                {
                    return Err(BackendError::Conflict);
                }
                let tid = run.tasks[index].id.clone();
                for edge in &mut run.edges {
                    if edge.to == tid {
                        edge.status = "dispatched".into();
                    }
                }
                Ok(Some(run.tasks[index].clone()))
            },
        )?;
        Ok(())
    }
    /// Call only after Jcode has actually created the invocation/session instance.
    pub fn start_instance(&self, role: &str, instance_id: &str) -> Result<Task, BackendError> {
        if instance_id.is_empty() || instance_id.len() > 200 {
            return Err(BackendError::Invalid);
        }
        let run = self.store.update(
            &self.tenant,
            &self.run,
            Some(&self.epoch),
            "task.started",
            |run| {
                active(run)?;
                let index = run
                    .tasks
                    .iter()
                    .position(|t| t.agent_definition_id == role)
                    .ok_or(BackendError::Invalid)?;
                if run.tasks[index].status != "queued"
                    || !predecessors_completed(run, &run.tasks[index].id)
                    || run
                        .edges
                        .iter()
                        .any(|e| e.to == run.tasks[index].id && e.status != "dispatched")
                {
                    return Err(BackendError::Conflict);
                }
                if run
                    .tasks
                    .iter()
                    .any(|t| t.agent_instance_id.as_deref() == Some(instance_id))
                {
                    return Err(BackendError::Conflict);
                }
                run.status = "running".into();
                let task = &mut run.tasks[index];
                task.status = "running".into();
                task.agent_instance_id = Some(instance_id.into());
                task.started_at = Some(now());
                Ok(Some(task.clone()))
            },
        )?;
        run.tasks
            .into_iter()
            .find(|t| t.agent_definition_id == role)
            .ok_or(BackendError::Invalid)
    }
    pub fn complete_task(&self, task_id: &str, summary: &str) -> Result<(), BackendError> {
        if summary.len() > 2000 {
            return Err(BackendError::Invalid);
        }
        self.store.update(
            &self.tenant,
            &self.run,
            Some(&self.epoch),
            "task.completed",
            |run| {
                active(run)?;
                let task = run
                    .tasks
                    .iter_mut()
                    .find(|t| t.id == task_id)
                    .ok_or(BackendError::Invalid)?;
                if task.status != "running" {
                    return Err(BackendError::Conflict);
                }
                task.status = "completed".into();
                task.summary = summary.into();
                task.completed_at = Some(now());
                for edge in &mut run.edges {
                    if edge.to == task.id {
                        edge.status = "completed".into();
                    }
                }
                Ok(Some(task.clone()))
            },
        )?;
        Ok(())
    }
    pub fn complete(&self, artifact: &str) -> Result<(), BackendError> {
        self.store
            .complete(&self.tenant, &self.run, &self.epoch, artifact)
    }
    pub fn fail(&self, _code: &str) -> Result<(), BackendError> {
        self.store.update(
            &self.tenant,
            &self.run,
            Some(&self.epoch),
            "run.failed",
            |run| {
                active(run)?;
                run.status = "failed".into();
                for t in &mut run.tasks {
                    if t.status == "running" || t.status == "queued" {
                        t.status = "failed".into();
                        t.error_code = Some("runtime_failed".into());
                        t.completed_at = Some(now());
                    }
                }
                Ok(None)
            },
        )?;
        Ok(())
    }
    pub fn is_cancelled(&self) -> bool {
        self.store.run(&self.tenant, &self.run).map_or(true, |r| {
            r.status == "cancellation_requested" || r.terminal()
        })
    }
    pub(crate) fn interrupt(&self) -> Result<(), BackendError> {
        self.store.update(
            &self.tenant,
            &self.run,
            Some(&self.epoch),
            "run.interrupted",
            |run| {
                run.status = "interrupted".into();
                for task in &mut run.tasks {
                    if task.status == "queued" || task.status == "running" {
                        task.status = "failed".into();
                        task.error_code = Some("runtime_interrupted".into());
                        task.completed_at = Some(now());
                    }
                }
                Ok(None)
            },
        )?;
        Ok(())
    }
    pub fn confirm_cancelled(&self) -> Result<(), BackendError> {
        self.store.update(
            &self.tenant,
            &self.run,
            Some(&self.epoch),
            "run.cancelled",
            |run| {
                if run.status != "cancellation_requested" {
                    return Err(BackendError::Conflict);
                }
                run.status = "cancelled".into();
                for t in &mut run.tasks {
                    if t.status == "running" || t.status == "queued" {
                        t.status = "cancelled".into();
                        t.completed_at = Some(now());
                    }
                }
                Ok(None)
            },
        )?;
        Ok(())
    }
}
/// Readiness follows stored incoming edges, never task array order. Unknown sources fail closed.
fn predecessors_completed(run: &Run, task_id: &str) -> bool {
    run.edges
        .iter()
        .filter(|edge| edge.to == task_id)
        .all(|edge| {
            run.tasks
                .iter()
                .any(|task| task.id == edge.from && task.status == "completed")
        })
}
fn active(run: &Run) -> Result<(), BackendError> {
    if run.status == "cancellation_requested" || run.terminal() {
        Err(BackendError::Fenced)
    } else {
        Ok(())
    }
}
