mod context;

// use std::path::PathBuf;

use context::TestContext;

async fn run_test<F, Fut>(test_name: &str, test_fn: F)
where
    F: FnOnce(TestContext) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let ctx = TestContext::new(test_name).await;
    test_fn(ctx.clone()).await;
    ctx.cleanup().await;
} 

#[tokio::test]
async fn test_basic_workflow() {
    run_test("basic_workflow", |ctx| async move {
        // Initialize and verify success
        ctx.init().await.success();

        // Add content
        ctx.add().await.success();

        // Push content
        ctx.push().await.success();

        // Verify specific asset is accessible
        let resp = ctx.get_content("writing/assets/ocean.jpg").await;
        assert!(resp.status().is_success());
    })
    .await;
}

#[tokio::test]
async fn test_cannot_add_before_init() {
    run_test("cannot_add_before_init", |ctx| async move {
        // Test push before init (should fail)
        ctx.leaky(&["add"]).failure();
    })
    .await;
}

// TODO: fix this test
// #[tokio::test]
// async fn test_can_add_from_child_dir() {
//     run_test("can_add_from_child_dir", |mut ctx| async move {
//         println!("-> init");
//         ctx.init().await.success();
//         println!("-> cd");
//         ctx.cd(Some(PathBuf::from("writing")));
//         println!("-> add");
//         ctx.add().await.success();

//     })
//     .await;
// }