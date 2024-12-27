use crate::{init, CommonEndpointErr, StateError};

macro_rules! assert_invalid {
    ($e:expr, $other_err:path) => {
        $crate::tests::assert_common_err!(
            $e,
            $other_err,
            CommonEndpointErr::StateErr(StateError::InvalidAuthState)
        );
    };
}

mod game_test {
    use super::*;
    use crate::game::{self, CodeErr};

    #[tokio::test]
    async fn confirm_code() -> anyhow::Result<()> {
        init();
        let state_id = game::request_auth().await?;
        assert_invalid!(
            game::validate_code(state_id, "nothing to see here".into()).await,
            CodeErr::Other
        );
        Ok(())
    }
}

mod browser_test {
    use crate::{
        browser::{self, CodeErr},
        game,
    };

    use super::*;

    #[tokio::test]
    async fn wait_for_game_code() -> anyhow::Result<()> {
        init();
        let state_id = game::request_auth().await?;
        assert_invalid!(browser::wait_code_validation(state_id).await, CodeErr::Other);
        Ok(())
    }
}
