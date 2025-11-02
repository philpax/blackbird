use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct TokioThread {
    tokio: TokioHandle,
    shutdown_requested: Arc<AtomicBool>,
    _tokio_thread_handle: std::thread::JoinHandle<()>,
}
#[derive(Clone)]
pub struct TokioHandle(tokio::sync::mpsc::Sender<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>);
impl TokioHandle {
    fn spawn(&self, task: impl Future<Output = ()> + Send + Sync + 'static) {
        self.0.blocking_send(Box::pin(task)).unwrap();
    }
}
impl TokioThread {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (tokio_tx, mut tokio_rx) = tokio::sync::mpsc::channel(100);
        let tokio = TokioHandle(tokio_tx);

        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let shutdown_flag = shutdown_requested.clone();

        // Create a thread for background processing
        let tokio_thread_handle = std::thread::spawn(move || {
            runtime.block_on(async {
                // Spawn signal handler task
                let shutdown_flag = shutdown_flag.clone();
                tokio::spawn(async move {
                    match tokio::signal::ctrl_c().await {
                        Ok(()) => {
                            tracing::info!("Received Ctrl+C signal, initiating graceful shutdown");
                            shutdown_flag.store(true, Ordering::Relaxed);
                        }
                        Err(err) => {
                            tracing::error!("Failed to listen for Ctrl+C signal: {}", err);
                        }
                    }
                });

                while let Some(task) = tokio_rx.recv().await {
                    tokio::spawn(task);
                }
            });
        });

        Self {
            tokio,
            shutdown_requested,
            _tokio_thread_handle: tokio_thread_handle,
        }
    }

    #[allow(unused)]
    pub fn handle(&self) -> TokioHandle {
        self.tokio.clone()
    }

    pub fn spawn(&self, task: impl Future<Output = ()> + Send + Sync + 'static) {
        self.tokio.spawn(task);
    }

    pub fn should_shutdown(&self) -> bool {
        self.shutdown_requested.load(Ordering::Relaxed)
    }
}
