use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;
use std::time::Duration;
use fs_extra::dir;

mod test_config;

use test_config::TestConfig;

// Helper struct to manage test state
struct TestContext {
    config: TestConfig,
}

impl TestContext {
    fn new() -> Self {
        let config = TestConfig::default();

        // just a throwaway dir for the pems
        let temp_dir = TempDir::new().unwrap();
        let pems_dir = temp_dir.path().join("pems");
        fs::create_dir_all(&pems_dir).expect("Failed to create pems directory");
        
        // Copy example files to temp directory
        fs::create_dir_all(&config.scratch_dir).expect("Failed to create scratch directory");
        
        // Copy all contents from fixtures to scratch directory
        let copy_options = dir::CopyOptions::new()
            .content_only(true)  // Only copy contents, not the directory itself
            .overwrite(true);    // Overwrite existing files if any
            
        fs_extra::dir::copy(
            &config.fixtures_dir, 
            &config.scratch_dir, 
            &copy_options
        ).expect("Failed to copy example files");

        Self {
            config,
        }
    }

    fn run_leaky(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        Command::cargo_bin("leaky-cli")
            .unwrap()
            .current_dir(&self.config.scratch_dir)
            .args(args)
            .assert()
    }

    // Helper to wait for services to be ready
    async fn wait_for_services(&self) -> bool {
        let client = reqwest::Client::new();
        let api_health_url = format!("http://localhost:{}/_status/healthz", self.config.server_port);
        
        for _ in 0..30 {
            let api_ok = client.get(&api_health_url).send().await
                .map(|r| r.status().is_success())
                .unwrap_or(false);

            println!("api_ok: {}", api_ok);

            if api_ok {
                // Give services an extra second to stabilize
                tokio::time::sleep(Duration::from_secs(1)).await;
                return true;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        false
    }
}

#[tokio::test]
async fn test_basic_workflow() {
    let ctx = TestContext::new();

    // Wait for services to be ready
    assert!(ctx.wait_for_services().await, "Services failed to start");

    // Test init
    ctx.run_leaky(&[
        "init",
        "--remote",
        "http://localhost:3001",
        "--key-path",
        "./pems",
    ])
    .success();

    // Test add
    ctx.run_leaky(&["add"])
        .success();

    // Test push
    ctx.run_leaky(&["push"])
        .success();

    println!("push complete");

    // Give the server a moment to process
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify specific asset is accessible
    let client = reqwest::Client::new();
    let resp = client.get("http://localhost:3001/writing/assets/ocean.jpg")
        .send()
        .await
        .expect("Failed to get ocean.jpg");
    assert!(resp.status().is_success(), "Server returned {} for ocean.jpg", resp.status());

    // Test pull
    ctx.run_leaky(&["pull"])
        .success();

    // // Give the server a moment to process
    // tokio::time::sleep(Duration::from_secs(2)).await;

    // // Verify content is accessible via nginx
    // let client = reqwest::Client::new();
    // let resp = client.get("http://localhost:3001/")
    //     .send()
    //     .await
    //     .expect("Failed to get content");
    // assert!(resp.status().is_success(), "Server returned {}", resp.status());

    // // Test pull
    // ctx.run_leaky(&["pull"])
    //     .success();

    // // Verify files match example
    // let example_tree = fs_extra::dir::get_dir_content(&ctx.example_dir)
    //     .expect("Failed to read example dir");
    
    // // Compare files (excluding .leaky and pems directories)
    // for path in example_tree.files.iter() {
    //     let path = PathBuf::from(path);
    //     let rel_path = path
    //         .strip_prefix(&ctx.example_dir)
    //         .unwrap();
    //     let test_path = ctx.temp_dir.path().join(rel_path);
        
    //     if !rel_path.starts_with(".leaky") && !rel_path.starts_with("pems") {
    //         assert!(
    //             test_path.exists(),
    //             "Missing file: {:?}", rel_path
    //         );
    //     }
    // }
}

// #[tokio::test]
// async fn test_error_cases() {
//     let ctx = TestContext::new();
//     assert!(ctx.wait_for_services().await, "Services failed to start");

//     // Test push before init (should fail with MissingDataPath)
//     ctx.run_leaky(&["push"])
//         .failure()
//         .stderr(predicate::str::contains("MissingDataPath"));

//     // Test invalid remote URL
//     ctx.run_leaky(&[
//         "init",
//         "--remote",
//         "invalid-url",
//         "--key-path",
//         "./pems",
//     ])
//     .failure();

//     // Initialize with valid URL but try to pull without pushing
//     ctx.run_leaky(&[
//         "init",
//         "--remote",
//         "http://localhost:3001",
//         "--key-path",
//         "./pems",
//     ])
//     .success();

//     // Try to pull non-existent content - should fail
//     ctx.run_leaky(&["pull"])
//         .failure()
//         .stderr(predicate::str::contains("error"));
// } 