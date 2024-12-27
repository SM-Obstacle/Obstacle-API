use std::future::Future;

use tokio::sync::oneshot;

use crate::{browser, game, init, Code, StateId};

mod browser_test;
mod game_test;
mod invalid_state;
mod state_not_received;

macro_rules! assert_common_err {
    ($e:expr, $other_err:path, $var:pat) => {
        assert!(matches!($e, Err($other_err($var))));
    };
}

pub(in crate::tests) use assert_common_err;

async fn with_endpoints<G, FutG, B, FutB>(
    game_endpoint: G,
    browser_endpoint: B,
) -> anyhow::Result<()>
where
    G: FnOnce(oneshot::Sender<StateId>, oneshot::Receiver<Code>) -> FutG,
    B: FnOnce(oneshot::Sender<Code>, oneshot::Receiver<StateId>) -> FutB,
    FutG: Future<Output = anyhow::Result<()>> + Send + 'static,
    FutB: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let (state_tx, state_rx) = oneshot::channel();
    let (code_tx, code_rx) = oneshot::channel();

    let (g_res, b_res) = tokio::try_join!(
        tokio::task::spawn(game_endpoint(state_tx, code_rx)),
        tokio::task::spawn(browser_endpoint(code_tx, state_rx))
    )?;

    g_res.and(b_res)
}

#[allow(dead_code)]
async fn sleep() {
    tokio::time::sleep(core::time::Duration::from_millis(500)).await;
}

#[tokio::test]
async fn basic() -> anyhow::Result<()> {
    init();

    with_endpoints(
        |state_tx, code_rx| async {
            let state_id = game::request_auth().await?;
            state_tx.send(state_id).expect("cannot send state id");
            game::wait_for_oauth(state_id, |access_token| async {
                anyhow::Ok(access_token.into_unsecure() == "hello")
            })
            .await?;
            let code = code_rx.await?;
            game::validate_code(state_id, code).await?;
            Ok(())
        },
        |code_tx, state_rx| async {
            let state_id = state_rx.await?;
            let code = browser::notify_ingame(state_id, "hello".into()).await?;
            code_tx.send(code).expect("cannot send code");
            browser::wait_code_validation(state_id).await?;
            Ok(())
        },
    )
    .await
}
