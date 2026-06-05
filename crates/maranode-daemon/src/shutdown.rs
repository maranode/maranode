//! stop daemon cleanly when SIGTERM or SIGINT arrives.

use std::future::Future;

pub fn signal() -> impl Future<Output = ()> {
    use tokio::signal;

    async move {
        #[cfg(unix)]
        {
            // SIGTERM handler is optional. Ctrl-C still works.
            match signal::unix::signal(signal::unix::SignalKind::terminate()) {
                Ok(mut sigterm) => {
                    tokio::select! {
                        _ = signal::ctrl_c() => {},
                        _ = sigterm.recv()   => {},
                    }
                }
                Err(e) => {
                    tracing::warn!("could not install SIGTERM handler ({e}); using ctrl-c only");
                    let _ = signal::ctrl_c().await;
                }
            }
        }
        #[cfg(not(unix))]
        {
            if let Err(e) = signal::ctrl_c().await {
                tracing::warn!("could not install ctrl-c handler: {e}");
            }
        }

        tracing::info!("Shutdown signal received");
    }
}
