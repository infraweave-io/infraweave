#[cfg(test)]
mod test_validate_all {
    use crate::ModuleManifest;

    const VALID_MANIFEST: &str = r#"
        apiVersion: infraweave.io/v1
        kind: Module
        metadata:
            name: s3bucket
        spec:
            moduleName: S3Bucket
            version: 0.2.1
            providers: []
            reference: https://github.com/your-org/s3bucket
            description: "S3Bucket description here..."
    "#;

    fn parse(yaml: &str) -> ModuleManifest {
        serde_yaml::from_str(yaml).unwrap()
    }

    /// Asserts that `validate_all()` returns an error whose message contains `expected_in_error`.
    fn assert_validation_fails_with(manifest: &ModuleManifest, expected_in_error: &str) {
        let result = manifest.validate_all();
        let err = result.unwrap_err();
        assert!(
            err.contains(expected_in_error),
            "validate_all() should fail with error containing {expected_in_error:?}, but got: {err}"
        );
    }

    #[test]
    fn valid_manifest_passes() {
        let manifest = parse(VALID_MANIFEST);
        manifest
            .validate_all()
            .expect("valid manifest should pass validate_all");
    }

    /// Tests that fail due to `metadata.validate_name()` (metadata.name rules).
    mod metadata_name {
        use super::*;

        #[test]
        fn rejects_hyphen_in_metadata_name() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: s3-bucket
                spec:
                    moduleName: S3Bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(&manifest, "Module name");
            assert_validation_fails_with(&manifest, "lowercase characters and numbers");
        }

        #[test]
        fn rejects_underscore_in_metadata_name() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: s3_bucket
                spec:
                    moduleName: S3bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(&manifest, "Module name");
            assert_validation_fails_with(&manifest, "lowercase characters and numbers");
        }

        #[test]
        fn rejects_name_shorter_than_2_chars() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: a
                spec:
                    moduleName: Aa
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(&manifest, "Module name");
        }

        #[test]
        fn rejects_name_starting_with_uppercase() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: S3bucket
                spec:
                    moduleName: S3bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(&manifest, "Module name");
            assert_validation_fails_with(&manifest, "lowercase characters and numbers");
        }

        #[test]
        fn rejects_empty_metadata_name() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: ""
                spec:
                    moduleName: Ab
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/ab
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(&manifest, "Module name");
        }

        #[test]
        fn rejects_space_in_metadata_name() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: "s3 bucket"
                spec:
                    moduleName: S3Bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(&manifest, "Module name");
            assert_validation_fails_with(&manifest, "lowercase characters and numbers");
        }
    }

    /// Tests that fail due to `spec.validate_module_name()` (spec.moduleName rules).
    mod spec_module_name {
        use super::*;

        #[test]
        fn rejects_module_name_not_starting_with_uppercase() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: s3bucket
                spec:
                    moduleName: s3Bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(&manifest, "must start with an uppercase character");
        }

        #[test]
        fn rejects_hyphen_in_module_name() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: s3bucket
                spec:
                    moduleName: S3-Bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(&manifest, "must only contain alphanumeric characters");
        }

        #[test]
        fn rejects_underscore_in_module_name() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: s3bucket
                spec:
                    moduleName: S3_Bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(&manifest, "must only contain alphanumeric characters");
        }

        #[test]
        fn rejects_empty_module_name() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: ab
                spec:
                    moduleName: ""
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/ab
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            // Empty moduleName fails name consistency (metadata "ab" != "".to_lowercase())
            assert_validation_fails_with(
                &manifest,
                "must exactly match lowercase of the moduleName",
            );
        }
    }

    /// Tests that fail due to `validate_name_consistency()` (metadata.name must equal moduleName.to_lowercase()).
    mod name_consistency {
        use super::*;

        #[test]
        fn rejects_when_metadata_name_not_lowercase_of_module_name() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Module
                metadata:
                    name: bucket
                spec:
                    moduleName: S3Bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(
                &manifest,
                "must exactly match lowercase of the moduleName",
            );
        }
    }

    /// Tests that fail due to `validate_kind()` (kind must be "Module").
    mod kind {
        use super::*;

        #[test]
        fn rejects_invalid_kind() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: Manifest
                metadata:
                    name: s3bucket
                spec:
                    moduleName: S3Bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(
                &manifest,
                "The kind field in module.yaml must be 'Module'",
            );
            assert_validation_fails_with(&manifest, "Manifest");
        }

        #[test]
        fn rejects_empty_kind() {
            let yaml = r#"
                apiVersion: infraweave.io/v1
                kind: ""
                metadata:
                    name: s3bucket
                spec:
                    moduleName: S3Bucket
                    version: 0.2.1
                    providers: []
                    reference: https://github.com/your-org/s3bucket
                    description: "desc"
            "#;
            let manifest = parse(yaml);
            assert_validation_fails_with(
                &manifest,
                "The kind field in module.yaml must be 'Module'",
            );
            assert_validation_fails_with(&manifest, "but found ''");
        }
    }
}

#[cfg(test)]
mod test_tf_variable_required {
    use crate::TfVariable;
    use serde_json::Value;

    fn var(default: Option<Value>, nullable: bool) -> TfVariable {
        TfVariable {
            name: "test".to_string(),
            _type: Value::String("string".to_string()),
            default,
            description: String::new(),
            nullable,
            sensitive: false,
        }
    }

    #[test]
    fn required_when_default_is_none() {
        let v = var(None, true);
        assert!(v.required(), "variable with no default should be required");
    }

    #[test]
    fn required_when_default_is_null_and_not_nullable() {
        let v = var(Some(Value::Null), false);
        assert!(
            v.required(),
            "variable with default null and nullable=false should be required"
        );
    }

    #[test]
    fn not_required_when_default_is_null_and_nullable() {
        let v = var(Some(Value::Null), true);
        assert!(
            !v.required(),
            "variable with default null and nullable=true should not be required"
        );
    }

    #[test]
    fn not_required_when_default_has_value() {
        let v = var(Some(Value::String("foo".to_string())), false);
        assert!(
            !v.required(),
            "variable with non-null default should not be required"
        );
    }
}
