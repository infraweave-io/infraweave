use heck::{ToLowerCamelCase, ToSnakeCase};

pub fn to_snake_case(s: &str) -> String {
    s.to_snake_case()
}

pub fn to_camel_case(s: &str) -> String {
    s.to_lower_camel_case()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_convert_camel_case_single_underscore() {
        let input = "test_abc";
        let expected = "testAbc";
        let actual = to_camel_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_convert_camel_case_single_number_no_underscore() {
        let input = "test5something";
        let expected = "test5something";
        let actual = to_camel_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_convert_camel_case_double_underscore() {
        let input = "test__abc";
        let expected = "testAbc";
        let actual = to_camel_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_convert_snake_case_basic() {
        let input = "bucketName";
        let expected = "bucket_name";
        let actual = to_snake_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_convert_snake_case_multiple_words() {
        let input = "myLongVariableName";
        let expected = "my_long_variable_name";
        let actual = to_snake_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_convert_snake_case_already_snake_case() {
        let input = "bucket_name";
        let expected = "bucket_name";
        let actual = to_snake_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_convert_snake_case_pascal_case() {
        let input = "BucketName";
        let expected = "bucket_name";
        let actual = to_snake_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_convert_snake_case_with_numbers() {
        let input = "variable123Name";
        let expected = "variable123_name";
        let actual = to_snake_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_convert_snake_case_single_word() {
        let input = "bucket";
        let expected = "bucket";
        let actual = to_snake_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_convert_snake_case_uppercase() {
        let input = "BUCKET_NAME";
        let expected = "bucket_name";
        let actual = to_snake_case(input);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_roundtrip_edge_cases_pass() {
        // Test cases that should pass roundtrip conversion
        let passing_cases = vec![
            ("http2_enabled", "number in middle of word"),
            ("s3_bucket", "number at start of word"),
            ("my_v2_api", "number between words"),
            ("ipv4_address", "number in middle of first word"),
            ("api_v1", "number at end"),
            ("normal_name", "snake_case"),
            ("bucket_name", "standard snake_case"),
            ("enable_logging", "standard snake_case"),
            ("port8080", "number at end of word"),
        ];

        for (original, description) in passing_cases {
            let camel = to_camel_case(original);
            let back = to_snake_case(&camel);

            println!(
                "PASS {}: '{}' -> '{}' -> '{}'",
                description, original, camel, back
            );

            assert_eq!(
                original, back,
                "Expected '{}' ({}) to pass roundtrip",
                original, description
            );
        }
    }

    #[test]
    fn test_roundtrip_edge_cases_fail() {
        // Test cases that should fail roundtrip conversion
        let failing_cases = vec![
            ("port_8080", "number after underscore"),
            ("x_123_test", "pure number segment"),
            ("test_123", "ends with pure number"),
            ("bucket__name", "double underscore"),
            ("_private", "leading underscore"),
            ("trailing_", "trailing underscore"),
            ("normalName", "camelCase"),
            ("NormalName", "PascalCase"),
            ("a_b_c", "multiple single letters"),
        ];

        for (original, description) in failing_cases {
            let camel = to_camel_case(original);
            let back = to_snake_case(&camel);

            println!(
                "FAIL {}: '{}' -> '{}' -> '{}'",
                description, original, camel, back
            );

            assert_ne!(
                original, back,
                "Expected '{}' ({}) to fail roundtrip, but it passed",
                original, description
            );
        }
    }
}
