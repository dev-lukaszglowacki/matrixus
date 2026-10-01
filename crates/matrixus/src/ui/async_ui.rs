//! Helpers for Tokio → GTK main-thread marshaling.
//!
//! GTK widgets are `!Send`. Only touch them on the GTK main thread via [`on_ui`]
//! or a [`UiSender`] attached with [`attach_ui_receiver`].

#![cfg(feature = "gui")]

use std::sync::mpsc;

use gtk4::glib;

/// Marker wrapper asserting a value will only be used on the GTK main thread.
pub struct UiSend<T>(pub T);

// SAFETY: Callers must only dereference `UiSend` inside `on_ui` / receiver
// callbacks that run on the GTK main thread.
unsafe impl<T> Send for UiSend<T> {}
unsafe impl<T> Sync for UiSend<T> {}

impl<T> UiSend<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

/// Schedule `f` on the default glib main context (GTK thread).
pub fn on_ui<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    glib::MainContext::default().invoke(f);
}

/// Run `work` on Tokio; then run `on_complete` on the GTK main thread.
pub fn spawn_tokio_then_ui<T, W, U>(work: W, on_complete: U)
where
    T: Send + 'static,
    W: std::future::Future<Output = T> + Send + 'static,
    U: FnOnce(T) + Send + 'static,
{
    tokio::spawn(async move {
        let value = work.await;
        on_ui(move || on_complete(value));
    });
}

/// Sender half of a UI message channel (safe to use from Tokio).
#[derive(Debug)]
pub struct UiSender<T>(mpsc::Sender<T>);

impl<T> Clone for UiSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Send> UiSender<T> {
    pub fn send(&self, value: T) -> Result<(), mpsc::SendError<T>> {
        self.0.send(value)
    }
}

/// Create a channel. Call [`attach_ui_receiver`] on the GTK thread with the
/// receiver; use the sender from any thread (including Tokio).
pub fn ui_channel<T: Send + 'static>() -> (UiSender<T>, mpsc::Receiver<T>) {
    let (tx, rx) = mpsc::channel();
    (UiSender(tx), rx)
}

/// Poll `rx` on the GTK main loop and invoke `handler` for each message.
///
/// Must be called from the GTK thread. Keeps polling until the sender is dropped.
pub fn attach_ui_receiver<T, F>(rx: mpsc::Receiver<T>, mut handler: F)
where
    T: Send + 'static,
    F: FnMut(T) + 'static,
{
    glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        loop {
            match rx.try_recv() {
                Ok(value) => handler(value),
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
    });
}
