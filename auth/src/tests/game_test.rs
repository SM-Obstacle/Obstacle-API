use crate::{
    game::{self, TooManyRequests},
    init,
};

async fn make_n_requests(n: usize) -> Result<(), TooManyRequests> {
    for _ in 0..n {
        game::request_auth().await?;
    }
    Ok(())
}

#[tokio::test]
async fn handle_20_auth_requests() {
    init();
    assert!(make_n_requests(20).await.is_ok());
}

#[tokio::test]
async fn too_many_auth_requests() {
    init();
    assert!(make_n_requests(100).await.is_err());
}
