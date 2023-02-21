//! Tests for the game_api crate.
//! 
//! These tests are run with:
//! - `cargo run --bin game_api` on a separate terminal
//! - `cargo test --bin game_api`
//! 
//! To test with a valid token, you must run the `game_api` binary, then run:
//! `curl -X POST -H "Content-Type: application/json" -d '{"state": "{STATE}", "time": 23, "respawn_count": 23, "player_login": "ahmad3", "map_game_id": "oui", "flags": 0}' http://localhost:3001/new_player_finished`
//! And go beside to `https://prod.live.maniaplanet.com/login/oauth/authorize?response_type=token&client_id={CLIENT_ID}&redirect_uri=http://localhost:{PORT}&state={STATE}`
//! After you log in, the new token will be printed in the terminal running the `curl` command.

use std::sync::Arc;

use reqwest::{Client, Response, StatusCode};

#[tokio::test]
async fn incomplete_player_finished() {
    let c = Client::new();

    let res = c
        .post("http://localhost:3001/player_finished")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = c
        .post("http://localhost:3001/player_finished")
        .body("{\"login\": \"test\"}")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = c
        .post("http://localhost:3001/player_finished")
        .body(
            r#"{"time": 23, "respawnCount": 23, "playerId": "ahmad3", "mapId": "oui", "flags": 0}"#,
        )
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unauthorized_player_finished() {
    let c = Client::new();

    let res = c
        .post("http://localhost:3001/player_finished")
        .header("Authorization", "hop")
        .header("Content-Type", "application/json")
        .body(
            r#"{"time": 23, "respawnCount": 23, "playerId": "ahmad3", "mapId": "oui", "flags": 0}"#,
        )
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn incomplete_gen_new_token() {
    let c = Client::new();

    let res = c
        .post("http://localhost:3001/gen_new_token")
        .header("Content-Type", "application/json")
        .body(
            r#"{"token_type": "oui", "expires_in": "yep", "access_token": "hey", "state": "yep"}"#,
        )
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(res.text().await.unwrap(), "missing /new_player_finished request");
}

async fn new_player_finished_req(c: &Client, state: &str) -> Response {
    c.post("http://localhost:3001/new_player_finished")
        .header("Content-Type", "application/json")
        .body(
            format!(
                r#"{{"state": "{}", "time": 23, "respawn_count": 23, "player_login": "ahmad3", "map_game_id": "oui", "flags": 0}}"#,
                state
            )
        )
        .send()
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "takes 5mn"]
async fn timeout_new_player_finished() {
    let c = Client::new();
    assert_eq!(
        new_player_finished_req(&c, "oui").await.status(),
        StatusCode::REQUEST_TIMEOUT
    );
}

#[tokio::test]
async fn unauthorized_gen_new_token() {
    let c = Arc::new(Client::new());
    let finish = c.clone();
    let finish = tokio::spawn(async move {
        let finish = new_player_finished_req(&finish, "oui").await;
        assert_eq!(finish.status(), StatusCode::BAD_REQUEST);
        assert_eq!(finish.text().await.unwrap(), "invalid MP access token");
    });
    // We suppose that the /new_player_finished request will be processed before the /gen_new_token
    // request, because the player will take some time to log in, so we wait a bit.
    // Otherwise, the `incomplete_gen_new_token` test checks if the /gen_new_token is called
    // without a /new_player_finished request.
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    let token = c
        .post("http://localhost:3001/gen_new_token")
        .header("Content-Type", "application/json")
        .body(
            r#"{"token_type": "oui", "expires_in": "yep", "access_token": "hey", "state": "oui"}"#,
        )
        .send()
        .await
        .unwrap();
    assert_eq!(token.status(), StatusCode::BAD_REQUEST);
    assert_eq!(token.text().await.unwrap(), "invalid MP access token");
    assert!(finish.await.is_ok());
}

#[tokio::test]
async fn state_already_received() {
    let c = Arc::new(Client::new());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let first_req = c.clone();
    let first_req = tokio::spawn(async move {
        tokio::select! {
            _ = rx => {}
            _ = new_player_finished_req(&first_req, "hey") => {
                panic!("first request should have been canceled");
            }
        }
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    let second_req = new_player_finished_req(&c, "hey").await;
    assert_eq!(second_req.status(), StatusCode::BAD_REQUEST);
    assert!(second_req.text().await.unwrap().contains("state already received at "));
    tx.send(()).unwrap();
    assert!(first_req.await.is_ok());
}
