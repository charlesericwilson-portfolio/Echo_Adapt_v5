use std::collections::VecDeque;
use std::time::Instant;

/// A completed command result from a supervised tmux session.
#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub session_name: String,
    pub marker_id: i64,
    pub output: String,
}

/// Runtime state for one named tmux session.
#[derive(Debug)]
pub struct SessionState {
    /// Last time this session was used.
    pub last_used: Instant,

    /// Marker for a command currently running in this session.
    pub running_marker: Option<i64>,

    /// Most recent completed marker that has been consumed.
    pub last_seen_marker: Option<i64>,

    /// Completed outputs waiting to be delivered to the agent.
    pub pending: VecDeque<SessionEvent>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            last_used: Instant::now(),
            running_marker: None,
            last_seen_marker: None,
            pending: VecDeque::new(),
        }
    }

    pub fn touch(&mut self) {
        self.last_used = Instant::now();
    }

    pub fn mark_running(&mut self, marker_id: i64) {
        self.running_marker = Some(marker_id);
        self.touch();
    }

    pub fn push_completed(
        &mut self,
        session_name: &str,
        marker_id: i64,
        output: String,
    ) {
        self.pending.push_back(SessionEvent {
            session_name: session_name.to_string(),
            marker_id,
            output,
        });

        self.running_marker = None;
        self.touch();
    }

    pub fn take_pending(&mut self) -> Option<SessionEvent> {
        let event = self.pending.pop_front();

        if let Some(ref completed) = event {
            self.last_seen_marker = Some(completed.marker_id);
        }

        event
    }

    pub fn is_running(&self) -> bool {
        self.running_marker.is_some()
    }
}
