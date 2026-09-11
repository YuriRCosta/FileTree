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
}

pub struct Operation {
    pub id: String,
    pub cancelled: Arc<AtomicBool>,
    state: Mutex<State>,
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
            id: uuid::Uuid::new_v4().to_string(),
            cancelled,
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
}

impl Operation {
    pub fn publish(&self, frame: Value, terminal: bool) {
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
        for output in subscribers {
            if output.machine(&frame).is_err() {
                output.detach();
                self.state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .subscribers
                    .retain(|subscriber| !subscriber.ptr_eq(&Arc::downgrade(&output)));
            }
        }
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
