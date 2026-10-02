//! Room Timeline presentation model

use matrix_core::TimelineEvent;

/// Timeline view state tracking messages in the active room
#[derive(Default, Debug, Clone)]
pub struct TimelineState {
    pub room_id: String,
    pub events: Vec<TimelineEvent>,
    /// Token for the next older `/messages` page. `None` after history start.
    pub prev_batch: Option<String>,
    /// True once a back-pagination returned no further `end` token.
    pub reached_start: bool,
}

impl TimelineState {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            events: Vec::new(),
            prev_batch: None,
            reached_start: false,
        }
    }

    pub fn append_event(&mut self, event: TimelineEvent) {
        self.events.push(event);
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.prev_batch = None;
        self.reached_start = false;
    }
}
