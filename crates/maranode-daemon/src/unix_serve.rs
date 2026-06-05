//! run axum on Unix domain socket. Axum 0.7 serve() only supports TCP.

use std::future::Future;

use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use tokio::net::UnixListener;
use tower::{util::ServiceExt, Service};

pub async fn serve_unix(
    listener: UnixListener,
    router: Router,
    signal: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let mut make_service = router.into_make_service();
    let mut signal = std::pin::pin!(signal);

    loop {
        let (stream, _) = tokio::select! {
            accept = listener.accept() => accept?,
            _ = signal.as_mut() => return Ok(()),
        };

        let io = TokioIo::new(stream);
        let svc = make_service
            .call(())
            .await
            .expect("infallible make_service");

        tokio::spawn(async move {
            let _ = Builder::new(TokioExecutor::new())
                .serve_connection(io, TowerToHyperService::new(svc))
                .await;
        });
    }
}
