use flume::{Receiver, RecvError, Sender};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
enum Message {
    Job(Box<dyn FnOnce() + Send + 'static>),
    AsyncJob(Pin<Box<dyn Future<Output = ()> + Send + 'static>>),
    Terminate,
}

#[cfg(target_arch = "wasm32")]
enum Message {
    Job {
        f: Box<dyn FnOnce(js_sys::Array) -> Pin<Box<dyn Future<Output = ()>>> + Send + 'static>,
        params: js_sys::Array,
    },
    Terminate,
}

#[derive(Debug)]
struct Worker {
    id: usize,

    #[cfg(not(target_arch = "wasm32"))]
    thread: Option<std::thread::JoinHandle<()>>,

    #[cfg(target_arch = "wasm32")]
    worker: Option<web_sys::Worker>,
}

impl Worker {
    #[cfg(not(target_arch = "wasm32"))]
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

        let thread = std::thread::Builder::new()
            .name(format!("Worker thread {}", id))
            .spawn(move || {
                pollster::block_on(future);
            })
            .expect("failed to spawn thread");

        Self {
            id,
            thread: Some(thread),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn new(id: usize, on_done_fn: Arc<Box<dyn Fn(js_sys::Array)>>) -> Self {
        use wasm_bindgen::JsValue;

        let url = {
            let script = include_str!("./worker.js");

            let blob =
                web_sys::Blob::new_with_str_sequence(&js_sys::Array::of1(&JsValue::from(script)))
                    .unwrap();

            web_sys::Url::create_object_url_with_blob(&blob).unwrap()
        };

        let worker = {
            let mut worker_options = web_sys::WorkerOptions::new();
            worker_options.name(&format!("Thread {}", id));
            web_sys::Worker::new_with_options(&url, &worker_options).unwrap()
        };

        let init_msg = js_sys::Array::of5(
            &MainJS.main_js(),
            &wasm_bindgen::module(),
            &wasm_bindgen::memory(),
            &JsValue::from(id as u32),
            &JsValue::from(Arc::into_raw(on_done_fn) as u32),
        );

        worker.post_message(&init_msg).unwrap();

        Self {
            id,
            worker: Some(worker),
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = "executeFnOnce")]
pub async fn execute_fn_once(ptr: u32, params: js_sys::Array) {
    let closure = unsafe {
        Box::from_raw(
            ptr as *mut Box<dyn FnOnce(js_sys::Array) -> Pin<Box<dyn Future<Output = ()>>>>,
        )
    };
    (*closure)(params).await
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = "executeFn")]
pub fn execute_fn(ptr: u32, params: js_sys::Array) {
    let closure = unsafe { Arc::from_raw(ptr as *mut Box<dyn Fn(js_sys::Array)>) };
    (*closure)(params)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub struct MainJS;

// from wasm-futures-executor
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
impl MainJS {
    #[wasm_bindgen(js_name = mainJS)]
    pub fn main_js(&self) -> js_sys::JsString {
        #[wasm_bindgen::prelude::wasm_bindgen]
        extern "C" {
            #[wasm_bindgen(js_namespace = ["import", "meta"], js_name = url)]
            static URL: js_sys::JsString;
        }

        URL.clone()
    }
}

#[derive(Debug)]
pub struct ThreadPool {
    #[cfg(not(target_arch = "wasm32"))]
    workers: Vec<Worker>,

    #[cfg(target_arch = "wasm32")]
    workers: Rc<Vec<Worker>>,

    sender: Sender<Message>,
}

impl ThreadPool {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(size: usize) -> Self {
        assert!(size > 0);

        let (sender, receiver) = flume::unbounded();

        let mut workers = Vec::with_capacity(size);

        for i in 0..size {
            workers.push(Worker::new(i, receiver.clone()));
        }

        Self { workers, sender }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn new(size: usize) -> Self {
        assert!(size > 0);

        let (on_done_sender, on_done_receiver) = flume::unbounded();
        let (task_sender, task_receiver) = flume::unbounded();

        let on_done = move |params: js_sys::Array| {
            let id = params.get(0).as_f64().unwrap() as usize;
            on_done_sender.send(id).unwrap();
        };

        let on_done_fn = Arc::new(Box::new(on_done) as Box<dyn Fn(js_sys::Array)>);

        let mut workers = Vec::with_capacity(size);

        for i in 0..size {
            workers.push(Worker::new(i, on_done_fn.clone()));
        }

        let this = Self {
            workers: Rc::new(workers),
            sender: task_sender,
        };
        let workers = this.workers.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let mut available_workers = Vec::<usize>::with_capacity(size);
            let mut remaining_tasks = Vec::new();

            let post_task = |task: Message, worker: &web_sys::Worker| match task {
                Message::Job { f, params } => {
                    let fn_ptr = Box::into_raw(Box::new(f)) as u32;
                    let on_done_ptr = Arc::into_raw(on_done_fn.clone()) as u32;

                    let msg = js_sys::Array::of3(
                        &wasm_bindgen::JsValue::from(fn_ptr),
                        &params,
                        &wasm_bindgen::JsValue::from(on_done_ptr),
                    );
                    worker.post_message(&msg).unwrap();
                }
                Message::Terminate => {
                    worker
                        .post_message(&wasm_bindgen::JsValue::from_str("terminate"))
                        .unwrap();
                }
            };

            loop {
                let on_done = on_done_receiver.recv_async();
                let on_task = task_receiver.recv_async();

                match futures::future::select(on_done, on_task).await {
                    futures::future::Either::Left((Ok(id), _)) => {
                        if let Some(task) = remaining_tasks.pop() {
                            post_task(task, workers[id].worker.as_ref().unwrap());
                        } else {
                            available_workers.push(id);
                        }
                    }
                    futures::future::Either::Right((Ok(task), _)) => {
                        if let Some(id) = available_workers.pop() {
                            post_task(task, workers[id].worker.as_ref().unwrap());
                        } else {
                            remaining_tasks.push(task);
                        }
                    }
                    futures::future::Either::Left((Err(err), _)) => {
                        log::error!("done receiver error: {:?}", err);
                        return;
                    }
                    futures::future::Either::Right((Err(err), _)) => {
                        log::error!("task receiver error: {:?}", err);
                        return;
                    }
                }
            }
        });

        this
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
    pub fn execute<F>(&self, f: F, params: Option<js_sys::Array>)
    where
        F: FnOnce(js_sys::Array) -> Pin<Box<dyn Future<Output = ()>>> + Send + 'static,
    {
        let f = Box::new(f)
            as Box<dyn FnOnce(js_sys::Array) -> Pin<Box<dyn Future<Output = ()>>> + Send + 'static>;

        let params = params.unwrap_or_else(|| js_sys::Array::new());

        if let Err(err) = self.sender.send(Message::Job { f, params }) {
            log::error!("{}", err);
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            for _ in &self.workers {
                self.sender.send(Message::Terminate).unwrap();
            }

            for worker in &mut self.workers {
                if let Some(thread) = worker.thread.take() {
                    thread.join().unwrap();
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            for _ in self.workers.iter() {
                self.sender.send(Message::Terminate).unwrap();
            }
        }
    }
}
