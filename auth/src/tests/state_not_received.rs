use crate::{init, CommonEndpointErr, StateError};

macro_rules! assert_not_received {
    ($e:expr, $other_err:path) => {
        $crate::tests::assert_common_err!(
            $e,
            $other_err,
            CommonEndpointErr::StateErr(StateError::StateNotReceived(_))
        );
    };
}

mod game_test {
    use crate::{
        game::{self, CodeErr, OAuthErr},
        StateId,
    };

    use super::*;

    #[tokio::test]
    async fn wait_for_browser_oauth() {
        init();

        assert_not_received!(
            game::wait_for_oauth(StateId::dummy(), |_| async { anyhow::Ok(false) }).await,
            OAuthErr::Other
        );
    }

    #[tokio::test]
    async fn confirm_code() {
        init();

        assert_not_received!(
            game::validate_code(StateId::dummy(), "nothing to see here".into()).await,
            CodeErr::Other
        );
    }
}

mod browser_test {
    use crate::{
        browser::{self, CodeErr, OAuthErr},
        game, StateId,
    };

    use super::*;

    #[tokio::test]
    async fn notify_game_oauth() {
        init();

        assert_not_received!(
            browser::notify_ingame(StateId::dummy(), "nothing to see here".into()).await,
            OAuthErr::Other
        );
    }

    #[tokio::test]
    async fn notify_game_oauth_after_auth_req() -> anyhow::Result<()> {
        init();
        let state_id = game::request_auth().await?;
        assert!(matches!(
            browser::notify_ingame(state_id, "nothing to see here".into()).await,
            Err(OAuthErr::Other(CommonEndpointErr::Timeout(_)))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn wait_for_game_code() {
        init();

        assert_not_received!(
            browser::wait_code_validation(StateId::dummy()).await,
            CodeErr::Other
        );
    }
}
