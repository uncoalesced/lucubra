// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! MCP transports: stdio now, Streamable HTTP behind `feature = "http-transport"` (D14).

use tokio_util::sync::CancellationToken;

use crate::server::LucubraServer;

/// Runs [`LucubraServer`] over stdin/stdout JSON-RPC lines until EOF or cancellation.
///
/// Drops any spawned Python worker subprocess before returning.
pub async fn serve_stdio(
    server: LucubraServer,
    ct: CancellationToken,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let running =
        rmcp::service::serve_server_with_ct(server.clone(), rmcp::transport::io::stdio_pipes(), ct)
            .await?;
    let _reason = running.waiting().await?;
    server.shutdown_worker().await;
    Ok(())
}

/// Optional Streamable HTTP seam compiled only when `http-transport` is enabled (D14).
///
/// Enforces loopback-only binding (`127.0.0.1` / `::1`) per SPEC section 4.4.
#[cfg(feature = "http-transport")]
pub fn build_streamable_http_service(
    options: crate::server::ServerOptions,
    bind: std::net::SocketAddr,
    ct: CancellationToken,
) -> std::result::Result<
    rmcp::transport::streamable_http_server::StreamableHttpService<
        LucubraServer,
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
    >,
    Box<dyn std::error::Error + Send + Sync>,
> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    use std::sync::Arc;

    if !bind.ip().is_loopback() {
        return Err(format!(
            "refusing non-loopback HTTP bind address {bind}; v1 permits loopback only"
        )
        .into());
    }
    let mut config = StreamableHttpServerConfig::default();
    config.cancellation_token = ct;
    let service = StreamableHttpService::new(
        move || LucubraServer::open(&options).map_err(|e| std::io::Error::other(e.to_string())),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    Ok(service)
}

/// Asynchronous Streamable HTTP entrypoint enforcing loopback bind (SPEC section 4.4).
// shortcut: builds the service but binds no listener, so it serves nothing; wire a
// loopback TCP listener before any CLI flag enables `http-transport` (D14).
#[cfg(feature = "http-transport")]
pub async fn serve_streamable_http(
    options: crate::server::ServerOptions,
    bind: std::net::SocketAddr,
    ct: CancellationToken,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _service = build_streamable_http_service(options, bind, ct.clone())?;
    ct.cancelled().await;
    Ok(())
}
