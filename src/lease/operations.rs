use super::{Authority, now};
use crate::lease::transport::Output;
use crate::{AppError, AppResult};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

const RESULT_LIFETIME: u64 = 24 * 60 * 60;
const RESULT_LIMIT: usize = 256;
const RESULT_BYTES: usize = 1024 * 1024 * 1024;
const OPERATION_RESERVATION: usize = 64 * 1024 * 1024;

pub struct Operations {
    authority: Arc<Authority>,
    entries: Mutex<HashMap<String, Arc<Operation>>>,
    concurrency: usize,
    views: Arc<Mutex<usize>>,
}

pub struct Operation {
    authority: Arc<Authority>,
    pub id: String,
    pub cancelled: Arc<AtomicBool>,
    state: Mutex<State>,
    views: Arc<Mutex<usize>>,
}

struct State {
    subscribers: Vec<Weak<Output>>,
    progress: Option<Value>,
    result: Option<Value>,
    completed_at: Option<u64>,
}

impl Operations {
    pub fn new(authority: Arc<Authority>, concurrency: usize) -> Self {
        Self {
            authority,
            entries: Mutex::new(HashMap::new()),
            concurrency,
            views: Arc::new(Mutex::new(0)),
        }
    }

    pub fn admit(
        &self,
        cancelled: Arc<AtomicBool>,
        output: &Arc<Output>,
    ) -> AppResult<Arc<Operation>> {
        self.authority
            .verify()
            .map_err(|error| AppError::command(error.to_string()))?;
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|_, operation| !operation.expired());
        let active = entries
            .values()
            .filter(|operation| !operation.complete())
            .count();
        let retained_bytes: usize = entries
            .values()
            .map(|operation| operation.retained_bytes())
            .sum();
        if entries.len() >= RESULT_LIMIT
            || active >= self.concurrency
            || retained_bytes.saturating_add((active + 1) * OPERATION_RESERVATION) > RESULT_BYTES
        {
            return Err(AppError::command(
                "native authority operation capacity reached; fetch completed results before accepting more work",
            ));
        }
        let operation = Arc::new(Operation {
            authority: Arc::clone(&self.authority),
            id: uuid::Uuid::new_v4().to_string(),
            cancelled,
            views: Arc::clone(&self.views),
            state: Mutex::new(State {
                subscribers: vec![Arc::downgrade(output)],
                progress: None,
                result: None,
                completed_at: None,
            }),
        });
        entries.insert(operation.id.clone(), Arc::clone(&operation));
        Ok(operation)
    }

    pub fn get(&self, id: &str) -> AppResult<Value> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|_, operation| !operation.expired());
        let operation = entries
            .get(id)
            .ok_or_else(|| AppError::invalid("unknown or expired operation"))?;
        Ok(operation.snapshot())
    }

    pub fn fetched(&self, id: &str) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries
            .get(id)
            .is_some_and(|operation| operation.complete())
        {
            entries.remove(id);
        }
    }

    pub fn list(&self) -> Value {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        json!(
            entries
                .values()
                .filter(|operation| !operation.expired())
                .map(|operation| { json!({"op": operation.id, "complete": operation.complete()}) })
                .collect::<Vec<_>>()
        )
    }

    pub fn cancel(&self, id: &str) -> bool {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(operation) = entries.get(id)
            && !operation.complete()
        {
            operation.cancelled.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }

    pub fn subscribe(&self, id: &str, output: &Arc<Output>) -> AppResult<Value> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let operation = entries
            .get(id)
            .filter(|operation| !operation.expired())
            .ok_or_else(|| AppError::invalid("unknown or expired operation"))?;
        let mut state = operation
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .subscribers
            .retain(|subscriber| subscriber.strong_count() > 0);
        if !state
            .subscribers
            .iter()
            .any(|subscriber| subscriber.ptr_eq(&Arc::downgrade(output)))
        {
            if state.subscribers.len() >= 32 {
                return Err(AppError::command("operation subscriber limit reached"));
            }
            state.subscribers.push(Arc::downgrade(output));
        }
        drop(state);
        Ok(operation.snapshot())
    }

    pub fn busy(&self) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .any(|operation| !operation.complete())
    }

    pub fn write_mode(&self) -> super::WriteMode {
        self.authority.write_mode()
    }

    pub fn authority_lost(&self) -> bool {
        self.authority.verify().is_err()
    }

    pub fn stop_after_identity_loss(&self) {
        for operation in self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
        {
            if !operation.complete() {
                operation.cancelled.store(true, Ordering::Release);
            }
        }
    }

    pub fn attach_view(&self) {
        *self
            .views
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
    }

    pub fn detach_view(&self) {
        let mut views = self
            .views
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *views -= 1;
        if *views == 0 && self.authority.verify().is_ok() {
            crate::hyprland::restore_owned_borders();
        }
    }
}

impl Operation {
    pub fn publish(
        &self,
        mut frame: Value,
        terminal: bool,
        primary: Option<&dyn Fn(&Output, &Value) -> std::io::Result<()>>,
    ) -> bool {
        let authority_lost = self.authority.verify().is_err();
        if terminal && authority_lost {
            frame["ok"] = Value::Bool(false);
            frame["error_id"] = Value::String("authority-lost".into());
            frame["error"] = Value::String(
                "authority-lost: storage identity changed; inspect recoverable partial output"
                    .into(),
            );
            frame["cancelled"] = Value::Bool(false);
            if let Some(payload) = frame.get_mut("payload").and_then(Value::as_object_mut) {
                payload.insert("ok".into(), Value::Bool(false));
                payload.insert("cancelled".into(), Value::Bool(false));
                payload.insert("partial".into(), Value::Bool(true));
                payload.insert("error_id".into(), Value::String("authority-lost".into()));
            }
        }
        let subscribers = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if terminal {
                state.result = Some(frame.clone());
                state.completed_at = Some(now());
            } else {
                state.progress = Some(frame.clone());
            }
            state
                .subscribers
                .retain(|subscriber| subscriber.strong_count() > 0);
            state
                .subscribers
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        if terminal && !authority_lost {
            let views = self
                .views
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *views == 0 {
                crate::hyprland::restore_owned_borders();
            }
        }
        let mut first = true;
        let mut delivered = false;
        for output in subscribers {
            let result = if terminal && first {
                first = false;
                delivered = true;
                primary.map_or_else(
                    || output.machine(&frame),
                    |deliver| deliver(&output, &frame),
                )
            } else {
                output.machine(&frame)
            };
            if result.is_err() {
                output.detach();
                self.state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .subscribers
                    .retain(|subscriber| !subscriber.ptr_eq(&Arc::downgrade(&output)));
            }
        }
        delivered
    }

    pub fn snapshot(&self) -> Value {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        json!({"op": self.id, "complete": state.completed_at.is_some(), "progress": state.progress, "result": state.result})
    }

    fn complete(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .completed_at
            .is_some()
    }

    fn expired(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .completed_at
            .is_some_and(|at| now().saturating_sub(at) >= RESULT_LIFETIME)
    }

    fn retained_bytes(&self) -> usize {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        [&state.progress, &state.result]
            .into_iter()
            .flatten()
            .map(|value| value.to_string().len())
            .sum()
    }
}
