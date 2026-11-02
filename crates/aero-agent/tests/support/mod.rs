//! Shared browser-scenario harness (C07, implementation plan §9).
//!
//! `Fixture` owns a local fixture server and a sandboxed `BrowserRuntime`
//! (process, CDP connection, target registry). Cleanup is synchronous and
//! owned: dropping a fixture (including while a failed assertion unwinds)
//! stops the registry, closes the connection, tears the browser sandbox down
//! with pidfd verification, and aborts the server — no async work runs inside
//! `Drop`. `shutdown` does the same and reports failures.

pub mod browser;
pub mod fixture_server;

use std::net::SocketAddr;
use std::time::Duration;

use aero_cdp::runtime::BrowserRuntime;
use aero_cdp::{CdpConnection, CdpError, CdpEvent, EventStream};

pub use browser::BrowserHarness;
pub use fixture_server::FixtureServer;

/// Default deadline for one CDP call in scenarios.
pub const CALL: Duration = Duration::from_secs(10);

/// Scenarios share the host process table and a browser each; run them one at
/// a time so process-level assertions see only their own sandbox.
pub async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    SERIAL.lock().await
}

/// Field order is drop order: browser runtime, then server.
pub struct Fixture {
    pub browser: BrowserHarness,
    pub server: FixtureServer,
}

impl Fixture {
    /// Start the server, then the browser runtime. On any error the parts
    /// already started are dropped, which tears them down.
    pub async fn start() -> Result<Self, String> {
        let server = FixtureServer::start()
            .await
            .map_err(|e| format!("fixture server: {e}"))?;
        let browser = BrowserHarness::launch().await?;
        Ok(Self { browser, server })
    }

    pub fn runtime(&self) -> &BrowserRuntime {
        self.browser.runtime()
    }

    /// The browser-level CDP connection (no session).
    pub fn cdp(&self) -> &CdpConnection {
        self.runtime().connection()
    }

    pub async fn shutdown(self) -> Result<(), String> {
        let Self { browser, server } = self;
        let browser = browser.shutdown().await;
        let server = server.shutdown().await;
        browser.and(server)
    }
}

/// Wait for the first event `accept` maps to a value. Missing events (lag) and
/// a closed stream are errors: the caller's view would be stale.
pub async fn wait_until<T>(
    events: &mut EventStream,
    timeout: Duration,
    mut accept: impl FnMut(&CdpEvent) -> Option<T>,
) -> Result<T, String> {
    let wait = async {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if let Some(value) = accept(&event) {
                        return Ok(value);
                    }
                }
                Err(CdpError::EventLag(missed)) => {
                    return Err(format!("missed {missed} events; resynchronize"));
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    };
    tokio::time::timeout(timeout, wait)
        .await
        .map_err(|_| format!("no matching event within {timeout:?}"))?
}

/// Poll `probe` every 100 ms until it yields a value or `timeout` passes.
/// For state with no reliable event; prefer [`wait_until`] when one exists.
pub async fn eventually<T, F, Fut>(timeout: Duration, mut probe: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(value) = probe().await {
            return Some(value);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// One raw HTTP/1.1 GET; returns the whole response text.
pub async fn http_get(address: SocketAddr, path: &str) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let exchange = async {
        let mut stream = tokio::net::TcpStream::connect(address).await?;
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&response).into_owned())
    };
    tokio::time::timeout(CALL, exchange)
        .await
        .map_err(|_| "HTTP request timed out".to_owned())?
        .map_err(|error| error.to_string())
}

/// True once connecting to `address` is refused (polled until `within`).
pub async fn refuses_connections(address: SocketAddr, within: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        if tokio::net::TcpStream::connect(address).await.is_err() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Host PIDs of processes running from the sandbox browser directory.
pub fn sandbox_processes() -> Vec<u32> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return found;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if let Ok(command) = std::fs::read(entry.path().join("cmdline"))
            && command.starts_with(b"/opt/aero/browser/")
        {
            found.push(pid);
        }
    }
    found
}
