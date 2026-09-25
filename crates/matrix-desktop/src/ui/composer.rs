//! Message Composer presentation state

/// Composer state for text input and message dispatch
#[derive(Default, Debug, Clone)]
pub struct ComposerState {
    pub input_text: String,
    pub reply_to_event_id: Option<String>,
}

impl ComposerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.input_text = text.into();
    }

    pub fn take_text(&mut self) -> String {
        std::mem::take(&mut self.input_text)
    }

    pub fn set_reply_to(&mut self, event_id: Option<String>) {
        self.reply_to_event_id = event_id;
    }
}
