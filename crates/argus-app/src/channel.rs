use std::sync::Arc;
/// An application subscriber supplied by a renderer adapter.
pub struct Channel<T>(Arc<dyn Fn(T) -> anyhow::Result<()> + Send + Sync>);
impl<T> Clone for Channel<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Channel<T> {
    pub fn new(send: impl Fn(T) -> anyhow::Result<()> + Send + Sync + 'static) -> Self {
        Self(Arc::new(send))
    }
    pub fn send(&self, value: T) -> anyhow::Result<()> {
        (self.0)(value)
    }
}
