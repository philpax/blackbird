use std::sync::{Arc, Mutex};

use tracing::Subscriber;
use tracing_subscriber::Layer;

/// Maximum number of log entries to keep in the buffer.
const MAX_LOG_ENTRIES: usize = 1000;

/// A log entry with level, target, and message.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: tracing::Level,
    pub target: String,
    pub message: String,
}

/// Shared log buffer that can be written to by the tracing layer and read by the UI.
#[derive(Clone)]
pub struct LogBuffer {
    entries: Arc<Mutex<Vec<LogEntry>>>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Push a new log entry, evicting old entries if the buffer is full.
    fn push(&self, entry: LogEntry) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= MAX_LOG_ENTRIES {
            entries.remove(0);
        }
        entries.push(entry);
    }

    /// Get a snapshot of all log entries.
    pub fn get_entries(&self) -> Vec<LogEntry> {
        self.entries.lock().unwrap().clone()
    }

    /// Get the number of log entries.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

/// A tracing layer that writes log messages to a `LogBuffer`.
pub struct LogBufferLayer {
    buffer: LogBuffer,
}

impl LogBufferLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for LogBufferLayer
where
    S: Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let level = *metadata.level();
        let target = metadata.target().to_string();

        // Extract the message from the event.
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let message = visitor.message.unwrap_or_default();

        self.buffer.push(LogEntry {
            level,
            target,
            message,
        });
    }
}

/// A visitor that extracts the message field from a tracing event.
#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }
}
