//! First production adapter for ADR 0055: caller-owned display fetches.
//! Admission never spawns a task. Existing ordinary-fetch permits remain in
//! force while other protocols migrate to the shared account owner.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use kukuri_transport::work_admission::{
    NetworkWorkOwner, WorkAdmission, WorkCompletion, WorkId, WorkKey, WorkLane, WorkLimits,
    WorkMode, WorkPersistence, WorkProtocol, WorkScope, WorkWaiter,
};
use tokio::sync::Notify;
use tokio::time::{Instant, timeout_at};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayAdmissionError {
    Deferred,
    Expired,
    Closed,
}

impl std::fmt::Display for DisplayAdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "display fetch admission: {self:?}")
    }
}

impl std::error::Error for DisplayAdmissionError {}

struct State {
    policy: NetworkWorkOwner,
    scopes: BTreeMap<WorkId, WorkScope>,
    ready: BTreeSet<WorkId>,
    cancelled: BTreeSet<WorkId>,
    closed: bool,
}

impl State {
    fn advance(&mut self) {
        let now = Instant::now().into_std();
        self.policy.expire(now);
        while let Some(id) = self.policy.next_cancellation() {
            self.cancelled.insert(id);
        }
        if !self.closed {
            while let Some(work) = self.policy.start_next(now) {
                self.ready.insert(work.id);
            }
        }
    }
}

pub(crate) struct DisplayWorkAdmission {
    state: Mutex<State>,
    changed: Notify,
}

impl Default for DisplayWorkAdmission {
    fn default() -> Self {
        Self::new(WorkLimits::default())
    }
}

impl DisplayWorkAdmission {
    fn new(limits: WorkLimits) -> Self {
        Self {
            state: Mutex::new(State {
                policy: NetworkWorkOwner::new(limits),
                scopes: BTreeMap::new(),
                ready: BTreeSet::new(),
                cancelled: BTreeSet::new(),
                closed: false,
            }),
            changed: Notify::new(),
        }
    }

    pub(crate) async fn acquire(
        self: &Arc<Self>,
        hash: [u8; 32],
        deadline: Instant,
    ) -> Result<DisplayWorkLease, DisplayAdmissionError> {
        let lease = {
            let mut state = self.state.lock().expect("display admission poisoned");
            if state.closed {
                return Err(DisplayAdmissionError::Closed);
            }
            if deadline <= Instant::now() {
                return Err(DisplayAdmissionError::Expired);
            }
            let scope = state
                .policy
                .register_scope()
                .ok_or(DisplayAdmissionError::Deferred)?;
            // Each display consumer owns its cancellation boundary. Display
            // fetches never join a normal or another scope's stored fetch.
            let key = WorkKey {
                scope,
                protocol: WorkProtocol::Blob,
                object: hash,
                mode: WorkMode::Display,
                persistence: WorkPersistence::Ephemeral,
                byte_limit: u64::MAX,
                deadline: deadline.into_std(),
                lane: WorkLane::Interactive,
            };
            let waiter = match state.policy.admit(key, &hash, Instant::now().into_std()) {
                WorkAdmission::Admitted(waiter) => waiter,
                _ => {
                    state.policy.revoke_scope(scope);
                    return Err(DisplayAdmissionError::Deferred);
                }
            };
            state.scopes.insert(waiter.work_id(), scope);
            state.advance();
            DisplayWorkLease {
                owner: self.clone(),
                scope,
                waiter,
                deadline,
                released: false,
            }
        };
        self.changed.notify_waiters();
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.state.lock().expect("display admission poisoned");
                state.advance();
                if state.closed {
                    return Err(DisplayAdmissionError::Closed);
                }
                if deadline <= Instant::now() {
                    return Err(DisplayAdmissionError::Expired);
                }
                if state.ready.remove(&lease.waiter.work_id()) {
                    return Ok(lease);
                }
            }
            timeout_at(deadline, notified)
                .await
                .map_err(|_| DisplayAdmissionError::Expired)?;
        }
    }

    pub(crate) fn close(&self) {
        {
            let mut state = self.state.lock().expect("display admission poisoned");
            if state.closed {
                return;
            }
            state.closed = true;
            let scopes = state.scopes.values().copied().collect::<Vec<_>>();
            for scope in scopes {
                state.policy.revoke_scope(scope);
            }
            state.advance();
        }
        self.changed.notify_waiters();
    }

    fn release(&self, scope: WorkScope, waiter: WorkWaiter, publish: bool) -> bool {
        let publish = {
            let mut state = self.state.lock().expect("display admission poisoned");
            let id = waiter.work_id();
            if !publish {
                state.policy.release_waiter(waiter);
                state.policy.revoke_scope(scope);
            }
            let disposition = state.policy.complete(id, Instant::now().into_std());
            state.policy.revoke_scope(scope);
            state.scopes.remove(&id);
            state.ready.remove(&id);
            state.cancelled.remove(&id);
            state.advance();
            !state.closed && disposition == WorkCompletion::Publish
        };
        self.changed.notify_waiters();
        publish
    }
}

pub(crate) struct DisplayWorkLease {
    owner: Arc<DisplayWorkAdmission>,
    scope: WorkScope,
    waiter: WorkWaiter,
    deadline: Instant,
    released: bool,
}

impl DisplayWorkLease {
    pub(crate) async fn cancelled(&self) {
        loop {
            let notified = self.owner.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.owner.state.lock().expect("display admission poisoned");
                state.advance();
                if state.closed || state.cancelled.contains(&self.waiter.work_id()) {
                    return;
                }
            }
            if timeout_at(self.deadline, notified).await.is_err() {
                return;
            }
        }
    }

    pub(crate) fn finish(mut self) -> bool {
        self.released = true;
        self.owner.release(self.scope, self.waiter, true)
    }
}

impl Drop for DisplayWorkLease {
    fn drop(&mut self) {
        if !self.released {
            self.owner.release(self.scope, self.waiter, false);
        }
    }
}

#[cfg(test)]
mod tests;
