use std::collections::HashSet;
use std::convert::Infallible;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use bytes::Bytes;
use http_body_util::{BodyExt, LengthLimitError, Limited, combinators::UnsyncBoxBody};
use hyper::body::Incoming;
use hyper::header::{CONNECTION, CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue, UPGRADE, VIA};
#[cfg(test)]
use hyper::header::{HOST, TRANSFER_ENCODING};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode, Uri, Version};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use sha1::{Digest as _, Sha1};
#[cfg(test)]
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{sleep, timeout};
use tokio_rustls::TlsAcceptor;

use crate::certs;
use crate::host::ip_is_loopback;
use crate::ports::local_lan_ip_for_ipv4_listener;
use crate::state::{FileSignature, StateStore, file_signature};
use crate::types::{ProxySettings, Route};
mod backend;
mod headers;
mod routing;
mod tls;
mod tunnel;

use self::backend::*;
use self::headers::*;
use self::routing::*;
use self::tls::*;
use self::tunnel::tunnel_websocket;

type ProxyBody = UnsyncBoxBody<Bytes, hyper::Error>;
type BoxBodyError = Box<dyn Error + Send + Sync>;
type ClientBody = UnsyncBoxBody<Bytes, BoxBodyError>;
type ProxyClient = Client<HttpConnector, ClientBody>;
type TlsSignature = Option<(FileSignature, FileSignature)>;

#[derive(Clone)]
struct ConnectionPermit {
    _permit: Arc<OwnedSemaphorePermit>,
}

impl ConnectionPermit {
    fn new(permit: OwnedSemaphorePermit) -> Self {
        Self {
            _permit: Arc::new(permit),
        }
    }
}

#[derive(Clone)]
struct ProxyLimits {
    connections: Arc<Semaphore>,
    requests: Arc<Semaphore>,
    websockets: Arc<Semaphore>,
}

impl ProxyLimits {
    fn new() -> Self {
        Self {
            connections: Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS)),
            requests: Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)),
            websockets: Arc::new(Semaphore::new(MAX_CONCURRENT_WEBSOCKET_TUNNELS)),
        }
    }
}

#[derive(Clone)]
struct RequestContext {
    remote_addr: SocketAddr,
    proxy_port: u16,
    tls: bool,
    local_ip: IpAddr,
    lan_ip: Option<IpAddr>,
    health_token: Arc<str>,
}

#[derive(Clone)]
struct ListenerContext {
    proxy_port: u16,
    lan_ip: Option<IpAddr>,
    health_token: Arc<str>,
}

const ROUTE_CACHE_MAX_AGE: Duration = Duration::from_millis(500);
const MAX_CONCURRENT_CONNECTIONS: usize = 256;
const MAX_CONCURRENT_REQUESTS: usize = 1024;
const MAX_CONCURRENT_WEBSOCKET_TUNNELS: usize = 64;
const MAX_BACKEND_HEADER_BYTES: usize = 64 * 1024;
const MAX_BACKEND_HEADER_COUNT: usize = 1024;
const MAX_BACKEND_REQUEST_BODY_BYTES: usize = 100 * 1024 * 1024;
const MAX_BACKEND_DRAIN_BODY_BYTES: usize = 10 * 1024 * 1024;
const MAX_CHUNK_HEADER_BYTES: usize = 8 * 1024;
const HTTP2_MAX_CONCURRENT_STREAMS: u32 = 64;
const HTTP2_MAX_FRAME_SIZE: u32 = 16 * 1024;
const HTTP2_MAX_HEADER_LIST_SIZE: u32 = 64 * 1024;
const HTTP2_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);
const HTTP2_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(5);
const BACKEND_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const BACKEND_HTTP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const BACKEND_BODY_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const HTTP1_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const WEBSOCKET_BACKEND_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const WEBSOCKET_UPGRADE_TIMEOUT: Duration = Duration::from_secs(5);
// Local HMR and dev-tool WebSockets can sit quiet for long stretches between
// edits, so the tunnel idle window is intentionally longer than HTTP waits.
const WEBSOCKET_TUNNEL_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);
const TLS_RELOAD_FILE_RETRY_DELAY: Duration = Duration::from_millis(25);
const TLS_RELOAD_FILE_ATTEMPTS: usize = 3;
const MAX_CONSECUTIVE_ACCEPT_ERRORS: u32 = 10;
const MAX_PROXY_HOPS: u8 = 8;
const TUNNEL_BUFFER_SIZE: usize = 16 * 1024;
const VIA_VALUE: &str = "1.1 jig";
const HEALTH_TOKEN_BYTES: usize = 64;

pub(crate) fn run_foreground(
    settings: ProxySettings,
    current_exe: std::path::PathBuf,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_async(settings, current_exe))
}

async fn run_async(settings: ProxySettings, current_exe: std::path::PathBuf) -> Result<()> {
    let store = StateStore::resolve(settings.state_dir.clone())?;
    let owns_runtime = Arc::new(AtomicBool::new(false));

    #[cfg(unix)]
    let result = {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("Failed to listen for SIGTERM")?;
        tokio::select! {
            result = run_bound(settings, store.clone(), current_exe, owns_runtime.clone()) => result,
            signal = tokio::signal::ctrl_c() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                signal.context("Failed to listen for Ctrl-C")?;
                anyhow::bail!("Jig proxy interrupted");
            }
            _ = terminate.recv() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                anyhow::bail!("Jig proxy terminated");
            }
        }
    };

    #[cfg(windows)]
    let result = {
        let mut ctrl_break =
            tokio::signal::windows::ctrl_break().context("Failed to listen for Ctrl-Break")?;
        let mut ctrl_close =
            tokio::signal::windows::ctrl_close().context("Failed to listen for console close")?;
        let mut ctrl_shutdown =
            tokio::signal::windows::ctrl_shutdown().context("Failed to listen for shutdown")?;
        tokio::select! {
            result = run_bound(settings, store.clone(), current_exe, owns_runtime.clone()) => result,
            signal = tokio::signal::ctrl_c() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                signal.context("Failed to listen for Ctrl-C")?;
                anyhow::bail!("Jig proxy interrupted");
            }
            _ = ctrl_break.recv() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                anyhow::bail!("Jig proxy interrupted");
            }
            _ = ctrl_close.recv() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                anyhow::bail!("Jig proxy console closed");
            }
            _ = ctrl_shutdown.recv() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                anyhow::bail!("Jig proxy shutdown requested");
            }
        }
    };

    #[cfg(not(any(unix, windows)))]
    let result = tokio::select! {
        result = run_bound(settings, store.clone(), current_exe, owns_runtime.clone()) => result,
        signal = tokio::signal::ctrl_c() => {
            clear_runtime_files_if_owned(&store, &owns_runtime);
            signal.context("Failed to listen for Ctrl-C")?;
            anyhow::bail!("Jig proxy interrupted");
        }
    };
    result
}

async fn run_bound(
    settings: ProxySettings,
    store: StateStore,
    current_exe: std::path::PathBuf,
    owns_runtime: Arc<AtomicBool>,
) -> Result<()> {
    let bind_host = if settings.lan { "0.0.0.0" } else { "127.0.0.1" };
    let http_listener = bind_listener(bind_host, settings.http_port).await?;
    let http_port = http_listener.local_addr()?.port();

    let https_listener = if settings.https {
        certs::ensure(&settings)?;
        let port = settings.https_port.unwrap_or(1443);
        let listener = bind_listener(bind_host, port).await?;
        Some(listener)
    } else {
        None
    };
    let https_port = https_listener
        .as_ref()
        .map(|listener| listener.local_addr().map(|addr| addr.port()))
        .transpose()?;

    let health_token = match write_runtime_files(&store, &current_exe, http_port, https_port) {
        Ok(token) => token,
        Err(error) => {
            store.clear_runtime_files();
            return Err(error);
        }
    };
    let health_token: Arc<str> = Arc::from(health_token);
    owns_runtime.store(true, Ordering::SeqCst);
    let lan_ip = if settings.lan {
        local_lan_ip_for_ipv4_listener()
    } else {
        None
    };
    eprintln!("jig proxy listening on http://{bind_host}:{http_port}");
    if settings.lan {
        eprintln!(
            "jig proxy LAN mode exposes process-owned loopback routes to devices that can reach this host; aliases remain loopback-client only. Use it only on trusted networks."
        );
    }
    if settings.lan {
        if let Some(ip) = lan_ip {
            eprintln!("jig proxy LAN address: http://{ip}:{http_port}");
        } else {
            eprintln!(
                "jig proxy LAN mode enabled, but no non-loopback IPv4 LAN address was detected for the IPv4 listener"
            );
        }
    }
    if let Some(actual) = https_port {
        eprintln!("jig proxy listening on https://{bind_host}:{actual}");
        if settings.lan {
            if let Some(ip) = lan_ip {
                eprintln!("jig proxy LAN TLS address: https://{ip}:{actual}");
            } else {
                eprintln!(
                    "jig proxy LAN TLS mode enabled, but no non-loopback IPv4 LAN address was detected for the IPv4 listener"
                );
            }
        }
    }

    let runtime_store = store.clone();
    let route_cache = RouteCache::new(store.clone());
    let limits = ProxyLimits::new();
    let listener_context = ListenerContext {
        proxy_port: http_port,
        lan_ip,
        health_token,
    };
    let http = serve_http(
        http_listener,
        route_cache.clone(),
        limits.clone(),
        listener_context.clone(),
    );
    let result = if let Some(listener) = https_listener {
        let https_port = https_port.unwrap_or(settings.https_port.unwrap_or(1443));
        let tls_cache = TlsCache::new(store, settings.http2);
        let https_context = ListenerContext {
            proxy_port: https_port,
            ..listener_context
        };
        tokio::select! {
            result = http => result,
            result = serve_https(listener, route_cache, tls_cache, limits, settings.http2, https_context) => result,
        }
    } else {
        http.await
    };
    clear_runtime_files_if_owned(&runtime_store, &owns_runtime);
    result
}

fn write_runtime_files(
    store: &StateStore,
    current_exe: &std::path::Path,
    http_port: u16,
    https_port: Option<u16>,
) -> Result<String> {
    store.replace_runtime_files(current_exe, http_port, https_port)
}

fn clear_runtime_files_if_owned(store: &StateStore, owns_runtime: &AtomicBool) {
    if owns_runtime.swap(false, Ordering::SeqCst) {
        store.clear_runtime_files();
    }
}

async fn bind_listener(bind_host: &str, port: u16) -> Result<TcpListener> {
    TcpListener::bind((bind_host, port))
        .await
        .map_err(|error| {
            if port < 1024 && port != 0 && error.kind() == std::io::ErrorKind::PermissionDenied {
                anyhow::anyhow!(
                    "Failed to bind privileged port {port}: {error}. Choose a port >= 1024, run the proxy as a root-owned service, or grant the Jig binary bind privileges first. On Linux, use `sudo setcap 'cap_net_bind_service=+ep' <path-to-jig>` for the installed binary; on macOS, use a root LaunchDaemon or a local port-forward from 80/443 to an unprivileged Jig proxy port."
                )
            } else {
                error.into()
            }
        })
}

#[derive(Clone)]
struct RouteCache {
    store: StateStore,
    client: ProxyClient,
    inner: Arc<RwLock<CachedRoutes>>,
}

struct CachedRoutes {
    loaded_at: Option<Instant>,
    signature: FileSignature,
    routes: Arc<Vec<Route>>,
}

impl RouteCache {
    fn new(store: StateStore) -> Self {
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(BACKEND_CONNECT_TIMEOUT));
        let client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(0)
            .pool_idle_timeout(Duration::from_secs(30))
            .build(connector);
        Self {
            store,
            client,
            inner: Arc::new(RwLock::new(CachedRoutes {
                loaded_at: None,
                signature: None,
                routes: Arc::new(Vec::new()),
            })),
        }
    }

    fn client(&self) -> ProxyClient {
        self.client.clone()
    }

    async fn proxy_listener_ports(&self) -> Result<Vec<u16>> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let mut ports = Vec::new();
            if let Some(port) = store.read_http_port()? {
                ports.push(port);
            }
            if let Some(port) = store.read_https_port()? {
                ports.push(port);
            }
            ports.sort_unstable();
            ports.dedup();
            Ok::<_, anyhow::Error>(ports)
        })
        .await
        .context("Proxy listener port loading task failed")?
    }

    async fn routes(&self) -> Result<Arc<Vec<Route>>> {
        let signature = current_routes_signature(&self.store).await?;
        {
            let cache = self
                .inner
                .read()
                .map_err(|_| anyhow::anyhow!("jig proxy route cache read lock was poisoned"))?;
            if signature.is_some()
                && cache.signature == signature
                && cache
                    .loaded_at
                    .is_some_and(|loaded_at| loaded_at.elapsed() <= ROUTE_CACHE_MAX_AGE)
            {
                return Ok(Arc::clone(&cache.routes));
            }
        }

        let store = self.store.clone();
        let (routes, signature) =
            tokio::task::spawn_blocking(move || best_effort_routes_snapshot(&store))
                .await
                .context("Route loading task failed")??;

        let mut cache = self
            .inner
            .write()
            .map_err(|_| anyhow::anyhow!("jig proxy route cache write lock was poisoned"))?;
        if signature.is_some()
            && cache.signature == signature
            && cache
                .loaded_at
                .is_some_and(|loaded_at| loaded_at.elapsed() <= ROUTE_CACHE_MAX_AGE)
        {
            return Ok(Arc::clone(&cache.routes));
        }
        if signature.is_some() {
            cache.signature = signature;
            cache.loaded_at = Some(Instant::now());
            cache.routes = Arc::clone(&routes);
        } else {
            cache.signature = None;
            cache.loaded_at = None;
            cache.routes = Arc::new(Vec::new());
        }
        Ok(routes)
    }
}

fn best_effort_routes_snapshot(store: &StateStore) -> Result<(Arc<Vec<Route>>, FileSignature)> {
    let mut signature = store.routes_signature();
    let mut routes = Arc::new(store.read_routes(true)?);
    let after_read = store.routes_signature();
    if after_read != signature {
        signature = after_read;
        routes = Arc::new(store.read_routes(true)?);
        if store.routes_signature() != signature {
            // Route writes raced both reads. Return the latest routes but no
            // signature so callers can serve this request without caching a
            // snapshot whose freshness was not proven.
            return Ok((routes, None));
        }
    }
    Ok((routes, signature))
}

#[derive(Clone)]
struct TlsCache {
    store: StateStore,
    http2: bool,
    inner: Arc<RwLock<CachedTls>>,
}

struct CachedTls {
    signature: TlsSignature,
    loaded_at: Option<Instant>,
    acceptor: Option<TlsAcceptor>,
}

impl TlsCache {
    fn new(store: StateStore, http2: bool) -> Self {
        Self {
            store,
            http2,
            inner: Arc::new(RwLock::new(CachedTls {
                signature: None,
                loaded_at: None,
                acceptor: None,
            })),
        }
    }

    async fn acceptor(&self) -> Result<TlsAcceptor> {
        // Match the route-cache TTL so same-size cert rewrites on coarse-mtime
        // filesystems cannot leave the old certificate pinned indefinitely.
        let signature = current_tls_signature(&self.store).await?;
        {
            let cache = self
                .inner
                .read()
                .map_err(|_| anyhow::anyhow!("jig proxy TLS cache read lock was poisoned"))?;
            if cache.signature == signature
                && cache
                    .loaded_at
                    .is_some_and(|loaded_at| loaded_at.elapsed() <= ROUTE_CACHE_MAX_AGE)
            {
                if let Some(acceptor) = &cache.acceptor {
                    return Ok(acceptor.clone());
                }
            }
        }

        {
            let cache = self
                .inner
                .write()
                .map_err(|_| anyhow::anyhow!("jig proxy TLS cache write lock was poisoned"))?;
            if cache.signature == signature
                && cache
                    .loaded_at
                    .is_some_and(|loaded_at| loaded_at.elapsed() <= ROUTE_CACHE_MAX_AGE)
            {
                if let Some(acceptor) = &cache.acceptor {
                    return Ok(acceptor.clone());
                }
            }
        }

        let store = self.store.clone();
        let http2 = self.http2;
        let acceptor = tokio::task::spawn_blocking(move || tls_acceptor(&store, http2))
            .await
            .context("TLS certificate loading task failed")??;
        let mut cache = self
            .inner
            .write()
            .map_err(|_| anyhow::anyhow!("jig proxy TLS cache write lock was poisoned"))?;
        cache.signature = signature;
        cache.loaded_at = Some(Instant::now());
        cache.acceptor = Some(acceptor.clone());
        Ok(acceptor)
    }
}

fn tls_signature(store: &StateStore) -> TlsSignature {
    Some((
        file_signature(&store.leaf_path()),
        file_signature(&store.leaf_key_path()),
    ))
}

async fn current_routes_signature(store: &StateStore) -> Result<FileSignature> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || store.routes_signature())
        .await
        .context("Route signature task failed")
}

async fn current_tls_signature(store: &StateStore) -> Result<TlsSignature> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || tls_signature(&store))
        .await
        .context("TLS certificate signature task failed")
}

async fn serve_http(
    listener: TcpListener,
    route_cache: RouteCache,
    limits: ProxyLimits,
    listener_context: ListenerContext,
) -> Result<()> {
    let mut consecutive_accept_errors = 0u32;
    loop {
        let permit = match limits.connections.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => bail!("jig proxy http connection limiter closed"),
        };
        let (stream, remote_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                drop(permit);
                consecutive_accept_errors += 1;
                eprintln!("jig proxy http accept error: {error}");
                if consecutive_accept_errors >= MAX_CONSECUTIVE_ACCEPT_ERRORS {
                    bail!(
                        "jig proxy http listener failed {consecutive_accept_errors} consecutive accepts; exiting"
                    );
                }
                sleep(ACCEPT_ERROR_BACKOFF).await;
                continue;
            }
        };
        consecutive_accept_errors = 0;
        let local_ip = stream
            .local_addr()
            .map(|addr| addr.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let route_cache = route_cache.clone();
        let limits = limits.clone();
        let listener_context = listener_context.clone();
        tokio::spawn(async move {
            let connection_permit = ConnectionPermit::new(permit);
            let request_context = RequestContext {
                remote_addr,
                proxy_port: listener_context.proxy_port,
                tls: false,
                local_ip,
                lan_ip: listener_context.lan_ip,
                health_token: listener_context.health_token,
            };
            let service = service_fn(move |req| {
                handle_request(
                    req,
                    request_context.clone(),
                    route_cache.clone(),
                    limits.clone(),
                )
            });
            let _connection_permit = connection_permit;
            let io = TokioIo::new(stream);
            let mut http1 = hyper::server::conn::http1::Builder::new();
            http1.timer(TokioTimer::new());
            http1.header_read_timeout(HTTP1_HEADER_READ_TIMEOUT);
            // Keep HTTP/1 single-request so idle clients cannot pin connection permits.
            http1.keep_alive(false);
            let result = http1.serve_connection(io, service).with_upgrades().await;
            if let Err(error) = result {
                if !is_disconnect_error(&error) {
                    eprintln!("jig proxy http connection error: {error}");
                }
            }
        });
    }
}

async fn serve_https(
    listener: TcpListener,
    route_cache: RouteCache,
    tls_cache: TlsCache,
    limits: ProxyLimits,
    http2: bool,
    listener_context: ListenerContext,
) -> Result<()> {
    let mut consecutive_accept_errors = 0u32;
    loop {
        let permit = match limits.connections.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => bail!("jig proxy https connection limiter closed"),
        };
        let (stream, remote_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                drop(permit);
                consecutive_accept_errors += 1;
                eprintln!("jig proxy https accept error: {error}");
                if consecutive_accept_errors >= MAX_CONSECUTIVE_ACCEPT_ERRORS {
                    bail!(
                        "jig proxy https listener failed {consecutive_accept_errors} consecutive accepts; exiting"
                    );
                }
                sleep(ACCEPT_ERROR_BACKOFF).await;
                continue;
            }
        };
        consecutive_accept_errors = 0;
        let local_ip = stream
            .local_addr()
            .map(|addr| addr.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let route_cache = route_cache.clone();
        let tls_cache = tls_cache.clone();
        let limits = limits.clone();
        let listener_context = listener_context.clone();
        tokio::spawn(async move {
            let connection_permit = ConnectionPermit::new(permit);
            // This includes certificate reload and the first ClientHello, so
            // stale or idle clients cannot pin connection permits indefinitely.
            let handshake = async {
                let tls = tls_cache
                    .acceptor()
                    .await
                    .context("failed to load TLS certificate")?;
                tls.accept(stream).await.context("TLS handshake failed")
            };
            let stream = match timeout(TLS_HANDSHAKE_TIMEOUT, handshake).await {
                Ok(Ok(stream)) => stream,
                Ok(Err(error)) => {
                    eprintln!("jig proxy TLS handshake from {remote_addr} failed: {error}");
                    return;
                }
                Err(_) => {
                    eprintln!("jig proxy TLS handshake timed out");
                    return;
                }
            };
            let request_context = RequestContext {
                remote_addr,
                proxy_port: listener_context.proxy_port,
                tls: true,
                local_ip,
                lan_ip: listener_context.lan_ip,
                health_token: listener_context.health_token,
            };
            let service = service_fn(move |req| {
                handle_request(
                    req,
                    request_context.clone(),
                    route_cache.clone(),
                    limits.clone(),
                )
            });
            let _connection_permit = connection_permit;
            let io = TokioIo::new(stream);
            if http2 {
                let mut builder =
                    hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
                builder
                    .http1()
                    .timer(TokioTimer::new())
                    .header_read_timeout(HTTP1_HEADER_READ_TIMEOUT)
                    // HTTP/2 has explicit stream and keepalive limits below.
                    .keep_alive(false);
                builder
                    .http2()
                    .timer(TokioTimer::new())
                    .max_concurrent_streams(Some(HTTP2_MAX_CONCURRENT_STREAMS))
                    .keep_alive_interval(Some(HTTP2_KEEP_ALIVE_INTERVAL))
                    .keep_alive_timeout(HTTP2_KEEP_ALIVE_TIMEOUT)
                    .max_frame_size(Some(HTTP2_MAX_FRAME_SIZE))
                    .max_header_list_size(HTTP2_MAX_HEADER_LIST_SIZE);
                let result = builder.serve_connection_with_upgrades(io, service).await;
                if let Err(error) = result {
                    if !is_disconnect_error(error.as_ref()) {
                        eprintln!("jig proxy https connection error: {error}");
                    }
                }
            } else {
                let mut http1 = hyper::server::conn::http1::Builder::new();
                http1.timer(TokioTimer::new());
                http1.header_read_timeout(HTTP1_HEADER_READ_TIMEOUT);
                // Keep HTTP/1 single-request so idle clients cannot pin connection permits.
                http1.keep_alive(false);
                let result = http1.serve_connection(io, service).with_upgrades().await;
                if let Err(error) = result {
                    if !is_disconnect_error(&error) {
                        eprintln!("jig proxy https connection error: {error}");
                    }
                }
            }
        });
    }
}

async fn handle_request(
    req: Request<Incoming>,
    context: RequestContext,
    route_cache: RouteCache,
    limits: ProxyLimits,
) -> Result<Response<ProxyBody>, Infallible> {
    let request_permit = match limits.requests.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Too many proxy requests are active.",
            ));
        }
    };
    let response = match route_request(
        req,
        context.clone(),
        route_cache,
        request_permit,
        limits.websockets.clone(),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!(
                "jig proxy request from {} failed: {error:#}",
                context.remote_addr
            );
            error_response(StatusCode::BAD_GATEWAY, "Bad gateway.")
        }
    };
    Ok(response)
}

async fn route_request(
    req: Request<Incoming>,
    context: RequestContext,
    route_cache: RouteCache,
    request_permit: OwnedSemaphorePermit,
    websocket_limit: Arc<Semaphore>,
) -> Result<Response<ProxyBody>> {
    if req.method() == Method::CONNECT {
        return Ok(error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "CONNECT requests are not supported by Jig proxy.",
        ));
    }
    if req.uri().path() == "/__jig_proxy_health" {
        // Health requests intentionally bypass normal route host validation,
        // but only after the loopback local/remote address and token checks
        // below succeed. Keep those checks together with this early path.
        if !health_request_allowed(
            &req,
            context.remote_addr.ip(),
            context.local_ip,
            &context.health_token,
        ) {
            return Ok(error_response(StatusCode::FORBIDDEN, "Forbidden."));
        }
        return Ok(health_response());
    }
    let host = match request_host_or_bad_request(&req) {
        Ok(host) => host,
        Err(response) => return Ok(*response),
    };
    let routes = route_cache.routes().await?;
    let Some(route) = find_route(&routes, &host).cloned() else {
        return Ok(not_found_response(
            &routes,
            &host,
            context.proxy_port,
            context.tls,
            ip_is_loopback(context.remote_addr.ip()),
        ));
    };
    if !route_allowed_for_remote_client(&route, context.remote_addr.ip()) {
        return Ok(not_found_response(
            &routes,
            &host,
            context.proxy_port,
            context.tls,
            false,
        ));
    }
    let proxy_ports = route_cache.proxy_listener_ports().await?;
    if route_targets_active_proxy_listener(&route, context.local_ip, context.lan_ip, &proxy_ports) {
        return Ok(error_response(
            StatusCode::BAD_GATEWAY,
            "Jig proxy loop detected.",
        ));
    }

    if is_websocket(req.headers()) {
        if req.method() != Method::GET {
            return Ok(error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "WebSocket proxying requires a GET upgrade request.",
            ));
        }
        if req.version() != Version::HTTP_11 {
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                "WebSocket proxying is supported for HTTP/1.1 upgrade requests only.",
            ));
        }
        // The request permit covers the HTTP upgrade handshake only. Once the
        // tunnel is established, the WebSocket semaphore tracks long-lived
        // upgraded connections so they cannot exhaust regular HTTP capacity.
        let version = req.version();
        return websocket(
            req,
            context.remote_addr,
            route,
            context.tls,
            version,
            websocket_limit,
        )
        .await;
    }

    proxy_http(
        req,
        context.remote_addr,
        route_cache.client(),
        route,
        context.tls,
        request_permit,
    )
    .await
}

async fn proxy_http(
    req: Request<Incoming>,
    remote_addr: SocketAddr,
    client: ProxyClient,
    route: Route,
    tls: bool,
    request_permit: OwnedSemaphorePermit,
) -> Result<Response<ProxyBody>> {
    let (mut parts, body) = req.into_parts();
    let inbound_version = parts.version;
    let path = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let uri: Uri = format!(
        "http://{}:{}{}",
        target_authority_host(&route.target_host),
        route.target_port,
        path
    )
    .parse()?;
    parts.uri = uri;
    // The legacy Hyper client is intentionally configured with `build_http()`,
    // so backend requests are normalized to HTTP/1.1 even when the client side
    // accepted HTTP/2 over TLS.
    parts.version = Version::HTTP_11;
    if let Err(error) = rewrite_proxy_headers(
        &mut parts.headers,
        remote_addr,
        &route,
        tls,
        inbound_version,
    ) {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            &format!("Invalid Jig proxy headers: {error}"),
        ));
    }
    match request_content_length(&parts.headers) {
        Ok(Some(length)) if length > MAX_BACKEND_REQUEST_BODY_BYTES as u64 => {
            return Ok(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body exceeds Jig proxy forwarding limit.",
            ));
        }
        Ok(_) => {}
        Err(_) => {
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                "Invalid request Content-Length.",
            ));
        }
    }
    let body = limited_request_body(body);
    let proxied = Request::from_parts(parts, body);
    let response = match timeout(BACKEND_HTTP_RESPONSE_TIMEOUT, client.request(proxied)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) if error_chain_contains_length_limit(&error) => {
            return Ok(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body exceeds Jig proxy forwarding limit.",
            ));
        }
        Ok(Err(error)) => return Err(error.into()),
        Err(_) => anyhow::bail!("Timed out waiting for backend response"),
    };
    let (mut parts, body) = response.into_parts();
    remove_hop_by_hop_headers(&mut parts.headers);
    remove_jig_proxy_headers(&mut parts.headers);
    append_via(&mut parts.headers, inbound_version);
    parts
        .headers
        .insert("x-jig-proxy", HeaderValue::from_static("1"));
    Ok(Response::from_parts(
        parts,
        body_with_request_permit(body, request_permit),
    ))
}

fn body_with_request_permit(body: Incoming, permit: OwnedSemaphorePermit) -> ProxyBody {
    let permit = Arc::new(permit);
    body.map_frame(move |frame| {
        let _permit = &permit;
        frame
    })
    .boxed_unsync()
}

fn limited_request_body(body: Incoming) -> ClientBody {
    Limited::new(body, MAX_BACKEND_REQUEST_BODY_BYTES).boxed_unsync()
}

async fn websocket(
    req: Request<Incoming>,
    remote_addr: SocketAddr,
    route: Route,
    tls: bool,
    version: Version,
    websocket_limit: Arc<Semaphore>,
) -> Result<Response<ProxyBody>> {
    if websocket_request_has_body(req.headers()) {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            "WebSocket upgrade requests with request bodies are not supported.",
        ));
    }
    let websocket_permit = match websocket_limit.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Too many WebSocket tunnels are already open.",
            ));
        }
    };
    let path = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    if path.chars().any(|ch| ch.is_control() || ch == ' ') {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            "WebSocket request path contains invalid characters.",
        ));
    }
    let websocket_accept = match websocket_accept_for_request(req.headers()) {
        Ok(value) => value,
        Err(error) => {
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid WebSocket upgrade request: {error}"),
            ));
        }
    };
    let target_ip = route
        .target_host
        .parse::<IpAddr>()
        .with_context(|| format!("Route target '{}' must be an IP literal", route.target_host))?;
    let mut backend = timeout(
        BACKEND_CONNECT_TIMEOUT,
        TcpStream::connect(SocketAddr::new(target_ip, route.target_port)),
    )
    .await
    .context("Timed out connecting to WebSocket backend")??;
    let mut headers = req.headers().clone();
    if let Err(error) = rewrite_proxy_headers(&mut headers, remote_addr, &route, tls, version) {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            &format!("Invalid Jig proxy headers: {error}"),
        ));
    }
    headers.remove(CONTENT_LENGTH);
    headers.insert(CONNECTION, HeaderValue::from_static("Upgrade"));
    headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
    let mut raw = format!("{} {} HTTP/1.1\r\n", req.method(), path);
    for (name, value) in &headers {
        if let Ok(value) = value.to_str() {
            // This manual HTTP/1.1 upgrade request is assembled from hyper
            // values: Method is typed, path_and_query was parsed by hyper, and
            // copied header names are already validated HeaderName values.
            // HeaderValue::to_str rejects CR/LF before copied values are used.
            if value.contains('\r') || value.contains('\n') {
                continue;
            }
            raw.push_str(name.as_str());
            raw.push_str(": ");
            raw.push_str(value);
            raw.push_str("\r\n");
        }
    }
    raw.push_str("\r\n");
    timeout(
        WEBSOCKET_BACKEND_WRITE_TIMEOUT,
        backend.write_all(raw.as_bytes()),
    )
    .await
    .context("Timed out writing WebSocket upgrade request to backend")??;

    let (status, headers, buffered) = timeout(
        BACKEND_HTTP_RESPONSE_TIMEOUT,
        read_backend_headers(&mut backend),
    )
    .await
    .context("Timed out waiting for WebSocket backend response headers")??;
    if status != StatusCode::SWITCHING_PROTOCOLS {
        // Non-101 WebSocket responses are usually small auth/error pages.
        // Drain them with explicit byte and time bounds before replying.
        let body = match timeout(
            BACKEND_BODY_DRAIN_TIMEOUT,
            complete_backend_body(&mut backend, &headers, buffered),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => anyhow::bail!("Timed out draining backend non-upgrade WebSocket response"),
        };
        let connection_headers = raw_connection_header_names(&headers);
        let mut builder = Response::builder().status(status);
        for (name, value) in headers {
            if raw_header_is_hop_by_hop(&name, &connection_headers)
                || name == CONTENT_LENGTH
                || is_jig_proxy_header(&name)
            {
                continue;
            }
            builder = builder.header(name, value);
        }
        builder = builder
            .header(CONTENT_LENGTH, body.len().to_string())
            .header(VIA, via_value(version));
        let mut response = builder.body(full_body(body))?;
        response
            .headers_mut()
            .insert("x-jig-proxy", HeaderValue::from_static("1"));
        return Ok(response);
    }
    if !websocket_accept_matches(&headers, &websocket_accept) {
        return Ok(error_response(
            StatusCode::BAD_GATEWAY,
            "WebSocket backend returned an invalid upgrade response.",
        ));
    }
    if !websocket_extensions_allowed(req.headers(), &headers) {
        return Ok(error_response(
            StatusCode::BAD_GATEWAY,
            "WebSocket backend negotiated unsupported extensions.",
        ));
    }
    if !websocket_subprotocol_allowed(req.headers(), &headers) {
        return Ok(error_response(
            StatusCode::BAD_GATEWAY,
            "WebSocket backend negotiated an unsupported subprotocol.",
        ));
    }
    let upgrade = hyper::upgrade::on(req);
    tokio::spawn(async move {
        let _websocket_permit = websocket_permit;
        let upgraded = match timeout(WEBSOCKET_UPGRADE_TIMEOUT, upgrade).await {
            Ok(Ok(upgraded)) => upgraded,
            Ok(Err(error)) => {
                eprintln!("jig proxy websocket upgrade failed after 101 response: {error}");
                return;
            }
            Err(_) => {
                eprintln!("jig proxy websocket upgrade timed out after 101 response");
                return;
            }
        };
        let mut client = TokioIo::new(upgraded);
        if !buffered.is_empty() {
            match timeout(WEBSOCKET_BACKEND_WRITE_TIMEOUT, client.write_all(&buffered)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    eprintln!(
                        "jig proxy websocket failed to flush buffered backend bytes: {error}"
                    );
                    return;
                }
                Err(_) => {
                    eprintln!("jig proxy websocket timed out flushing buffered backend bytes");
                    return;
                }
            }
        }
        tunnel_websocket(client, backend).await;
    });

    let connection_headers = raw_connection_header_names(&headers);
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        if raw_header_is_hop_by_hop(&name, &connection_headers) || is_jig_proxy_header(&name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    let mut response = builder.body(empty_body())?;
    let headers = response.headers_mut();
    headers.insert(CONNECTION, HeaderValue::from_static("Upgrade"));
    headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
    headers.append(VIA, header_value_or_default(via_value(version), VIA_VALUE));
    headers.insert("x-jig-proxy", HeaderValue::from_static("1"));
    Ok(response)
}

fn websocket_accept_for_request(headers: &HeaderMap) -> Result<String> {
    let mut keys = headers.get_all("sec-websocket-key").iter();
    let key = match (keys.next(), keys.next()) {
        (Some(key), None) => key,
        (None, _) => bail!("WebSocket request missing Sec-WebSocket-Key"),
        (Some(_), Some(_)) => bail!("WebSocket request has conflicting Sec-WebSocket-Key headers"),
    }
    .to_str()
    .context("WebSocket request has non-UTF8 Sec-WebSocket-Key")?;
    Ok(websocket_accept_for_key(key))
}

fn websocket_accept_for_key(key: &str) -> String {
    let mut sha1 = Sha1::new();
    sha1.update(key.as_bytes());
    sha1.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    BASE64_STANDARD.encode(sha1.finalize())
}

fn websocket_accept_matches(headers: &[(HeaderName, HeaderValue)], expected: &str) -> bool {
    let mut actual = None;
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.as_str().eq_ignore_ascii_case("sec-websocket-accept"))
    {
        let Ok(value) = value.to_str() else {
            return false;
        };
        if actual.replace(value.trim()).is_some() {
            return false;
        }
    }
    actual == Some(expected)
}

fn websocket_extensions_allowed(
    request_headers: &HeaderMap,
    response_headers: &[(HeaderName, HeaderValue)],
) -> bool {
    let requested = websocket_extension_names(
        request_headers
            .get_all("sec-websocket-extensions")
            .iter()
            .filter_map(|value| value.to_str().ok()),
    );
    response_headers
        .iter()
        .filter(|(name, _)| {
            name.as_str()
                .eq_ignore_ascii_case("sec-websocket-extensions")
        })
        .all(|(_, value)| {
            value.to_str().ok().is_some_and(|value| {
                websocket_extension_names(std::iter::once(value))
                    .iter()
                    .all(|extension| requested.contains(extension))
            })
        })
}

fn websocket_extension_names<'a>(values: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
    values
        .into_iter()
        .flat_map(|value| value.split(','))
        .filter_map(|extension| extension.split(';').next())
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.to_ascii_lowercase())
        .collect()
}

fn websocket_subprotocol_allowed(
    request_headers: &HeaderMap,
    response_headers: &[(HeaderName, HeaderValue)],
) -> bool {
    let requested = websocket_token_values(
        request_headers
            .get_all("sec-websocket-protocol")
            .iter()
            .filter_map(|value| value.to_str().ok()),
    );
    let mut selected = Vec::new();
    for (_, value) in response_headers
        .iter()
        .filter(|(name, _)| name.as_str().eq_ignore_ascii_case("sec-websocket-protocol"))
    {
        let Ok(value) = value.to_str() else {
            return false;
        };
        selected.extend(websocket_token_values(std::iter::once(value)));
    }
    match selected.as_slice() {
        [] => true,
        [protocol] => requested.iter().any(|item| item == protocol),
        _ => false,
    }
}

fn websocket_token_values<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    values
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn is_disconnect_error(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
            if matches!(
                io_error.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionReset
            ) {
                return true;
            }
        }
        current = error.source();
    }
    false
}

fn error_chain_contains_length_limit(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.downcast_ref::<LengthLimitError>().is_some() {
            return true;
        }
        current = error.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;
    use std::fs;
    #[cfg(unix)]
    use std::io::BufReader;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use http_body_util::Empty;
    use hyper::header::HeaderValue;
    #[cfg(unix)]
    use rustls::pki_types::ServerName;
    use tempfile::tempdir;
    use tokio::time::{sleep, timeout};
    #[cfg(unix)]
    use tokio_rustls::TlsConnector;

    use super::*;
    use crate::state::now_ms;
    use crate::types::RouteMode;

    #[test]
    fn websocket_detection_requires_connection_upgrade() {
        let mut headers = HeaderMap::new();
        headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
        headers.insert("sec-websocket-key", HeaderValue::from_static("abc"));
        headers.insert("sec-websocket-version", HeaderValue::from_static("13"));
        assert!(!is_websocket(&headers));

        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive, Upgrade"));
        assert!(is_websocket(&headers));

        headers.remove("sec-websocket-key");
        assert!(!is_websocket(&headers));
    }

    #[test]
    fn websocket_upgrade_rejects_request_bodies() {
        let mut headers = HeaderMap::new();
        assert!(!websocket_request_has_body(&headers));

        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
        assert!(!websocket_request_has_body(&headers));

        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("1"));
        assert!(websocket_request_has_body(&headers));

        headers.remove(CONTENT_LENGTH);
        headers.insert(TRANSFER_ENCODING, HeaderValue::from_static("chunked"));
        assert!(websocket_request_has_body(&headers));
    }

    #[test]
    fn websocket_accept_header_must_match_request_key() {
        let expected = websocket_accept_for_key("client-key");
        let valid = vec![(
            HeaderName::from_static("sec-websocket-accept"),
            HeaderValue::from_str(&expected).unwrap(),
        )];
        assert!(websocket_accept_matches(&valid, &expected));

        let invalid = vec![(
            HeaderName::from_static("sec-websocket-accept"),
            HeaderValue::from_static("wrong"),
        )];
        assert!(!websocket_accept_matches(&invalid, &expected));

        let duplicate = vec![
            (
                HeaderName::from_static("sec-websocket-accept"),
                HeaderValue::from_str(&expected).unwrap(),
            ),
            (
                HeaderName::from_static("sec-websocket-accept"),
                HeaderValue::from_str(&expected).unwrap(),
            ),
        ];
        assert!(!websocket_accept_matches(&duplicate, &expected));
    }

    #[test]
    fn websocket_accept_request_requires_single_key() {
        let mut request = HeaderMap::new();
        request.insert("sec-websocket-key", HeaderValue::from_static("a"));
        assert_eq!(
            websocket_accept_for_request(&request).unwrap(),
            websocket_accept_for_key("a")
        );

        request.append("sec-websocket-key", HeaderValue::from_static("b"));
        let error = websocket_accept_for_request(&request)
            .unwrap_err()
            .to_string();
        assert!(error.contains("conflicting Sec-WebSocket-Key"));
    }

    #[test]
    fn websocket_extensions_allow_backend_subset_of_client_request() {
        let mut request = HeaderMap::new();
        request.insert(
            "sec-websocket-extensions",
            HeaderValue::from_static("permessage-deflate; client_max_window_bits, x-dev"),
        );
        let response = vec![(
            HeaderName::from_static("sec-websocket-extensions"),
            HeaderValue::from_static("permessage-deflate"),
        )];

        assert!(websocket_extensions_allowed(&request, &response));
    }

    #[test]
    fn websocket_extensions_reject_unrequested_backend_extension() {
        let request = HeaderMap::new();
        let response = vec![(
            HeaderName::from_static("sec-websocket-extensions"),
            HeaderValue::from_static("permessage-deflate"),
        )];

        assert!(!websocket_extensions_allowed(&request, &response));
    }

    #[test]
    fn websocket_subprotocol_must_match_client_request() {
        let mut request = HeaderMap::new();
        request.insert(
            "sec-websocket-protocol",
            HeaderValue::from_static("chat, superchat"),
        );
        let selected = vec![(
            HeaderName::from_static("sec-websocket-protocol"),
            HeaderValue::from_static("superchat"),
        )];
        let unrequested = vec![(
            HeaderName::from_static("sec-websocket-protocol"),
            HeaderValue::from_static("admin"),
        )];
        let duplicate = vec![
            (
                HeaderName::from_static("sec-websocket-protocol"),
                HeaderValue::from_static("chat"),
            ),
            (
                HeaderName::from_static("sec-websocket-protocol"),
                HeaderValue::from_static("superchat"),
            ),
        ];

        assert!(websocket_subprotocol_allowed(&request, &selected));
        assert!(!websocket_subprotocol_allowed(&request, &unrequested));
        assert!(!websocket_subprotocol_allowed(&request, &duplicate));
    }

    #[test]
    fn health_request_requires_loopback_client_and_host() {
        let loopback = "127.0.0.1".parse().unwrap();
        let remote = "192.168.1.50".parse().unwrap();
        let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let localhost = Request::builder()
            .header(HOST, "localhost")
            .header("x-jig-proxy-health-token", token)
            .body(())
            .unwrap();
        let loopback_literal = Request::builder()
            .header(HOST, "127.0.0.1:1355")
            .header("x-jig-proxy-health-token", token)
            .body(())
            .unwrap();
        let ipv6_loopback = Request::builder()
            .header(HOST, "[::1]:1355")
            .header("x-jig-proxy-health-token", token)
            .body(())
            .unwrap();
        let malformed_ipv6_loopback = Request::builder()
            .header(HOST, "[::1]evil")
            .header("x-jig-proxy-health-token", token)
            .body(())
            .unwrap();
        let routed_host = Request::builder()
            .header(HOST, "web.demo.localhost")
            .header("x-jig-proxy-health-token", token)
            .body(())
            .unwrap();
        let wrong_token = Request::builder()
            .header(HOST, "localhost")
            .header("x-jig-proxy-health-token", "wrong")
            .body(())
            .unwrap();
        let missing_token = Request::builder()
            .header(HOST, "localhost")
            .body(())
            .unwrap();

        assert!(health_request_allowed(
            &localhost, loopback, loopback, token
        ));
        assert!(health_request_allowed(
            &loopback_literal,
            loopback,
            loopback,
            token
        ));
        assert!(health_request_allowed(
            &ipv6_loopback,
            "::1".parse().unwrap(),
            "::1".parse().unwrap(),
            token
        ));
        assert!(health_request_allowed(
            &localhost,
            "::ffff:127.0.0.1".parse().unwrap(),
            "::ffff:127.0.0.1".parse().unwrap(),
            token
        ));
        assert!(!health_request_allowed(&localhost, remote, loopback, token));
        assert!(!health_request_allowed(&localhost, loopback, remote, token));
        assert!(!health_request_allowed(
            &malformed_ipv6_loopback,
            loopback,
            loopback,
            token
        ));
        assert!(!health_request_allowed(
            &routed_host,
            loopback,
            loopback,
            token
        ));
        assert!(!health_request_allowed(
            &wrong_token,
            loopback,
            loopback,
            token
        ));
        assert!(!health_request_allowed(
            &missing_token,
            loopback,
            loopback,
            token
        ));
        assert!(!constant_time_ascii_eq("health-token-prefix", token));
        assert!(!constant_time_ascii_eq("short", "short"));
        assert!(constant_time_ascii_eq(token, token));
    }

    #[test]
    fn request_host_rejects_conflicting_authority_and_host() {
        let request = Request::builder()
            .uri("https://web.demo.localhost/")
            .header(HOST, "api.demo.localhost")
            .body(())
            .unwrap();

        let error = request_host(&request).unwrap_err().to_string();
        assert!(error.contains("Conflicting request authority"));
    }

    #[test]
    fn request_host_rejects_duplicate_host_headers() {
        let mut request = Request::builder()
            .header(HOST, "web.demo.localhost")
            .body(())
            .unwrap();
        request
            .headers_mut()
            .append(HOST, HeaderValue::from_static("api.demo.localhost"));

        let error = request_host(&request).unwrap_err().to_string();

        assert!(error.contains("Conflicting request Host headers"));
    }

    #[test]
    fn invalid_request_host_maps_to_bad_request_response() {
        let request = Request::builder().body(()).unwrap();

        let response = request_host_or_bad_request(&request).unwrap_err();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn request_host_prefers_matching_authority() {
        let request = Request::builder()
            .uri("https://web.demo.localhost/")
            .header(HOST, "web.demo.localhost")
            .body(())
            .unwrap();

        assert_eq!(request_host(&request).unwrap(), "web.demo.localhost");
    }

    #[test]
    fn target_authority_brackets_ipv6_literals() {
        assert_eq!(target_authority_host("::1"), "[::1]");
        assert_eq!(target_authority_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(target_authority_host("localhost"), "localhost");
    }

    #[test]
    fn chunked_backend_body_is_decoded() {
        let body = decode_chunked_body(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n").unwrap();
        assert_eq!(body, Bytes::from_static(b"hello world"));
    }

    #[test]
    fn backend_status_requires_http_prefix() {
        assert_eq!(
            parse_backend_status(b"HTTP/1.1 204 No Content").unwrap(),
            StatusCode::NO_CONTENT
        );
        let error = parse_backend_status(b"BOGUS 200 OK")
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP/"));
    }

    #[test]
    fn backend_status_rejects_non_standard_spacing() {
        assert!(parse_backend_status(b"HTTP/1.1\t204 No Content").is_err());
        assert!(parse_backend_status(b"HTTP/1.1  204 No Content").is_err());
        assert!(parse_backend_status(b"HTTP/1.1 204\tNo Content").is_err());
    }

    #[test]
    fn chunked_scanner_advances_across_complete_chunks() {
        let mut scanner = ChunkedMessageScanner::default();
        assert_eq!(scanner.scan(b"5\r\nhello\r\n").unwrap(), None);
        assert_eq!(
            scanner.scan(b"5\r\nhello\r\n0\r\n\r\n").unwrap(),
            Some("5\r\nhello\r\n0\r\n\r\n".len())
        );
    }

    #[test]
    fn chunked_scanner_does_not_count_pending_data_as_header() {
        let mut scanner = ChunkedMessageScanner::default();
        let mut raw = b"3000\r\n".to_vec();
        raw.extend(std::iter::repeat_n(b'a', MAX_CHUNK_HEADER_BYTES + 1));

        assert_eq!(scanner.scan(&raw).unwrap(), None);

        raw.resize("3000\r\n".len() + 0x3000, b'a');
        raw.extend_from_slice(b"\r\n0\r\n\r\n");

        assert_eq!(scanner.scan(&raw).unwrap(), Some(raw.len()));
    }

    #[test]
    fn backend_content_length_rejects_duplicate_values() {
        let headers = vec![
            (CONTENT_LENGTH, HeaderValue::from_static("5")),
            (CONTENT_LENGTH, HeaderValue::from_static("6")),
        ];

        let error = content_length(&headers).unwrap_err().to_string();

        assert!(error.contains("multiple Content-Length"));

        let error = content_length(&[(CONTENT_LENGTH, HeaderValue::from_static("5, 5"))])
            .unwrap_err()
            .to_string();

        assert!(error.contains("multiple Content-Length"));
    }

    #[test]
    fn disconnect_error_uses_io_error_kind() {
        let broken_pipe = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed");
        assert!(is_disconnect_error(&broken_pipe));

        let misleading_message = std::io::Error::other("contains broken pipe text");
        assert!(!is_disconnect_error(&misleading_message));
    }

    #[derive(Debug)]
    struct WrappedError {
        source: std::io::Error,
    }

    impl fmt::Display for WrappedError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "wrapped error")
        }
    }

    impl Error for WrappedError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.source)
        }
    }

    #[test]
    fn disconnect_error_checks_nested_sources() {
        let wrapped = WrappedError {
            source: std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset"),
        };

        assert!(is_disconnect_error(&wrapped));
    }

    #[test]
    fn removes_standard_and_connection_named_hop_by_hop_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("x-debug-hop"));
        headers.insert("x-debug-hop", HeaderValue::from_static("1"));
        headers.insert("te", HeaderValue::from_static("trailers"));
        headers.insert("trailer", HeaderValue::from_static("x-trailer"));
        headers.insert("proxy-connection", HeaderValue::from_static("keep-alive"));
        headers.insert("proxy-authorization", HeaderValue::from_static("secret"));
        headers.insert("x-end-to-end", HeaderValue::from_static("keep"));

        remove_hop_by_hop_headers(&mut headers);

        assert!(!headers.contains_key(CONNECTION));
        assert!(!headers.contains_key("x-debug-hop"));
        assert!(!headers.contains_key("te"));
        assert!(!headers.contains_key("trailer"));
        assert!(!headers.contains_key("proxy-connection"));
        assert!(!headers.contains_key("proxy-authorization"));
        assert_eq!(
            headers.get("x-end-to-end"),
            Some(&HeaderValue::from_static("keep"))
        );
    }

    #[test]
    fn removes_backend_owned_jig_proxy_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-jig-proxy", HeaderValue::from_static("backend"));
        headers.insert("x-jig-proxy-pid", HeaderValue::from_static("1234"));
        headers.insert("x-jig-proxy-hops", HeaderValue::from_static("7"));
        headers.insert("x-end-to-end", HeaderValue::from_static("keep"));

        remove_jig_proxy_headers(&mut headers);

        assert!(!headers.contains_key("x-jig-proxy"));
        assert!(!headers.contains_key("x-jig-proxy-pid"));
        assert!(!headers.contains_key("x-jig-proxy-hops"));
        assert_eq!(
            headers.get("x-end-to-end"),
            Some(&HeaderValue::from_static("keep"))
        );
    }

    #[test]
    fn rewrite_proxy_headers_increments_inbound_proxy_hop_header() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("x-jig-proxy-hops"));
        headers.insert("x-jig-proxy-hops", HeaderValue::from_static("2"));
        let route = Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        rewrite_proxy_headers(
            &mut headers,
            "127.0.0.1:5000".parse().unwrap(),
            &route,
            false,
            Version::HTTP_11,
        )
        .unwrap();

        assert!(!headers.contains_key(CONNECTION));
        assert_eq!(
            headers.get("x-jig-proxy-hops"),
            Some(&HeaderValue::from_static("3"))
        );
        assert_eq!(headers.get(VIA), Some(&HeaderValue::from_static(VIA_VALUE)));
    }

    #[test]
    fn rewrite_proxy_headers_rejects_excessive_proxy_hops() {
        let mut headers = HeaderMap::new();
        headers.insert("x-jig-proxy-hops", HeaderValue::from_static("8"));
        let route = Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        let error = rewrite_proxy_headers(
            &mut headers,
            "127.0.0.1:5000".parse().unwrap(),
            &route,
            false,
            Version::HTTP_11,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("hop limit"));
    }

    #[test]
    fn rewrite_proxy_headers_replaces_inbound_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.10"));
        headers.insert("x-forwarded-port", HeaderValue::from_static("443"));
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.11"));
        headers.insert("forwarded", HeaderValue::from_static("for=203.0.113.10"));
        let route = Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        rewrite_proxy_headers(
            &mut headers,
            "127.0.0.1:5000".parse().unwrap(),
            &route,
            false,
            Version::HTTP_11,
        )
        .unwrap();

        assert_eq!(
            headers.get("x-forwarded-for"),
            Some(&HeaderValue::from_static("127.0.0.1"))
        );
        assert_eq!(
            headers.get(HOST),
            Some(&HeaderValue::from_static("web.demo.localhost"))
        );
        assert!(!headers.contains_key("forwarded"));
        assert!(!headers.contains_key("x-forwarded-port"));
        assert!(!headers.contains_key("x-real-ip"));
    }

    #[test]
    fn rewrite_proxy_headers_removes_inbound_proxy_owned_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-jig-proxy", HeaderValue::from_static("spoofed"));
        headers.insert("x-jig-proxy-pid", HeaderValue::from_static("9999"));
        headers.insert("x-jig-proxy-hops", HeaderValue::from_static("2"));
        let route = Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        rewrite_proxy_headers(
            &mut headers,
            "127.0.0.1:5000".parse().unwrap(),
            &route,
            false,
            Version::HTTP_11,
        )
        .unwrap();

        assert!(!headers.contains_key("x-jig-proxy"));
        assert!(!headers.contains_key("x-jig-proxy-pid"));
        assert_eq!(
            headers.get("x-jig-proxy-hops"),
            Some(&HeaderValue::from_static("3"))
        );
    }

    #[test]
    fn request_content_length_limit_rejects_oversized_requests() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&(MAX_BACKEND_REQUEST_BODY_BYTES as u64 + 1).to_string())
                .unwrap(),
        );

        assert!(
            request_content_length(&headers).unwrap().unwrap()
                > MAX_BACKEND_REQUEST_BODY_BYTES as u64
        );

        headers.insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&(MAX_BACKEND_REQUEST_BODY_BYTES as u64).to_string()).unwrap(),
        );

        assert!(
            request_content_length(&headers).unwrap().unwrap()
                <= MAX_BACKEND_REQUEST_BODY_BYTES as u64
        );
    }

    #[test]
    fn request_content_length_rejects_conflicting_duplicates() {
        let mut headers = HeaderMap::new();
        headers.append(CONTENT_LENGTH, HeaderValue::from_static("5"));
        headers.append(CONTENT_LENGTH, HeaderValue::from_static("7"));

        let error = request_content_length(&headers).unwrap_err().to_string();
        assert!(error.contains("conflicting Content-Length"));

        let mut matching = HeaderMap::new();
        matching.append(CONTENT_LENGTH, HeaderValue::from_static("5"));
        matching.append(CONTENT_LENGTH, HeaderValue::from_static("5"));
        assert_eq!(request_content_length(&matching).unwrap(), Some(5));
    }

    #[test]
    fn request_content_length_rejects_transfer_encoding_conflicts() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("5"));
        headers.insert(TRANSFER_ENCODING, HeaderValue::from_static("chunked"));

        let error = request_content_length(&headers).unwrap_err().to_string();

        assert!(error.contains("Content-Length and Transfer-Encoding"));
    }

    #[test]
    fn rewrite_proxy_headers_appends_via() {
        let mut headers = HeaderMap::new();
        headers.insert(VIA, HeaderValue::from_static("1.0 upstream"));
        let route = Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        rewrite_proxy_headers(
            &mut headers,
            "127.0.0.1:5000".parse().unwrap(),
            &route,
            false,
            Version::HTTP_2,
        )
        .unwrap();

        let via = headers
            .get_all(VIA)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(via, vec!["1.0 upstream", "2.0 jig"]);
    }

    #[test]
    fn backend_header_parser_preserves_obs_text_values() {
        let raw = b"HTTP/1.1 200 OK\r\nx-binary: \xFF\r\n\r\n";
        let end = find_header_end(raw, 0).unwrap();
        let header_block = &raw[..end - 4];
        let status_line_end = find_crlf(header_block, 0).unwrap();

        let headers = parse_backend_headers(&header_block[status_line_end..]).unwrap();

        assert_eq!(headers[0].0, HeaderName::from_static("x-binary"));
        assert_eq!(headers[0].1.as_bytes(), b"\xFF");
    }

    #[test]
    fn backend_header_parser_rejects_whitespace_before_colon() {
        let error = parse_backend_headers(b"x-bad : value\r\n")
            .unwrap_err()
            .to_string();

        assert!(error.contains("whitespace before ':'"));
    }

    #[test]
    fn backend_header_parser_rejects_bare_cr_in_values() {
        let error = parse_backend_headers(b"x-bad: value\rcarry\r\n")
            .unwrap_err()
            .to_string();

        assert!(error.contains("bare CR or LF"));
    }

    #[test]
    fn backend_header_parser_caps_header_count() {
        let raw = "x-a: b\r\n".repeat(MAX_BACKEND_HEADER_COUNT + 1);

        let error = parse_backend_headers(raw.as_bytes())
            .unwrap_err()
            .to_string();

        assert!(error.contains("exceeded"));
    }

    #[test]
    fn chunked_body_parser_rejects_overflowing_size() {
        let error = decode_chunked_body(b"ffffffffffffffff\r\n")
            .unwrap_err()
            .to_string();

        assert!(error.contains("overflowed"));
    }

    #[test]
    fn non_loopback_clients_cannot_use_remote_target_aliases() {
        let route = Route {
            hostname: "remote.demo.localhost".into(),
            target_host: "10.0.0.5".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        assert!(!route_allowed_for_remote_client(
            &route,
            "192.168.1.50".parse().unwrap()
        ));
        assert!(route_allowed_for_remote_client(
            &route,
            "127.0.0.1".parse().unwrap()
        ));
    }

    #[test]
    fn route_targets_must_be_ip_literals_at_serve_time() {
        let route = Route {
            hostname: "remote.demo.localhost".into(),
            target_host: "example.com".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        assert!(!route_allowed_for_remote_client(
            &route,
            "127.0.0.1".parse().unwrap()
        ));
        assert!(!route_allowed_for_remote_client(
            &route,
            "192.168.1.50".parse().unwrap()
        ));
    }

    #[test]
    fn non_loopback_clients_cannot_use_loopback_target_aliases() {
        let route = Route {
            hostname: "api.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        assert!(!route_allowed_for_remote_client(
            &route,
            "192.168.1.50".parse().unwrap()
        ));
    }

    #[test]
    fn non_loopback_clients_can_use_loopback_target_process_routes() {
        let route = Route {
            hostname: "api.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: Some(std::process::id()),
            owner_start_token: None,
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        };

        assert!(route_allowed_for_remote_client(
            &route,
            "192.168.1.50".parse().unwrap()
        ));
    }

    #[test]
    fn route_targets_active_proxy_listener_rejects_loopback_self_route() {
        let route = Route {
            hostname: "api.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4300,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        assert!(route_targets_active_proxy_listener(
            &route,
            "127.0.0.1".parse().unwrap(),
            None,
            &[4300]
        ));
        assert!(!route_targets_active_proxy_listener(
            &route,
            "127.0.0.1".parse().unwrap(),
            None,
            &[4301]
        ));
    }

    #[test]
    fn route_targets_active_proxy_listener_uses_cached_lan_ip() {
        let route = Route {
            hostname: "api.demo.localhost".into(),
            target_host: "192.0.2.10".into(),
            target_port: 4300,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        };

        assert!(route_targets_active_proxy_listener(
            &route,
            "127.0.0.1".parse().unwrap(),
            Some("192.0.2.10".parse().unwrap()),
            &[4300]
        ));
    }

    #[test]
    fn non_loopback_clients_cannot_use_remote_target_process_routes() {
        let route = Route {
            hostname: "web.demo.localhost".into(),
            target_host: "10.0.0.5".into(),
            target_port: 4000,
            owner_pid: Some(std::process::id()),
            owner_start_token: None,
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        };

        assert!(!route_allowed_for_remote_client(
            &route,
            "192.168.1.50".parse().unwrap()
        ));
        assert!(route_allowed_for_remote_client(
            &route,
            "127.0.0.1".parse().unwrap()
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn tls_cache_reloads_after_freshness_ttl() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            ..ProxySettings::default()
        };
        crate::certs::ensure_for_hosts(&settings, &["web.demo.localhost".into()]).unwrap();
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let cache = TlsCache::new(store, true);
        cache.acceptor().await.unwrap();

        let stale_loaded_at = Instant::now() - ROUTE_CACHE_MAX_AGE - Duration::from_millis(1);
        {
            let mut inner = cache.inner.write().unwrap();
            inner.loaded_at = Some(stale_loaded_at);
        }

        cache.acceptor().await.unwrap();
        let inner = cache.inner.read().unwrap();
        assert!(inner.loaded_at.unwrap() > stale_loaded_at);
    }

    #[cfg(unix)]
    #[test]
    fn tls_acceptor_rejects_symlinked_key_file() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            ..ProxySettings::default()
        };
        crate::certs::ensure_for_hosts(&settings, &["web.demo.localhost".into()]).unwrap();
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let key_path = store.leaf_key_path();
        let outside_key = temp.path().join("outside-leaf-key.pem");
        fs::rename(&key_path, &outside_key).unwrap();
        symlink(&outside_key, &key_path).unwrap();

        let result = tls_acceptor(&store, false);

        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Failed to open TLS certificate file")
        );
    }

    #[cfg(unix)]
    #[test]
    fn tls_acceptor_retries_transient_key_pair_mismatch() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            ..ProxySettings::default()
        };
        crate::certs::ensure_for_hosts(&settings, &["web.demo.localhost".into()]).unwrap();
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let key_path = store.leaf_key_path();
        let original_key = fs::read_to_string(&key_path).unwrap();
        fs::write(
            &key_path,
            rcgen::KeyPair::generate().unwrap().serialize_pem(),
        )
        .unwrap();

        let restore_key_path = key_path.clone();
        let restore = std::thread::spawn(move || {
            std::thread::sleep(TLS_RELOAD_FILE_RETRY_DELAY / 2);
            fs::write(restore_key_path, original_key).unwrap();
        });

        tls_acceptor(&store, false).unwrap();
        restore.join().unwrap();
    }

    #[tokio::test]
    async fn backend_headers_reject_truncated_response() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_backend_headers(&mut stream)
                .await
                .unwrap_err()
                .to_string()
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"HTTP/1.1 101 Switching").await.unwrap();
        drop(client);

        let error = server.await.unwrap();
        assert!(error.contains("before completing response headers"));
    }

    #[tokio::test]
    async fn backend_headers_reject_malformed_status_line() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_backend_headers(&mut stream)
                .await
                .unwrap_err()
                .to_string()
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"NOTHTTP\r\n\r\n").await.unwrap();

        let error = server.await.unwrap();
        assert!(error.contains("HTTP/"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn https_proxy_serves_h2_requests() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            http2: true,
            ..ProxySettings::default()
        };
        let hostname = "web.demo.localhost";
        crate::certs::ensure_for_hosts(&settings, &[hostname.to_string()]).unwrap();
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let backend_port = backend.local_addr().unwrap().port();
        let backend_task = tokio::spawn(async move {
            let (mut stream, _) = backend.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 6\r\n\r\nh2-ok\n")
                .await
                .unwrap();
        });
        store
            .add_route(Route {
                hostname: hostname.into(),
                target_host: "127.0.0.1".into(),
                target_port: backend_port,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let route_cache = RouteCache::new(store.clone());
        let tls_cache = TlsCache::new(store.clone(), true);
        let server = tokio::spawn(async move {
            serve_https(
                listener,
                route_cache,
                tls_cache,
                ProxyLimits::new(),
                true,
                ListenerContext {
                    proxy_port: addr.port(),
                    lan_ip: None,
                    health_token: Arc::from("test-health-token"),
                },
            )
            .await
            .unwrap();
        });

        let mut roots = rustls::RootCertStore::empty();
        let cert_file = std::fs::File::open(store.ca_path()).unwrap();
        for cert in rustls_pemfile::certs(&mut BufReader::new(cert_file)) {
            roots.add(cert.unwrap()).unwrap();
        }
        let mut client_config = rustls::ClientConfig::builder_with_provider(
            rustls::crypto::aws_lc_rs::default_provider().into(),
        )
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        client_config.alpn_protocols = vec![b"h2".to_vec()];

        let connector = TlsConnector::from(Arc::new(client_config));
        let server_name = ServerName::try_from(hostname).unwrap().to_owned();
        let stream = TcpStream::connect(addr).await.unwrap();
        let tls = connector.connect(server_name, stream).await.unwrap();
        assert_eq!(tls.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
        let (mut sender, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
                .await
                .unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let request = Request::builder()
            .uri(format!("https://{hostname}/"))
            .header(HOST, hostname)
            .body(Empty::<Bytes>::new())
            .unwrap();
        let response = sender.send_request(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, Bytes::from_static(b"h2-ok\n"));
        backend_task.await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn https_not_found_uses_bound_proxy_port() {
        let routes = vec![Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        }];

        let response = not_found_response(&routes, "missing.demo.localhost", 1443, true, true);
        let collected = response.into_body().collect().await.unwrap();
        let text = String::from_utf8(collected.to_bytes().to_vec()).unwrap();
        assert!(text.contains("https://web.demo.localhost:1443"));
        assert!(!text.contains(":443"));
    }

    #[tokio::test]
    async fn not_found_hides_routes_for_non_loopback_clients() {
        let routes = vec![Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        }];

        let response = not_found_response(&routes, "missing.demo.localhost", 1355, false, false);
        let collected = response.into_body().collect().await.unwrap();
        let text = String::from_utf8(collected.to_bytes().to_vec()).unwrap();
        assert!(text.contains("hidden"));
        assert!(!text.contains("web.demo.localhost"));
    }

    #[tokio::test]
    async fn request_limit_exhaustion_returns_service_unavailable() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve_http(
            listener,
            RouteCache::new(store),
            ProxyLimits {
                connections: Arc::new(Semaphore::new(8)),
                requests: Arc::new(Semaphore::new(0)),
                websockets: Arc::new(Semaphore::new(8)),
            },
            ListenerContext {
                proxy_port: addr.port(),
                lan_ip: None,
                health_token: Arc::from("test-health-token"),
            },
        ));

        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: web.demo.localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut received = Vec::new();
        timeout(Duration::from_secs(2), client.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8_lossy(&received);

        assert!(response.contains("503 Service Unavailable"));
        assert!(response.contains("Too many proxy requests are active."));
        server.abort();
    }

    #[tokio::test]
    async fn http_proxy_streams_backend_response() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            http_port: 0,
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let backend_port = backend.local_addr().unwrap().port();
        let backend_task = tokio::spawn(async move {
            let (mut stream, _) = backend.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n",
                )
                .await
                .unwrap();
            sleep(Duration::from_millis(500)).await;
            stream.write_all(b"6\r\n world\r\n0\r\n\r\n").await.unwrap();
        });

        let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
        let proxy_port = wait_for_http_port(&store).await;
        store
            .add_route(Route {
                hostname: "stream.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: backend_port,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        client
            .write_all(
                b"GET / HTTP/1.1\r\nHost: stream.demo.localhost\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut received = Vec::new();
        timeout(Duration::from_millis(250), async {
            let mut buffer = [0u8; 1024];
            loop {
                let n = client.read(&mut buffer).await.unwrap();
                assert_ne!(n, 0, "proxy closed before first chunk");
                received.extend_from_slice(&buffer[..n]);
                if String::from_utf8_lossy(&received).contains("hello") {
                    break;
                }
            }
        })
        .await
        .expect("proxy buffered the response instead of streaming the first chunk");

        proxy_task.abort();
        backend_task.abort();
    }

    #[tokio::test]
    async fn inbound_proxy_owned_headers_do_not_trigger_loop_detection() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            http_port: 0,
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let backend_port = backend.local_addr().unwrap().port();
        let backend_task = tokio::spawn(async move {
            let (mut stream, _) = backend.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 256];
            loop {
                let n = stream.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text.contains("x-jig-proxy-hops: 3\r\n"));
            assert!(
                !request_text
                    .to_ascii_lowercase()
                    .contains("x-jig-proxy-pid")
            );
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
        let proxy_port = wait_for_http_port(&store).await;
        store
            .add_route(Route {
                hostname: "spoof.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: backend_port,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        client
            .write_all(
                b"GET / HTTP/1.1\r\nHost: spoof.demo.localhost\r\nX-Jig-Proxy-Hops: 2\r\nX-Jig-Proxy-Pid: 9999\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut received = Vec::new();
        timeout(Duration::from_secs(2), client.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8_lossy(&received);

        assert!(response.contains("204 No Content"));

        backend_task.await.unwrap();
        proxy_task.abort();
    }

    #[tokio::test]
    async fn malformed_proxy_hop_header_returns_bad_request() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            http_port: 0,
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
        let proxy_port = wait_for_http_port(&store).await;
        store
            .add_route(Route {
                hostname: "bad-hop.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 9,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        client
            .write_all(
                b"GET / HTTP/1.1\r\nHost: bad-hop.demo.localhost\r\nX-Jig-Proxy-Hops: 1\r\nX-Jig-Proxy-Hops: 2\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut received = Vec::new();
        timeout(Duration::from_secs(2), client.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8_lossy(&received);

        assert!(response.contains("400 Bad Request"));
        assert!(response.contains("Invalid Jig proxy headers"));
        proxy_task.abort();
    }

    #[tokio::test]
    async fn connect_method_is_rejected_before_proxying() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            http_port: 0,
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
        let proxy_port = wait_for_http_port(&store).await;

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        client
            .write_all(
                b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut received = Vec::new();
        timeout(Duration::from_secs(2), client.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8_lossy(&received);

        assert!(response.contains("405 Method Not Allowed"));
        assert!(response.contains("CONNECT requests are not supported"));
        proxy_task.abort();
    }

    #[tokio::test]
    async fn bind_failure_preserves_existing_runtime_files() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            http_port: 0,
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        store.write_pid(4242).unwrap();
        store.write_http_port(31337).unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let occupied = listener.local_addr().unwrap().port();
        let settings = ProxySettings {
            http_port: occupied,
            ..settings
        };

        let error = run_async(settings, PathBuf::from("test-jig"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("Failed to bind") || error.contains("Address already in use"));
        assert_eq!(store.read_pid().unwrap(), Some(4242));
        assert_eq!(store.read_http_port().unwrap(), Some(31337));
    }

    #[tokio::test]
    async fn route_cache_invalidates_after_route_file_changes() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let cache = RouteCache::new(store.clone());

        assert!(cache.routes().await.unwrap().is_empty());

        store
            .add_route(Route {
                hostname: "cache.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let routes = cache.routes().await.unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].hostname, "cache.demo.localhost");
    }

    #[tokio::test]
    async fn backend_body_rejects_content_length_with_chunked_encoding() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accept = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            stream
        });
        let mut client = TcpStream::connect(addr).await.unwrap();
        let _server = accept.await.unwrap();
        let headers = vec![
            (CONTENT_LENGTH, HeaderValue::from_static("0")),
            (
                HeaderName::from_static("transfer-encoding"),
                HeaderValue::from_static("chunked"),
            ),
        ];

        let error = complete_backend_body(&mut client, &headers, Bytes::new())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("both Content-Length and Transfer-Encoding"));
    }

    #[tokio::test]
    async fn websocket_non_switching_response_drains_backend_body() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            http_port: 0,
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let backend_port = backend.local_addr().unwrap().port();
        let backend_task = tokio::spawn(async move {
            let (mut stream, _) = backend.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 11\r\nconnection: close, x-secret\r\nx-secret: leak\r\nx-jig-proxy-pid: spoofed\r\n\r\nhello",
                )
                .await
                .unwrap();
            sleep(Duration::from_millis(100)).await;
            stream.write_all(b" world").await.unwrap();
        });

        let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
        let proxy_port = wait_for_http_port(&store).await;
        store
            .add_route(Route {
                hostname: "ws.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: backend_port,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        client
            .write_all(
                b"GET /socket HTTP/1.1\r\nHost: ws.demo.localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: x\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .await
            .unwrap();
        let mut received = Vec::new();
        timeout(Duration::from_secs(2), async {
            let mut buffer = [0u8; 1024];
            loop {
                let n = client.read(&mut buffer).await.unwrap();
                assert_ne!(n, 0, "proxy closed before returning the backend body");
                received.extend_from_slice(&buffer[..n]);
                if String::from_utf8_lossy(&received).contains("hello world") {
                    break;
                }
            }
        })
        .await
        .expect("proxy did not drain the backend non-101 WebSocket response");
        let text = String::from_utf8(received).unwrap();

        assert!(text.contains("401 Unauthorized"));
        assert!(text.contains("hello world"));
        assert!(!text.to_ascii_lowercase().contains("x-secret"));
        assert!(!text.to_ascii_lowercase().contains("x-jig-proxy-pid"));

        proxy_task.abort();
        backend_task.abort();
    }

    #[tokio::test]
    async fn websocket_switching_protocols_tunnels_bytes() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            http_port: 0,
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let backend_port = backend.local_addr().unwrap().port();
        let backend_task = tokio::spawn(async move {
            let (mut stream, _) = backend.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 256];
            loop {
                let n = stream.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text.starts_with("GET /socket?x=%0AInjected:%20yes HTTP/1.1\r\n"));
            assert!(!request_text.contains("\r\nInjected:"));
            assert!(request_text.contains("x-jig-proxy-hops: 1\r\n"));
            assert!(request_text.contains("via: 1.1 jig\r\n"));
            assert!(!request_text.to_ascii_lowercase().contains("content-length"));
            assert!(
                !request_text
                    .to_ascii_lowercase()
                    .contains("proxy-authorization")
            );
            assert!(!request_text.to_ascii_lowercase().contains("x-secret"));
            assert!(
                !request_text
                    .to_ascii_lowercase()
                    .contains("x-forwarded-port")
            );
            assert!(!request_text.to_ascii_lowercase().contains("x-real-ip"));
            assert!(
                !request_text
                    .to_ascii_lowercase()
                    .contains("x-jig-proxy-pid")
            );
            let accept = websocket_accept_for_key("x");
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade, X-Secret\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {accept}\r\nX-Secret: leak\r\nX-Jig-Proxy-Pid: spoofed\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut ping = [0u8; 4];
            stream.read_exact(&mut ping).await.unwrap();
            assert_eq!(&ping, b"ping");
            stream.write_all(b"pong").await.unwrap();
        });

        let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
        let proxy_port = wait_for_http_port(&store).await;
        store
            .add_route(Route {
                hostname: "ws-ok.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: backend_port,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        client
            .write_all(
                b"GET /socket?x=%0AInjected:%20yes HTTP/1.1\r\nHost: ws-ok.demo.localhost\r\nConnection: Upgrade, X-Secret\r\nUpgrade: websocket\r\nSec-WebSocket-Key: x\r\nSec-WebSocket-Version: 13\r\nContent-Length: 0\r\nProxy-Authorization: Basic bad\r\nX-Secret: leak\r\nX-Forwarded-Port: 443\r\nX-Real-IP: 203.0.113.1\r\nX-Jig-Proxy-Pid: spoofed\r\n\r\n",
            )
            .await
            .unwrap();
        let mut received = Vec::new();
        timeout(Duration::from_secs(2), async {
            let mut buffer = [0u8; 256];
            loop {
                let n = client.read(&mut buffer).await.unwrap();
                assert_ne!(n, 0, "proxy closed before 101 response");
                received.extend_from_slice(&buffer[..n]);
                if received.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
        })
        .await
        .expect("proxy did not return the 101 response");
        let response_text = String::from_utf8_lossy(&received);
        assert!(response_text.contains("101 Switching Protocols"));
        assert!(!response_text.to_ascii_lowercase().contains("x-secret"));
        assert!(
            !response_text
                .to_ascii_lowercase()
                .contains("x-jig-proxy-pid")
        );

        client.write_all(b"ping").await.unwrap();
        let mut pong = [0u8; 4];
        timeout(Duration::from_secs(2), client.read_exact(&mut pong))
            .await
            .expect("proxy did not tunnel backend bytes")
            .unwrap();
        assert_eq!(&pong, b"pong");

        proxy_task.abort();
        backend_task.abort();
    }

    async fn wait_for_http_port(store: &StateStore) -> u16 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(port) = store.read_http_port().unwrap() {
                return port;
            }
            assert!(Instant::now() < deadline, "proxy did not start listening");
            sleep(Duration::from_millis(25)).await;
        }
    }
}
