use std::path::PathBuf;
use rand;
use assert_cmd::Command;
use reqwest;
use std::fs;
use std::process::Command as ProcessCommand;
use std::time::Duration;
use tokio::time::sleep;

pub struct TestContext {
    pub config: TestConfig,
    pub cd: Option<PathBuf>,
    pub server_handle: Option<tokio::task::JoinHandle<()>>,
    // pub nginx_handle: Option<std::process::Child>,
    pub container_id: Option<String>,
}

#[allow(unused)]
impl TestContext {
    pub async fn new(test_name: &str) -> Self {
        let config = TestConfig::new(test_name);
        Self::setup_clean_environment(&config).await;
        let mut ctx = Self {
            config,
            cd: None,
            server_handle: None,
            // nginx_handle: None,
            container_id: None,
        };
        ctx.start_server().await;
        ctx.start_nginx().await;
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
    }

    async fn start_server(&mut self) {
        let server_db_url = "sqlite::memory:".to_string();

        let ipfs_scheme = self.config.ipfs_scheme.clone();
        let ipfs_host = self.config.ipfs_host.clone();
        let ipfs_port = self.config.ipfs_port;
        let proxy_scheme = self.config.proxy_scheme.clone();
        let proxy_host = self.config.proxy_host.clone();
        let proxy_port = self.config.proxy_port;

        let workspace_root = std::env::current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();

        let child = ProcessCommand::new("cargo")
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
            .env("LISTEN_ADDR", format!("0.0.0.0:{}", self.config.server_port))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Failed to start server");

        self.server_handle = Some(tokio::task::spawn(async move {
            let _ = child;
        }));

        self.wait_for_server().await;
    }

    async fn start_nginx(&mut self) {
        // Write nginx config to temp file
        let config_path = self.config.scratch_dir.join("nginx.conf");
        fs::write(&config_path, self.config.generate_nginx_config())
            .expect("Failed to write nginx config");

        let output = ProcessCommand::new("docker")
            .args([
                "run",
                "--rm",  // Remove container when stopped
                "-d",    // Run in background
                "-v",    // Mount config file
                &format!("{}:/etc/nginx/nginx.conf:ro", config_path.display()),
                "-p",    // Map random port
                &format!("{}:80", self.config.proxy_port),
                "--network=host", // Use host network to connect to local services
                "nginx:alpine",   // Use lightweight nginx image
            ])
            .output()
            .expect("Failed to start nginx container");

        let container_id = String::from_utf8(output.stdout)
            .or_else(|_| String::from_utf8(output.stderr))
            .expect("Invalid container ID")
            .lines()
            .next()
            .expect("No container ID found")
            .trim()
            .to_string();

        self.container_id = Some(container_id);

        // Wait for nginx to be ready
        self.wait_for_nginx().await;
    }

    async fn wait_for_nginx(&self) {
        let client = reqwest::Client::new();
        let health_url = format!(
            "http://{}:{}/nginx-health",
            self.config.proxy_host,
            self.config.proxy_port
        );

        for _i in 0..5 {
            match client.get(&health_url).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return;
                    }
                }
                _ => {}
            }
            sleep(Duration::from_secs(1)).await;
        }
        panic!("Nginx failed to start after 5 seconds");
    }

    async fn wait_for_server(&self) {
        let client = reqwest::Client::new();
        let health_url = format!(
            "http://{}:{}/_status/healthz",
            self.config.proxy_host,
            self.config.server_port
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

    pub async fn cleanup(mut self) {
        // Stop nginx container
        if let Some(container_id) = self.container_id.take() {
            let _ = ProcessCommand::new("docker")
                .args(["stop", &container_id])
                .output();
        }

        if let Some(handle) = self.server_handle {
            handle.abort();
            let _ = ProcessCommand::new("pkill")
                .args(["-f", "leaky-server"])
                .output();
        }

        fs::remove_dir_all(&self.config.scratch_dir).ok();
    }

    pub fn cd(&mut self, path: Option<PathBuf>) {
        self.cd = path;
    }

    pub fn leaky(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        let current_dir = match self.cd.clone() {
            Some(path) => self.config.data_dir.join(path),
            None => self.config.data_dir.clone(),
        };
        let assert = Command::cargo_bin("leaky-cli")
            .unwrap()
            .current_dir(current_dir)
            .args(args)
            .assert();

        // let output = assert.get_output();
        // if !output.stdout.is_empty() {
        //     println!("stdout:\n{}", String::from_utf8_lossy(&output.stdout));
        // }
        // if !output.stderr.is_empty() {
        //     println!("stderr:\n{}", String::from_utf8_lossy(&output.stderr));
        // }

        assert
    }

    pub async fn init(&self) -> assert_cmd::assert::Assert {
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

    pub async fn add(&self) -> assert_cmd::assert::Assert {
        self.leaky(&["add"])
    }

    pub async fn push(&self) -> assert_cmd::assert::Assert {
        self.leaky(&["push"])
    }

    pub async fn pull(&self) -> assert_cmd::assert::Assert {
        self.leaky(&["pull"])
    }

    pub async fn get_content(&self, path: &str) -> reqwest::Response {
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

    pub async fn assert_file_exists(&self, path: &str) {
        assert!(fs::metadata(&self.config.data_dir.join(path)).is_ok());
    }

    pub async fn data_clean(&self) {
        if let Ok(entries) = fs::read_dir(&self.config.data_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
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
}

impl Clone for TestContext {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            cd: self.cd.clone(),
            server_handle: None,
            // nginx_handle: None,
            container_id: None,
        }
    }
} 

#[derive(Clone)]
pub struct TestConfig {
    pub ipfs_scheme: String,
    pub ipfs_host: String,
    pub ipfs_port: u16,
    pub proxy_scheme: String,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub server_port: u16,
    pub fixtures_dir: PathBuf,
    pub scratch_dir: PathBuf,
    pub pems_dir: PathBuf,
    pub data_dir: PathBuf,
    pub pem_rel_path: PathBuf,
}

impl TestConfig {
    pub fn new(test_name: &str) -> Self {
        // Get random ports between 3001-4000
        let proxy_port = 3001 + rand::random::<u16>() % 1000;
        let server_port = 4001 + rand::random::<u16>() % 1000;
        
        let scratch_dir = PathBuf::from("./tests/scratch").join(test_name);
        let fixtures_dir = PathBuf::from("./tests/fixtures");
        
        Self {
            ipfs_scheme: "http".to_string(),
            ipfs_host: "localhost".to_string(),
            ipfs_port: 5001,
            proxy_scheme: "http".to_string(),
            proxy_host: "localhost".to_string(),
            proxy_port,
            server_port,
            fixtures_dir: fixtures_dir.clone(),
            scratch_dir: scratch_dir.clone(),
            pems_dir: scratch_dir.join("pems"),
            data_dir: scratch_dir.join("data"),
            pem_rel_path: PathBuf::from("../pems"),
        }
    }

    pub fn generate_nginx_config(&self) -> String {
        format!(
            r#"
events {{
    worker_connections 1024;
}}

http {{
    server {{
        listen {};

        location = /nginx-health {{
            access_log off;
            add_header Content-Type text/plain;
            return 200 'nginx is responding\n';
        }}

        location ^~ /api/v0/ipfs/ {{
            proxy_pass http://localhost:5001/api/v0/;
            proxy_set_header Host $host;
        }}

        location ^~ /api/ {{
            proxy_pass http://localhost:{}/api/;
            proxy_set_header Host $host;
        }}

        location ^~ /_status/ {{
            proxy_pass http://localhost:{}/api/;
            proxy_set_header Host $host;
        }}

        location / {{
            proxy_pass http://localhost:{}/content/;
            proxy_set_header Host $host;
        }}
    }}
}}
            "#,
            self.proxy_port,
            self.server_port,
            self.server_port,
            self.server_port
        )
    }
} 