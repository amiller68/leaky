use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;

// Test configuration
#[derive(Clone)]
struct TestConfig {
    ipfs_scheme: String,
    ipfs_host: String,
    ipfs_port: u16,
    proxy_scheme: String,
    proxy_host: String,
    proxy_port: u16,
    fixtures_dir: PathBuf,
    scratch_dir: PathBuf,
    pems_dir: PathBuf,
    data_dir: PathBuf,
    pem_rel_path: PathBuf,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self::new(
            PathBuf::from("./tests/scratch"),
            PathBuf::from("./tests/fixtures"),
        )
    }
}

impl TestConfig {
    fn new(scratch_dir: PathBuf, fixtures_dir: PathBuf) -> Self {
        // Clone scratch_dir before first use
        let scratch = scratch_dir.clone();
        Self {
            ipfs_scheme: "http".to_string(),
            ipfs_host: "localhost".to_string(),
            ipfs_port: 5001,
            proxy_scheme: "http".to_string(),
            proxy_host: "localhost".to_string(),
            proxy_port: 3001,
            fixtures_dir: fixtures_dir.clone(),
            scratch_dir: scratch.clone(),
            pems_dir: scratch.join("pems"),
            data_dir: scratch.join("data"),
            pem_rel_path: PathBuf::from("../pems"),
        }
    }
}

// Test context manages server lifecycle and cleanup
struct TestContext {
    config: TestConfig,
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl TestContext {
    async fn new() -> Self {
        let config = TestConfig::default();
        Self::setup_clean_environment(&config).await;
        let mut ctx = Self {
            config,
            server_handle: None,
        };
        ctx.start_server().await;
        ctx
    }

    async fn setup_clean_environment(config: &TestConfig) {
        // Clean and recreate test directories
        fs::remove_dir_all(&config.scratch_dir).ok();

        fs::create_dir_all(&config.scratch_dir).expect("Failed to create scratch dir");
        fs::create_dir_all(&config.pems_dir).expect("Failed to create pems dir");
        fs::create_dir_all(&config.data_dir).expect("Failed to create data dir");

        // Copy fixtures
        fs_extra::dir::copy(
            &config.fixtures_dir,
            &config.data_dir,
            &fs_extra::dir::CopyOptions::new()
                .content_only(true)
                .overwrite(true),
        )
        .expect("Failed to copy fixtures");

        println!("-> environment setup complete");
    }

    async fn start_server(&mut self) {
        let server_db_url = "sqlite::memory:".to_string();

        let ipfs_scheme = self.config.ipfs_scheme.clone();
        let ipfs_host = self.config.ipfs_host.clone();
        let ipfs_port = self.config.ipfs_port;
        let proxy_scheme = self.config.proxy_scheme.clone();
        let proxy_host = self.config.proxy_host.clone();
        let proxy_port = self.config.proxy_port;

        println!("-> starting server with in-memory database");

        let workspace_root = std::env::current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();

        // Spawn server process without waiting for it
        let child = std::process::Command::new("cargo")
            .current_dir(&workspace_root)
            .args(["run", "--bin", "leaky-server"])
            .env(
                "IPFS_RPC_URL",
                format!("{}://{}:{}", ipfs_scheme, ipfs_host, ipfs_port),
            )
            .env(
                "GET_CONTENT_FORWARDING_URL",
                format!("{}://{}:{}", proxy_scheme, proxy_host, proxy_port),
            )
            .env("SQLITE_DATABASE_URL", server_db_url)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("Failed to start server");

        // Store the child process handle instead of waiting on it
        self.server_handle = Some(tokio::task::spawn(async move {
            let _ = child; // Keep child process alive
        }));

        self.wait_for_server().await;
        println!("-> server started");
    }

    async fn wait_for_server(&self) {
        let client = reqwest::Client::new();
        let health_url = format!(
            "{}://{}:{}/_status/healthz",
            self.config.proxy_scheme, self.config.proxy_host, self.config.proxy_port
        );

        for _i in 0..5 {
            match client.get(&health_url).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        sleep(Duration::from_secs(1)).await;
                        return;
                    }
                }
                _ => {}
            }
            sleep(Duration::from_secs(3)).await;
        }
        panic!("Server failed to start after 60 seconds");
    }

    async fn cleanup(self) {
        if let Some(handle) = self.server_handle {
            // Kill the server process
            handle.abort();

            // On Unix systems, we might want to ensure the process is killed
            let _ = std::process::Command::new("pkill")
                .args(["-f", "leaky-server"])
                .output();
        }

        // Clean up all test directories
        fs::remove_dir_all(&self.config.scratch_dir).ok();
    }

    fn leaky(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        let assert = Command::cargo_bin("leaky-cli")
            .unwrap()
            .current_dir(&self.config.data_dir)
            .args(args)
            .assert();

        // Print output
        let output = assert.get_output();
        if !output.stderr.is_empty() {
            println!("stderr:\n{}", String::from_utf8_lossy(&output.stderr));
        }

        assert
    }

    async fn init(&self) -> assert_cmd::assert::Assert {
        self.leaky(&[
            "init",
            "--remote",
            &format!(
                "{}://{}:{}",
                self.config.proxy_scheme, self.config.proxy_host, self.config.proxy_port
            ),
            "--key-path",
            self.config.pem_rel_path.to_str().unwrap(),
        ])
    }

    async fn data_clean(&self) {
        // Read the directory entries
        if let Ok(entries) = fs::read_dir(&self.config.data_dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                // Skip hidden files (those starting with .)
                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| !n.starts_with('.'))
                    .unwrap_or(false)
                {
                    if path.is_file() {
                        fs::remove_file(path).ok();
                    } else if path.is_dir() {
                        fs::remove_dir_all(path).ok();
                    }
                }
            }
        }
    }

    async fn assert_file_exists(&self, path: &str) {
        assert!(fs::metadata(&self.config.data_dir.join(path)).is_ok());
    }

    async fn add(&self) -> assert_cmd::assert::Assert {
        self.leaky(&["add"])
    }

    async fn push(&self) -> assert_cmd::assert::Assert {
        self.leaky(&["push"])
    }

    async fn pull(&self) -> assert_cmd::assert::Assert {
        self.leaky(&["pull"])
    }

    async fn get_content(&self, path: &str) -> reqwest::Response {
        let client = reqwest::Client::new();
        client
            .get(&format!(
                "{}://{}:{}/{}",
                self.config.proxy_scheme, self.config.proxy_host, self.config.proxy_port, path
            ))
            .send()
            .await
            .expect(&format!("Failed to get {}", path))
    }
}

impl Clone for TestContext {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            server_handle: None,
        }
    }
}

// Test helper to run a test with fresh context
async fn run_test<F, Fut>(test_fn: F)
where
    F: FnOnce(TestContext) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let ctx = TestContext::new().await;
    test_fn(ctx.clone()).await;
    ctx.cleanup().await;
}

#[tokio::test]
async fn test_basic_workflow() {
    run_test(|ctx| async move {
        // Initialize and verify success
        ctx.init().await.success();

        // Add content
        ctx.add().await.success();

        // Push content
        ctx.push().await.success();

        // Verify specific asset is accessible
        let resp = ctx.get_content("writing/assets/ocean.jpg").await;
        assert!(resp.status().is_success());

        // Clean data
        ctx.data_clean().await;

        // Pull content
        ctx.pull().await.success();

        // Verify specific asset is accessible
        ctx.assert_file_exists("writing/assets/ocean.jpg").await;
    })
    .await;
}

// #[tokio::test]
// async fn test_error_cases() {
//     run_test(|ctx| async move {
//         // Test push before init (should fail)
//         ctx.push().await
//             .failure()
//             .stderr(predicates::str::contains("MissingDataPath"));

//         // Test invalid remote URL
//         ctx.leaky(&[
//             "init",
//             "--remote",
//             "invalid-url",
//             "--key-path",
//             PEM_REL_PATH,
//         ])
//         .failure();

//         // Initialize properly
//         ctx.init().await.success();

//         // Try to pull non-existent content
//         ctx.pull().await
//             .failure()
//             .stderr(predicates::str::contains("error"));
//     }).await;
// }
