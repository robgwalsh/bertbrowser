//! A small fixed pool of named threads. No async runtime: a job is a closure, results go back through
//! the app's own state and events.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct WorkerPool {
    sender: Mutex<Option<Sender<Job>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl WorkerPool {
    pub fn new(count: usize) -> WorkerPool {
        let (sender, receiver) = channel::<Job>();
        let receiver: Arc<Mutex<Receiver<Job>>> = Arc::new(Mutex::new(receiver));
        let threads = (0..count.max(1))
            .filter_map(|i| {
                let receiver = receiver.clone();
                std::thread::Builder::new()
                    .name(format!("bb-worker-{i}"))
                    .spawn(move || {
                        loop {
                            let job = {
                                let guard = receiver.lock().unwrap_or_else(|p| p.into_inner());
                                guard.recv()
                            };
                            let Ok(job) = job else { return };
                            // A panicking job must not take the worker with it.
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    })
                    .ok()
            })
            .collect();
        WorkerPool {
            sender: Mutex::new(Some(sender)),
            threads: Mutex::new(threads),
        }
    }

    pub fn execute(&self, job: impl FnOnce() + Send + 'static) {
        if let Some(sender) = self
            .sender
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            let _ = sender.send(Box::new(job));
        }
    }

    /// Stops accepting jobs and waits for the queue to drain.
    pub fn shutdown(&self) {
        self.sender.lock().unwrap_or_else(|p| p.into_inner()).take();
        let threads = std::mem::take(&mut *self.threads.lock().unwrap_or_else(|p| p.into_inner()));
        for thread in threads {
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}
