//! Room List Sidebar presentation model

use matrix_core::RoomSummary;

/// Sidebar view state tracking room list and active selection
#[derive(Default, Debug, Clone)]
pub struct SidebarState {
    pub rooms: Vec<RoomSummary>,
    pub selected_room_id: Option<String>,
}

impl SidebarState {
    pub fn update_rooms(&mut self, rooms: Vec<RoomSummary>) {
        self.rooms = rooms;
    }

    pub fn select_room(&mut self, room_id: impl Into<String>) {
        self.selected_room_id = Some(room_id.into());
    }

    pub fn selected_room(&self) -> Option<&RoomSummary> {
        let id = self.selected_room_id.as_deref()?;
        self.rooms.iter().find(|r| r.room_id == id)
    }
}
