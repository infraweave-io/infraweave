//! Tests for the api_module logic.

mod test_deprecate_module {
    use std::sync::Arc;

    use env_defs::{ModuleResp, ModuleStackData};
    use tokio::test;

    use crate::interface::{GenericCloudHandler, TestCloudProvider};
    use crate::logic::deprecate_module;

    fn non_deprecated_module(version: &str) -> ModuleResp {
        let mut m = ModuleResp::default();
        m.version = version.to_string();
        m.deprecated = false;
        m.deprecated_message = None;
        m.stack_data = None;
        m
    }

    fn non_deprecated_stack_module(version: &str) -> ModuleResp {
        let mut m = non_deprecated_module(version);
        m.stack_data = Some(ModuleStackData { modules: vec![] });
        m
    }

    fn deprecated_module(version: &str) -> ModuleResp {
        let mut m = ModuleResp::default();
        m.version = version.to_string();
        m.deprecated = true;
        m.deprecated_message = Some("obsolete".to_string());
        m.stack_data = None;
        m
    }

    #[test]
    async fn err_when_module_version_not_found() {
        let mut mock = TestCloudProvider::new();
        mock.expect_get_module_version()
            .returning(|_m: &str, _t: &str, _v: &str| Ok(None));

        let handler = GenericCloudHandler::with_provider(Arc::new(mock), None);
        let result = deprecate_module(&handler, "my-mod", "stable", "1.0.0", None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    async fn err_when_already_deprecated() {
        let mut mock = TestCloudProvider::new();
        mock.expect_get_module_version()
            .returning(|_m: &str, _t: &str, _v: &str| Ok(Some(deprecated_module("1.0.0"))));

        let handler = GenericCloudHandler::with_provider(Arc::new(mock), None);
        let result = deprecate_module(&handler, "my-mod", "stable", "1.0.0", None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("already deprecated"));
    }

    #[test]
    async fn err_when_deprecating_latest_version() {
        let mut mock = TestCloudProvider::new();
        mock.expect_get_module_version()
            .returning(|_m: &str, _t: &str, _v: &str| Ok(Some(non_deprecated_module("1.0.0"))));
        mock.expect_get_latest_module_version()
            .returning(|_m: &str, _t: &str| Ok(Some(non_deprecated_module("1.0.0"))));

        let handler = GenericCloudHandler::with_provider(Arc::new(mock), None);
        let result = deprecate_module(&handler, "my-mod", "stable", "1.0.0", None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err
            .to_string()
            .contains("Cannot deprecate the latest version"));
    }

    #[test]
    async fn err_when_deprecating_latest_stack_version() {
        let mut mock = TestCloudProvider::new();
        mock.expect_get_module_version()
            .returning(|_m: &str, _t: &str, _v: &str| {
                Ok(Some(non_deprecated_stack_module("1.0.0")))
            });
        mock.expect_get_latest_stack_version()
            .returning(|_m: &str, _t: &str| Ok(Some(non_deprecated_stack_module("1.0.0"))));

        let handler = GenericCloudHandler::with_provider(Arc::new(mock), None);
        let result = deprecate_module(&handler, "my-stack", "stable", "1.0.0", None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err
            .to_string()
            .contains("Cannot deprecate the latest version"));
    }
}
