use std::future::Future;
use std::pin::Pin;

use flume::{Receiver, RecvError, Sender};

enum Message {
    #[cfg(not(target_arch = "wasm32"))]
    Job(Box<dyn FnOnce() + Send + 'static>),
    #[cfg(not(target_arch = "wasm32"))]
    AsyncJob(Pin<Box<dyn Future<Output = ()> + Send + 'static>>),

    #[cfg(target_arch = "wasm32")]
    AsyncJob(Pin<Box<dyn Future<Output = ()> + 'static>>),

    Terminate,
}

#[derive(Debug)]
struct Worker {
    id: usize,
    #[cfg(not(target_arch = "wasm32"))]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Receiver<Message>) -> Self {
        let future = async move {
            log::debug!("Started thread {}", id);

            loop {
                match receiver.recv_async().await {
                    Ok(msg) => match msg {
                        #[cfg(not(target_arch = "wasm32"))]
                        Message::Job(job) => {
                            log::trace!("Thread {} received job", id);
                            job();
                        }
                        Message::AsyncJob(job) => {
                            log::trace!("Thread {} received async job", id);
                            job.await;
                        }
                        Message::Terminate => {
                            log::debug!("Terminating thread {}", id);
                            break;
                        }
                    },
                    Err(RecvError::Disconnected) => {
                        log::error!("Sender disconnected, stopping thread {}", id);
                        break;
                    }
                }
            }
        };

        #[cfg(not(target_arch = "wasm32"))]
        let thread = std::thread::Builder::new()
            .name(format!("Worker thread {}", id))
            .spawn(move || {
                pollster::block_on(future);
            })
            .expect("failed to spawn thread");

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(future);

        Self {
            id,
            #[cfg(not(target_arch = "wasm32"))]
            thread: Some(thread),
        }
    }
}

#[derive(Debug)]
pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Sender<Message>,
}

impl ThreadPool {
    pub fn new(size: usize) -> Self {
        assert!(size > 0);

        let (sender, receiver) = flume::unbounded();

        let mut workers = Vec::with_capacity(size);

        for i in 0..size {
            workers.push(Worker::new(i, receiver.clone()));
        }

        Self { workers, sender }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.send(Message::Job(job)).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn execute_async<F>(&self, f: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let job = Box::pin(f);
        self.sender.send(Message::AsyncJob(job)).unwrap();
    }

    #[cfg(target_arch = "wasm32")]
    pub fn execute_async<F>(&self, f: F)
    where
        F: Future<Output = ()> + 'static,
    {
        let job = Box::pin(f);
        self.sender.send(Message::AsyncJob(job)).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        for _ in &self.workers {
            self.sender.send(Message::Terminate).unwrap();
        }

        #[cfg(not(target_arch = "wasm32"))]
        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}
