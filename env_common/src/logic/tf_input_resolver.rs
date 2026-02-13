use env_utils::to_snake_case;
use hcl::{
    expr::{Heredoc, TemplateExpr, Traversal, Variable},
    Expression, Identifier,
};
use log::debug;
use regex::Regex;

use crate::errors::ModuleError;

pub struct TfInputResolver {
    regex: Regex,
    known_variables: Vec<String>,
    known_outputs: Vec<String>,
}

impl TfInputResolver {
    pub fn new(known_variables: Vec<String>, known_outputs: Vec<String>) -> Self {
        debug!("Known Variables: {known_variables:?}");
        debug!("Known Output: {known_outputs:?}");
        TfInputResolver {
            regex: Regex::new(
                r"(?P<full_ref>\{\{\s?(?P<kind>\w+):+(?P<claim>\w+):+(?P<field>\w+)\s?\}\})",
            )
            .unwrap(),
            known_variables,
            known_outputs,
        }
    }

    pub fn resolve(&self, value: serde_yaml::Value) -> Result<Expression, ModuleError> {
        match value {
            serde_yaml::Value::Null => Ok(Expression::Null),
            serde_yaml::Value::Bool(val) => Ok(Expression::Bool(val)),
            serde_yaml::Value::Number(number) => Ok(Expression::Number(
                TfInputResolver::number_yaml_to_hcl(number),
            )),
            serde_yaml::Value::String(val) => self.string_to_expression(&val),
            serde_yaml::Value::Sequence(values) => Ok(Expression::Array(
                values
                    .iter()
                    .map(|val| self.resolve(val.clone()).unwrap())
                    .collect(),
            )),
            serde_yaml::Value::Mapping(map) => Ok(Expression::Object(hcl::Object::from_iter(
                map.iter().map(|(key, val)| {
                    (
                        hcl::ObjectKey::Identifier(
                            Identifier::new(key.as_str().unwrap().to_string()).unwrap(),
                        ),
                        self.resolve(val.clone()).unwrap(),
                    )
                }),
            ))),
        }
    }

    fn number_yaml_to_hcl(number: serde_yaml::Number) -> hcl::Number {
        if let Some(i) = number.as_i64() {
            hcl::Number::from(i)
        } else if let Some(f) = number.as_f64() {
            hcl::Number::from_f64(f).expect("failed to convert float")
        } else {
            panic!("Unexpected number format")
        }
    }

    fn string_to_expression(&self, input: &str) -> Result<Expression, ModuleError> {
        let mut return_string = input.to_string();
        for m in self.regex.captures_iter(input) {
            let to_replace = m.name("full_ref").unwrap().as_str();
            let kind = m.name("kind").unwrap().as_str();
            let claim_name = m.name("claim").unwrap().as_str();
            let field = m.name("field").unwrap().as_str();

            // Validate canonical format matches expected pattern
            let canonical_pattern = format!("{kind}::{claim_name}::{field}");
            if !to_replace.contains(&canonical_pattern) {
                // This should not happen with strict regex enforcement, but keep check for safety
                return Err(ModuleError::InvalidReference(
                    to_replace.to_string(),
                    format!("{{{{ {}::{}::{} }}}}", kind, claim_name, field),
                ));
            }

            let field_snake_case = to_snake_case(field);

            // Handle Stack::variables::* references
            if kind == "Stack" && claim_name == "variables" {
                // Stack-level variables are stored as stack__<variable_name>
                let stack_var_key = format!("stack__{}", field_snake_case);

                if self.known_variables.contains(&stack_var_key) {
                    if input.len() == to_replace.len() {
                        return Ok(Expression::from(
                            Traversal::builder(Variable::new("var").unwrap())
                                .attr(stack_var_key)
                                .build(),
                        ));
                    } else {
                        return_string = return_string
                            .replace(to_replace, &format!("${{var.{}}}", stack_var_key));
                    }
                    continue;
                } else {
                    return Err(ModuleError::UnresolvedReference(
                        to_replace.to_string(),
                        stack_var_key,
                    ));
                }
            }

            let search_key = TfInputResolver::prefix_name(claim_name, &field_snake_case);

            if self.known_outputs.contains(&search_key) {
                if input.len() == to_replace.len() {
                    return Ok(Expression::from(
                        Traversal::builder(Variable::new("module").unwrap())
                            .attr(to_snake_case(claim_name))
                            .attr(field_snake_case)
                            .build(),
                    ));
                } else {
                    return_string = return_string.replace(
                        to_replace,
                        &format!(
                            "${{module.{}.{}}}",
                            to_snake_case(claim_name),
                            field_snake_case
                        ),
                    );
                }
            } else if self.known_variables.contains(&search_key) {
                if input.len() == to_replace.len() {
                    return Ok(Expression::from(
                        Traversal::builder(Variable::new("var").unwrap())
                            .attr(TfInputResolver::prefix_name(claim_name, &field_snake_case))
                            .build(),
                    ));
                } else {
                    return_string = return_string.replace(
                        to_replace,
                        &format!(
                            "${{var.{}}}",
                            TfInputResolver::prefix_name(claim_name, &field_snake_case)
                        ),
                    );
                }
            } else {
                return Err(ModuleError::UnresolvedReference(
                    to_replace.to_string(),
                    search_key.to_string(),
                ));
            }
        }
        if return_string == input {
            Ok(Expression::String(input.to_string()))
        } else {
            // If the string contains newlines, use heredoc format
            if return_string.contains('\n') {
                Ok(Expression::from(TemplateExpr::Heredoc(Heredoc::new(
                    Identifier::new("EOF").unwrap(),
                    return_string,
                ))))
            } else {
                Ok(Expression::from(TemplateExpr::QuotedString(return_string)))
            }
        }
    }

    fn prefix_name(claim_name: &str, field_name: &str) -> String {
        format!("{}__{}", to_snake_case(claim_name), field_name)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use hcl::{Expression, Identifier};

    use crate::{errors::ModuleError, logic::tf_input_resolver::TfInputResolver};

    #[test]
    fn no_reference_just_converts() {
        let tf_input_resolver = TfInputResolver::new(Vec::with_capacity(0), Vec::with_capacity(0));
        let expr = tf_input_resolver.resolve(serde_yaml::Value::String("hello".to_string()));

        assert!(expr.is_ok(), "Didn't return a result");
        if let Expression::String(val) = expr.unwrap() {
            assert_eq!("hello".to_string(), val)
        } else {
            assert!(false, "Expression isn't Expression::String")
        }
    }

    #[test]
    fn reference_without_target_throw_error() {
        let tf_input_resolver = TfInputResolver::new(Vec::with_capacity(0), Vec::with_capacity(0));
        let expr = tf_input_resolver.resolve(serde_yaml::Value::String(
            "{{ S3Bucket::bucket1a::bucketName }}".to_string(),
        ));

        assert!(expr.is_err(), "Didn't error out");
        if let ModuleError::UnresolvedReference(reference, seach_key) = expr.err().unwrap() {
            assert_eq!(reference, "{{ S3Bucket::bucket1a::bucketName }}");
            assert_eq!(seach_key, "bucket1a__bucket_name");
        } else {
            assert!(false, "Incorrect error type");
        }
    }

    #[test]
    fn reference_to_output_direct() {
        let tf_input_resolver = TfInputResolver::new(
            Vec::with_capacity(0),
            vec![String::from("bucket1a__bucket_name")],
        );
        let expr = tf_input_resolver.resolve(serde_yaml::Value::String(
            "{{ S3Bucket::bucket1a::bucketName }}".to_string(),
        ));

        assert!(expr.is_ok(), "Didn't return a result");
        if let Expression::Traversal(traversal) = expr.unwrap() {
            assert_eq!(
                hcl::format::to_string(traversal.as_ref()).unwrap(),
                "module.bucket1a.bucket_name"
            )
        } else {
            assert!(false, "Didn't return a traversal")
        }
    }

    #[test]
    fn reference_to_output_template() {
        let tf_input_resolver = TfInputResolver::new(
            Vec::with_capacity(0),
            vec![String::from("bucket1a__bucket_name")],
        );
        let expr = tf_input_resolver.resolve(serde_yaml::Value::String(
            "{{ S3Bucket::bucket1a::bucketName }}-after".to_string(),
        ));

        assert!(expr.is_ok(), "Didn't return a result");
        if let Expression::TemplateExpr(template_expr) = expr.unwrap() {
            assert_eq!(
                hcl::format::to_string(template_expr.as_ref()).unwrap(),
                "\"${module.bucket1a.bucket_name}-after\""
            )
        } else {
            assert!(false, "Didn't return a TemplateExpr")
        }
    }

    #[test]
    fn reference_to_variable_direct() {
        let tf_input_resolver = TfInputResolver::new(
            vec![String::from("bucket1a__bucket_name")],
            Vec::with_capacity(0),
        );
        let expr = tf_input_resolver.resolve(serde_yaml::Value::String(
            "{{ S3Bucket::bucket1a::bucketName }}".to_string(),
        ));

        assert!(expr.is_ok(), "Didn't return a result");
        if let Expression::Traversal(traversal) = expr.unwrap() {
            assert_eq!(
                hcl::format::to_string(traversal.as_ref()).unwrap(),
                "var.bucket1a__bucket_name"
            )
        } else {
            assert!(false, "Didn't return a traversal")
        }
    }

    #[test]
    fn reference_to_variable_template() {
        let tf_input_resolver = TfInputResolver::new(
            vec![String::from("bucket1a__bucket_name")],
            Vec::with_capacity(0),
        );
        let expr = tf_input_resolver.resolve(serde_yaml::Value::String(
            "{{ S3Bucket::bucket1a::bucketName }}-after".to_string(),
        ));

        assert!(expr.is_ok(), "Didn't return a result");
        if let Expression::TemplateExpr(template_expr) = expr.unwrap() {
            assert_eq!(
                hcl::format::to_string(template_expr.as_ref()).unwrap(),
                "\"${var.bucket1a__bucket_name}-after\""
            )
        } else {
            assert!(false, "Didn't return a TemplateExpr")
        }
    }

    #[test]
    fn reference_template_output_and_variable() {
        let tf_input_resolver = TfInputResolver::new(
            vec![String::from("bucket1a__bucket_name")],
            vec![String::from("bucket1b__bucket_name")],
        );
        let expr = tf_input_resolver.resolve(serde_yaml::Value::String(
            "{{ S3Bucket::bucket1a::bucketName }}-{{ S3Bucket::bucket1b::bucketName }}".to_string(),
        ));

        assert!(expr.is_ok(), "Didn't return a result");
        if let Expression::TemplateExpr(template_expr) = expr.unwrap() {
            assert_eq!(
                hcl::format::to_string(template_expr.as_ref()).unwrap(),
                "\"${var.bucket1a__bucket_name}-${module.bucket1b.bucket_name}\""
            )
        } else {
            assert!(false, "Didn't return a TemplateExpr")
        }
    }

    #[test]
    fn reference_in_yaml_mapping() {
        let tf_input_resolver = TfInputResolver::new(
            vec![String::from("bucket1a__bucket_name")],
            vec![String::from("bucket1b__bucket_name")],
        );
        let expr = tf_input_resolver.resolve(serde_yaml::Value::Mapping(
            serde_yaml::Mapping::from_iter(vec![
                (
                    serde_yaml::Value::String("from_var".to_string()),
                    serde_yaml::Value::String(
                        "{{ S3Bucket::bucket1a::bucketName }}-should_be_variable".to_string(),
                    ),
                ),
                (
                    serde_yaml::Value::String("from_output".to_string()),
                    serde_yaml::Value::String(
                        "{{ S3Bucket::bucket1b::bucketName }}-should_be_output".to_string(),
                    ),
                ),
            ]),
        ));

        assert!(expr.is_ok(), "Didn't return a result");
        if let Expression::Object(map) = expr.unwrap() {
            if let Expression::TemplateExpr(template_expr) = map
                .get(&hcl::ObjectKey::Identifier(
                    Identifier::new("from_var").unwrap(),
                ))
                .unwrap()
            {
                assert_eq!(
                    hcl::format::to_string(template_expr.as_ref()).unwrap(),
                    "\"${var.bucket1a__bucket_name}-should_be_variable\""
                );
            } else {
                assert!(false, "Didn't return Template for key \"from_var\"");
            }
            if let Expression::TemplateExpr(template_expr) = map
                .get(&hcl::ObjectKey::Identifier(
                    Identifier::new("from_output").unwrap(),
                ))
                .unwrap()
            {
                assert_eq!(
                    hcl::format::to_string(template_expr.as_ref()).unwrap(),
                    "\"${module.bucket1b.bucket_name}-should_be_output\""
                );
            } else {
                assert!(false, "Didn't return Template for key \"from_output\"");
            }
        } else {
            assert!(false, "Didn't return Expression::Object");
        }
    }

    #[test]
    fn reference_in_yaml_list() {
        let tf_input_resolver = TfInputResolver::new(
            vec![String::from("bucket1a__bucket_name")],
            vec![String::from("bucket1b__bucket_name")],
        );
        let expr = tf_input_resolver.resolve(serde_yaml::Value::Sequence(vec![
            serde_yaml::Value::String(
                "{{ S3Bucket::bucket1a::bucketName }}-should_be_variable".to_string(),
            ),
            serde_yaml::Value::String(
                "{{ S3Bucket::bucket1b::bucketName }}-should_be_output".to_string(),
            ),
        ]));

        assert!(expr.is_ok(), "Didn't return a result");
        if let Expression::Array(arr) = expr.unwrap() {
            assert_eq!(
                arr.iter()
                    .map(|e| hcl::format::to_string(e).unwrap())
                    .collect::<HashSet<String>>(),
                HashSet::from_iter(vec![
                    "\"${var.bucket1a__bucket_name}-should_be_variable\"".to_string(),
                    "\"${module.bucket1b.bucket_name}-should_be_output\"".to_string()
                ])
            );
        } else {
            assert!(false, "Didn't return Expression::Array");
        }
    }

    #[test]
    fn reference_in_multiline_string() {
        let tf_input_resolver = TfInputResolver::new(
            Vec::with_capacity(0),
            vec![String::from("s3bucket__bucket_arn")],
        );

        let policy_json = r#"{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": "s3:ListBucket",
      "Resource": "{{ S3Bucket::s3bucket::bucketArn }}"
    }
  ]
}"#;

        let expr = tf_input_resolver.resolve(serde_yaml::Value::String(policy_json.to_string()));

        assert!(expr.is_ok(), "Didn't return a result");

        // Verify it returns a TemplateExpr with Heredoc (not QuotedString)
        match expr.unwrap() {
            Expression::TemplateExpr(template_expr) => {
                match template_expr.as_ref() {
                    hcl::expr::TemplateExpr::Heredoc(heredoc) => {
                        // Verify the reference was replaced
                        assert!(
                            heredoc.template.contains("${module.s3bucket.bucket_arn}"),
                            "Expected reference to module output, got: {}",
                            heredoc.template
                        );
                        // Verify JSON structure is preserved
                        assert!(
                            heredoc.template.contains("s3:ListBucket"),
                            "Expected policy content to be preserved, got: {}",
                            heredoc.template
                        );
                        // Verify newlines are preserved
                        assert!(
                            heredoc.template.contains('\n'),
                            "Expected newlines to be preserved in heredoc"
                        );
                    }
                    hcl::expr::TemplateExpr::QuotedString(_) => {
                        panic!(
                            "Multiline strings with references must use Heredoc, not QuotedString. \
                            QuotedString will cause Terraform to fail with 'Invalid multi-line string' error."
                        );
                    }
                }
            }
            other => {
                panic!("Expected TemplateExpr, got: {:?}", other);
            }
        }
    }

    #[test]
    fn invalid_references() {
        let tf_input_resolver = TfInputResolver::new(vec![], vec![]);
        assert!(
            vec![
                "{{ S3Bucket:bucket1a::bucketName }}".to_string(),
                "{{ S3Bucket:bucket1a:bucketName }}".to_string(),
                "{{ S3Bucket:bucket1a:::bucketName }}".to_string(),
                "{{ S3Bucket::bucket1a:bucketName }}".to_string(),
                "{{ S3Bucket:::bucket1a:::bucketName }}".to_string(),
            ]
            .iter()
            .map(|input|tf_input_resolver.resolve(serde_yaml::Value::String(input.clone())))
            .all(|result| matches!(result, Err(err) if matches!(err, ModuleError::InvalidReference(_, _))))
        )
    }
}
