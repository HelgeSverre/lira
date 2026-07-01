//! I/O thread pool for offloading blocking syscalls off the single VM thread.
//!
//! Fibers are cooperatively scheduled on one OS thread, so a blocking syscall
//! (an HTTP request) would otherwise stall every other fiber for its whole
//! duration — no I/O overlap. Instead, a blocking syscall is packaged as an
//! [`IoJob`], handed to a pool of worker threads, and the calling fiber is
//! parked (`FiberState::BlockedIo`). The workers run requests in parallel and
//! return owned, `Send` results ([`IoCompletion`]); the VM thread harvests
//! them and wakes the corresponding fibers.
//!
//! Only plain data (owned `String`/`i64`) crosses the thread boundary — never a
//! `Gc` handle or any VM state — so the VM's single-threaded, non-`Send` value
//! model is preserved. The result `Value` is reconstructed on the VM thread.

use crate::runtime::Runtime;
use crate::value::FiberId;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

/// Number of worker threads. I/O-bound, so this is the ceiling on concurrent
/// in-flight requests, not a function of CPU count.
const POOL_SIZE: usize = 16;

/// A blocking syscall to run off the VM thread. Carries the parked fiber's id
/// so the result can be routed back to it.
pub enum IoJob {
    HttpGet {
        fiber: FiberId,
        url: String,
    },
    HttpPost {
        fiber: FiberId,
        url: String,
        body: String,
        content_type: String,
    },
    HttpRequest {
        fiber: FiberId,
        method: String,
        url: String,
        headers: String,
        body: String,
    },
}

/// The result of a completed [`IoJob`], surfaced to Lira as `[status, body]`.
/// `status` is `-1` on a transport error, with `body` holding the message —
/// matching the existing inline syscall contract.
pub struct IoCompletion {
    pub fiber: FiberId,
    pub status: i64,
    pub body: String,
}

fn run_job(job: IoJob) -> IoCompletion {
    match job {
        IoJob::HttpGet { fiber, url } => {
            let (status, body) = match Runtime::http_get_blocking(&url) {
                Ok((s, _headers, b)) => (s, b),
                Err(e) => (-1, e),
            };
            IoCompletion { fiber, status, body }
        }
        IoJob::HttpPost {
            fiber,
            url,
            body,
            content_type,
        } => {
            let (status, body) = match Runtime::http_post_blocking(&url, &body, &content_type) {
                Ok((s, _headers, b)) => (s, b),
                Err(e) => (-1, e),
            };
            IoCompletion { fiber, status, body }
        }
        IoJob::HttpRequest {
            fiber,
            method,
            url,
            headers,
            body,
        } => {
            let (status, body) = match Runtime::http_request_blocking(&method, &url, &headers, &body)
            {
                Ok((s, b)) => (s, b),
                Err(e) => (-1, e),
            };
            IoCompletion { fiber, status, body }
        }
    }
}

/// A fixed pool of worker threads plus the channels to feed them jobs and
/// collect completions. Created lazily on first use so programs that never do
/// blocking I/O spawn no threads.
pub struct IoPool {
    job_tx: Sender<IoJob>,
    completion_rx: Receiver<IoCompletion>,
    /// Jobs submitted but not yet harvested. The scheduler uses this to decide
    /// whether an otherwise-idle VM thread should wait for I/O vs. declare a
    /// deadlock / finish.
    pending: usize,
}

impl IoPool {
    pub fn new() -> Self {
        let (job_tx, job_rx) = channel::<IoJob>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let (comp_tx, completion_rx) = channel::<IoCompletion>();

        for _ in 0..POOL_SIZE {
            let job_rx = Arc::clone(&job_rx);
            let comp_tx = comp_tx.clone();
            thread::spawn(move || loop {
                // Hold the lock only to dequeue; the blocking request runs
                // outside it so all workers can be in-flight simultaneously.
                let job = {
                    let guard = match job_rx.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    match guard.recv() {
                        Ok(job) => job,
                        Err(_) => break, // pool dropped: sender gone
                    }
                };
                if comp_tx.send(run_job(job)).is_err() {
                    break; // VM gone
                }
            });
        }

        IoPool {
            job_tx,
            completion_rx,
            pending: 0,
        }
    }

    /// Submit a blocking job to the pool.
    pub fn submit(&mut self, job: IoJob) {
        if self.job_tx.send(job).is_ok() {
            self.pending += 1;
        }
    }

    pub fn pending(&self) -> usize {
        self.pending
    }

    /// Harvest every already-completed job without blocking.
    pub fn drain_completed(&mut self) -> Vec<IoCompletion> {
        let mut out = Vec::new();
        loop {
            match self.completion_rx.try_recv() {
                Ok(c) => {
                    self.pending -= 1;
                    out.push(c);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    /// Block until one job completes. Called only when the VM thread has nothing
    /// else to run but I/O is still in flight.
    pub fn wait_one(&mut self) -> Option<IoCompletion> {
        match self.completion_rx.recv() {
            Ok(c) => {
                self.pending -= 1;
                Some(c)
            }
            Err(_) => None,
        }
    }
}

impl Default for IoPool {
    fn default() -> Self {
        Self::new()
    }
}
