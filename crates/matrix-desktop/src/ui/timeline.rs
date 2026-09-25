//! Room Timeline presentation model

use matrix_core::TimelineEvent;

/// Timeline view state tracking messages in the active room
#[derive(Default, Debug, Clone)]
pub struct TimelineState {
    pub room_id: String,
    pub events: Vec<TimelineEvent>,
}

impl TimelineState {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            events: Vec::new(),
        }
    }

    pub fn append_event(&mut self, event: TimelineEvent) {
        self.events.push(event);
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}
