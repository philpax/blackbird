//! A minimal fixed-size worker pool for background image decodes.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, mpsc::Sender},
};

pub(super) struct ThreadPool {
    tx: Sender<Box<dyn FnOnce() + Send>>,
}

impl ThreadPool {
    pub(super) fn new(num_threads: usize) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..num_threads {
            let rx = rx.clone();
            std::thread::spawn(move || {
                loop {
                    // Take the job while holding the lock, but release it
                    // before running the job — holding it across `job()`
                    // would serialize the workers, and a panicking job would
                    // poison the mutex and kill the whole pool.
                    let job = match rx.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => break,
                    };
                    let Ok(job) = job else { break };
                    if catch_unwind(AssertUnwindSafe(job)).is_err() {
                        tracing::error!("a cover art worker job panicked");
                    }
                }
            });
        }
        Self { tx }
    }

    pub(super) fn spawn(&self, f: impl FnOnce() + Send + 'static) {
        let _ = self.tx.send(Box::new(f));
    }
}
