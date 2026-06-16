//! serve axum over TLS (rustls) for deployments without a reverse proxy.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;
use tower::Service;

pub fn load_server_config(cert_path: &Path, key_path: &Path) -> Result<Arc<ServerConfig>> {
    let certs = load_certs(cert_path)?;
    let key = load_key(key_path)?;

    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("rustls protocol versions")?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("certificate and private key do not match, or are malformed")?;

    Ok(Arc::new(config))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("open TLS certificate {}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader).collect::<std::io::Result<Vec<_>>>()?;
    if certs.is_empty() {
        anyhow::bail!("no certificates found in {}", path.display());
    }
    Ok(certs)
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("open TLS private key {}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| anyhow::anyhow!("no private key found in {}", path.display()))
}

pub async fn serve_tls(
    listener: TcpListener,
    router: Router,
    tls: Arc<ServerConfig>,
    signal: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let acceptor = TlsAcceptor::from(tls);
    let mut make_service = router.into_make_service();
    let mut signal = std::pin::pin!(signal);

    loop {
        let (stream, _) = tokio::select! {
            accept = listener.accept() => accept?,
            _ = signal.as_mut() => return Ok(()),
        };

        let acceptor = acceptor.clone();
        let svc = make_service
            .call(())
            .await
            .expect("infallible make_service");

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(stream).await {
                Ok(s) => s,
                Err(_) => return,
            };
            let io = TokioIo::new(tls_stream);
            let _ = Builder::new(TokioExecutor::new())
                .serve_connection(io, TowerToHyperService::new(svc))
                .await;
        });
    }
}
