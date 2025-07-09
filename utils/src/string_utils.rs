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
}
