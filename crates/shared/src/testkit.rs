use crate::connect::LaserFactory;
use crate::lifecycle::eventually;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::process::{Child, Command};

const USERNAME: &str = "iggy";
const PASSWORD: &str = "laser";
const READY_TIMEOUT: Duration = Duration::from_secs(90);

/// A private iggy-server and plane on loopback ephemeral ports, stopped when dropped.
pub struct TestStack {
    _data: TempDir,
    _iggy: Child,
    _plane: Child,
    tcp: u16,
}

impl TestStack {
    pub async fn start() -> Self {
        let data = TempDir::new().expect("a temp dir for the test stack");
        let tcp = free_port();
        let http = free_port();
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
            .env("LD_PLANE_QUERY_SOCKET", &socket)
            .env("LD_PLANE_DB_PATH", data.path().join("index.db"))
            .env("LD_PLANE_HEALTH_ADDR", format!("127.0.0.1:{}", free_port()))
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
        eventually(READY_TIMEOUT, || {
            let factory = factory.clone();
            async move {
                let Ok(laser) = factory.connect("frostline-ready").await else {
                    return false;
                };
                let capabilities = laser.refresh_capabilities().await;
                capabilities.filters.native && capabilities.filters.catalog
            }
        })
        .await
        .expect("the test stack serves filters and the catalog");
    }
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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("an ephemeral loopback port")
        .port()
}
