use std::pin::Pin;

pub struct TokioThread {
    tokio: TokioHandle,
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

        // Create a thread for background processing
        let tokio_thread_handle = std::thread::spawn(move || {
            runtime.block_on(async {
                while let Some(task) = tokio_rx.recv().await {
                    tokio::spawn(task);
                }
            });
        });

        Self {
            tokio,
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
}
