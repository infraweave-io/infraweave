use env_defs::{DeploymentManifest, Lifecycle, ProviderResp};
use env_utils::to_camel_case;
use hcl::{
    expr::{Traversal, TraversalOperator, Variable},
    Attribute, Block, BlockLabel, Expression, Identifier, Object, ObjectKey,
};
use log::info;

use crate::logic::tf_input_resolver::TfInputResolver;

pub fn module_block(
    deployment: &DeploymentManifest,
    variables: &Vec<Attribute>,
    providers: &Vec<(ObjectKey, Expression)>,
    dependencies: &Vec<String>,
    lifecycle_enabled_variable: Option<Lifecycle>,
) -> Block {
    Block::builder("module")
        .add_label(BlockLabel::String(deployment.metadata.name.clone()))
        .add_attribute(Attribute::new(
            "source",
            Expression::String(format!(
                "./{}-{}",
                deployment.kind.clone(),
                deployment.spec.module_version.clone().unwrap()
            )),
        ))
        .add_attributes(variables.clone())
        .add_attribute(Attribute::new(
            "providers",
            Expression::Object(Object::from(providers.clone())),
        ))
        .add_attributes(dependencies_attributes(dependencies))
        .add_blocks(match &lifecycle_enabled_variable {
            None => Vec::with_capacity(0),
            Some(lifecycle) => {
                let parsed_expr = hcl::edit::parser::parse_expr(&lifecycle.enabled).unwrap();
                vec![Block::builder("lifecycle")
                    .add_attribute(Attribute::new("enabled", parsed_expr))
                    .build()]
            }
        })
        .build()
}

pub fn variables(
    module_inputs: &Vec<(String, String)>,
    deployment: &DeploymentManifest,
    input_resolver: &TfInputResolver,
) -> Vec<Attribute> {
    let mut return_val: Vec<Attribute> = Vec::new();
    for (input_name, fq_input_name) in module_inputs {
        if let Some(val) = deployment
            .spec
            .variables
            .get(&serde_yaml::Value::String(to_camel_case(input_name)))
        {
            let mut expr = input_resolver
                .resolve(val.clone())
                .unwrap_or_else(|e| panic!("{e}"));
            if can_be_variable(&expr) {
                expr = Expression::from(
                    hcl::expr::Traversal::builder(Variable::new("var").unwrap())
                        .attr(fq_input_name.to_string())
                        .build(),
                )
            }
            info!(
                "Assigning {}={} for {}",
                &input_name,
                hcl::format::to_string(&expr).unwrap(),
                deployment.metadata.name
            );
            return_val.push(Attribute::new(input_name.to_string(), expr));
        } else {
            return_val.push(Attribute::new(
                input_name.to_string(),
                Expression::from(
                    hcl::expr::Traversal::builder(Variable::new("var").unwrap())
                        .attr(fq_input_name.to_string())
                        .build(),
                ),
            ));
        }
    }
    return_val
}

// TODO: Check this, I believe that Expression::Array, Expression::Object can never be variable. Since the assignment will be wonky, I think.
fn can_be_variable(expr: &Expression) -> bool {
    match expr {
        Expression::Array(expressions) => expressions.iter().all(|e| can_be_variable(e) == true),
        Expression::Object(vec_map) => vec_map.values().all(|e| can_be_variable(e) == true),
        Expression::TemplateExpr(_) => false,
        Expression::Traversal(_) => false,
        _ => true,
    }
}

pub fn providers(provider_resps: &Vec<ProviderResp>) -> Vec<(ObjectKey, Expression)> {
    provider_resps
        .iter()
        .map(|provider_resp| {
            let configuration_name_expr =
                config_name_to_expression(provider_resp.manifest.spec.configuration_name());
            (
                ObjectKey::Expression(configuration_name_expr.clone()),
                configuration_name_expr.clone(),
            )
        })
        .collect::<Vec<(ObjectKey, Expression)>>()
}

fn config_name_to_expression(provider_name: String) -> Expression {
    let parts: Vec<&str> = provider_name.split(".").collect();
    let first = Expression::Variable(Variable::new(parts[0]).unwrap());
    if parts.len() == 1 {
        return first;
    }
    return Expression::from(Traversal::new(
        first,
        parts[1..]
            .iter()
            .map(|p| TraversalOperator::GetAttr(Identifier::new(p.to_string()).unwrap()))
            .collect::<Vec<TraversalOperator>>(),
    ));
}

fn dependencies_attributes(dependencies: &Vec<String>) -> Vec<Attribute> {
    if dependencies.is_empty() {
        return Vec::with_capacity(0);
    }
    vec![Attribute::new(
        Identifier::new("depends_on").unwrap(),
        Expression::Array(
            dependencies
                .iter()
                .map(|s| {
                    Expression::from(
                        Traversal::builder(Variable::new("module").unwrap())
                            .attr(s.clone())
                            .build(),
                    )
                })
                .collect(),
        ),
    )]
}
