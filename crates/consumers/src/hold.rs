use frostline_shared::ShutdownWatch;
use tokio::sync::oneshot;

/// Keeps a finished reader's connections open until it is released, so its socket counters can be read first.
pub struct Hold {
    pub reached: oneshot::Sender<()>,
    pub release: oneshot::Receiver<()>,
}

impl Hold {
    pub async fn wait(self, mut shutdown: ShutdownWatch) {
        let _ = self.reached.send(());
        tokio::select! { _ = self.release => {}, () = shutdown.cancelled() => {} }
    }
}
