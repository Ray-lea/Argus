//! Background work never blocks the GPUI foreground executor.
use std::{future::Future, sync::LazyLock};
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("argus-worker")
        .build()
        .expect("background runtime")
});
pub fn spawn<F: Future + Send + 'static>(task: F) -> tokio::task::JoinHandle<F::Output>
where
    F::Output: Send + 'static,
{
    RUNTIME.spawn(task)
}
pub fn block_on<F: Future>(task: F) -> F::Output {
    RUNTIME.block_on(task)
}
