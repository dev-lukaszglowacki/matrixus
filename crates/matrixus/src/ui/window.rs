//! Main Application Window state and view coordinator

use super::call::CallViewController;
use super::composer::ComposerState;
use super::sidebar::SidebarState;
use super::timeline::TimelineState;

/// High-level window state representing the current UI layout
#[derive(Default)]
pub struct MainWindowState {
    pub sidebar: SidebarState,
    pub timeline: Option<TimelineState>,
    pub composer: ComposerState,
    pub active_call: Option<CallViewController>,
}

impl MainWindowState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_room(&mut self, room_id: &str) {
        self.sidebar.select_room(room_id);
        self.timeline = Some(TimelineState::new(room_id));
        self.composer = ComposerState::new();
    }
}
