use crate::connect::LaserFactory;
use crate::lifecycle::eventually;
use serde::Deserialize;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::process::{Child, Command};

const USERNAME: &str = "iggy";
const PASSWORD: &str = "laser";
const READY_TIMEOUT: Duration = Duration::from_secs(90);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// A private iggy-server and plane on loopback ephemeral ports, stopped when dropped.
pub struct TestStack {
    _data: TempDir,
    _iggy: Child,
    _plane: Child,
    tcp: u16,
    health: u16,
}

impl TestStack {
    pub async fn start() -> Self {
        let data = TempDir::new().expect("a temp dir for the test stack");
        let tcp = free_port();
        let http = free_port();
        let health = free_port();
        let socket = data.path().join("plane.sock");
        let iggy = Command::new(resolve("iggy-server"))
            .env("IGGY_ROOT_USERNAME", USERNAME)
            .env("IGGY_ROOT_PASSWORD", PASSWORD)
            .env("IGGY_PATH", data.path().join("iggy"))
            .env("IGGY_TCP_ADDRESS", format!("127.0.0.1:{tcp}"))
            .env("IGGY_HTTP_ADDRESS", format!("127.0.0.1:{http}"))
            .env("IGGY_QUIC_ENABLED", "false")
            .env("IGGY_WEBSOCKET_ENABLED", "false")
            .env("IGGY_PLANE_ENABLED", "true")
            .env("IGGY_PLANE_SOCKET_PATH", &socket)
            .env("IGGY_PLANE_REQUEST_TIMEOUT", "15s")
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("iggy-server starts");
        let plane = Command::new(resolve("plane"))
            .env("LD_PLANE_IGGY_URL", format!("tcp://127.0.0.1:{tcp}"))
            .env("LD_PLANE_IGGY_HTTP_URL", format!("http://127.0.0.1:{http}"))
            .env("LD_PLANE_IGGY_USERNAME", USERNAME)
            .env("LD_PLANE_IGGY_PASSWORD", PASSWORD)
            .env("LD_PLANE_IGGY_TLS_DISABLED", "true")
            .env("LD_PLANE_IGGY_PAT_FILE", "/nonexistent/plane.pat")
            .env("LD_PLANE_CLUSTER_KIND", "standalone")
            .env("LD_PLANE_QUERY_SOCKET", &socket)
            .env("LD_PLANE_DB_PATH", data.path().join("index.db"))
            .env("LD_PLANE_HEALTH_ADDR", format!("127.0.0.1:{health}"))
            .env("LD_PLANE_INTERNAL_API_TOKEN", "frostline-test")
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("plane starts");
        let stack = Self {
            _data: data,
            _iggy: iggy,
            _plane: plane,
            tcp,
            health,
        };
        stack.wait_until_ready().await;
        stack
    }

    pub fn connection_string(&self) -> String {
        self.connection_string_as(USERNAME, PASSWORD)
    }

    pub fn connection_string_as(&self, username: &str, password: &str) -> String {
        format!("iggy://{username}:{password}@127.0.0.1:{}", self.tcp)
    }

    pub fn factory(&self) -> LaserFactory {
        LaserFactory::from_connection_string(self.connection_string())
    }

    /// A factory that signs in as another user, for permission tests.
    pub fn factory_as(&self, username: &str, password: &str) -> LaserFactory {
        LaserFactory::from_connection_string(self.connection_string_as(username, password))
    }

    async fn wait_until_ready(&self) {
        let factory = self.factory();
        let health = self.health;
        eventually(READY_TIMEOUT, || {
            let factory = factory.clone();
            async move {
                let Ok(laser) = factory.connect("frostline-ready").await else {
                    return false;
                };
                let capabilities = laser.refresh_capabilities().await;
                capabilities.filters.native
                    && capabilities.filters.catalog
                    && tokio::task::spawn_blocking(move || plane_ready(health))
                        .await
                        .unwrap_or(false)
            }
        })
        .await
        .expect("the test stack serves filters and the catalog");
    }
}

#[derive(Deserialize)]
struct PlaneReadiness {
    ready: bool,
}

fn resolve(binary: &str) -> PathBuf {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/resolve-runtime");
    let output = std::process::Command::new(script)
        .arg(binary)
        .output()
        .expect("the runtime resolver runs");
    assert!(
        output.status.success(),
        "the runtime resolver failed for {binary}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    PathBuf::from(
        String::from_utf8(output.stdout)
            .expect("a UTF-8 path")
            .trim(),
    )
}

// The plane advertises filters before its control replay ends, and schema registration waits for that replay.
fn plane_ready(port: u16) -> bool {
    let probe = || -> std::io::Result<String> {
        let mut stream = TcpStream::connect(("127.0.0.1", port))?;
        stream.set_read_timeout(Some(PROBE_TIMEOUT))?;
        stream.write_all(b"GET /ready HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    };
    probe().ok().is_some_and(|response| {
        response
            .split_once("\r\n\r\n")
            .and_then(|(_, body)| serde_json::from_str::<PlaneReadiness>(body).ok())
            .is_some_and(|readiness| readiness.ready)
    })
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("an ephemeral loopback port")
        .port()
}
