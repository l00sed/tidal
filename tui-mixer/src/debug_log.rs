//! Debug logging infrastructure.
//!
//! Provides a lock-free bounded queue that captures `tracing` output and
//! feeds it into the TUI debug pane when `DEBUG=1` is set. Also handles
//! stderr redirection to prevent TUI corruption.

use crossbeam_queue::ArrayQueue;
use std::sync::OnceLock;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;
use tracing_subscriber::prelude::*;

/// Shared bounded queue for debug log messages (capacity: 256).
static LOG_QUEUE: OnceLock<ArrayQueue<String>> = OnceLock::new();

/// Drains all queued messages into the provided vec.
pub fn drain_log_queue(buf: &mut Vec<String>) {
    if let Some(queue) = LOG_QUEUE.get() {
        while let Some(msg) = queue.pop() {
            buf.push(msg);
            if buf.len() > 500 {
                buf.remove(0);
            }
        }
    }
}

struct DebugLayer;

impl<S: Subscriber> Layer<S> for DebugLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = QueueVisitor;
        event.record(&mut visitor);
    }
}

struct QueueVisitor;

impl tracing::field::Visit for QueueVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let queue = LOG_QUEUE.get_or_init(|| ArrayQueue::new(256));
        let target = field.name();
        let msg = format!("[{}] {:?}", target, value);
        let _ = queue.push(msg);
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        let queue = LOG_QUEUE.get_or_init(|| ArrayQueue::new(256));
        let msg = format!("[{}] {}", field.name(), value);
        let _ = queue.push(msg);
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        let queue = LOG_QUEUE.get_or_init(|| ArrayQueue::new(256));
        let msg = format!("[{}] {}", field.name(), value);
        let _ = queue.push(msg);
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        let queue = LOG_QUEUE.get_or_init(|| ArrayQueue::new(256));
        let msg = format!("[{}] {}", field.name(), value);
        let _ = queue.push(msg);
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        let queue = LOG_QUEUE.get_or_init(|| ArrayQueue::new(256));
        let msg = format!("[{}] {}", field.name(), value);
        let _ = queue.push(msg);
    }
}

/// Initialize the tracing subscriber to route logs to the debug pane.
/// Also redirects stderr to /dev/null to prevent TUI corruption.
pub fn init_logging() {
    use tracing_subscriber::EnvFilter;

    LOG_QUEUE.get_or_init(|| ArrayQueue::new(256));

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));

    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(DebugLayer);

    // The global default can only be set once; ignore errors from repeated calls.
    let _ = tracing::subscriber::set_global_default(subscriber);

    // Always redirect stderr to prevent library warnings from corrupting TUI
    redirect_stderr_to_devnull();
}

/// Redirect stderr (fd 2) to /dev/null.
fn redirect_stderr_to_devnull() {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        if let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
            let fd = devnull.as_raw_fd();
            unsafe {
                libc::dup2(fd, 2);
            }
        }
    }
}
