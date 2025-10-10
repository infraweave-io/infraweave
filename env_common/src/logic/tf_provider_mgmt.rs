use log::warn;
use std::collections::HashMap;

use hcl::{Attribute, Block, BlockBuilder, Body, Expression, Identifier, Object, ObjectKey};

pub struct TfProviderMgmt {
    terraform: Vec<Block>,
    providers: Vec<Block>,
    variables: Vec<Block>,
    locals: Vec<Attribute>,
    output: Vec<Block>,
    data: Vec<Block>,
}

impl Default for TfProviderMgmt {
    fn default() -> Self {
        TfProviderMgmt::new()
    }
}

impl TfProviderMgmt {
    pub const fn new() -> Self {
        TfProviderMgmt {
            terraform: Vec::new(),
            providers: Vec::new(),
            variables: Vec::new(),
            locals: Vec::new(),
            output: Vec::new(),
            data: Vec::new(),
        }
    }

    fn add_check_duplicate_on_labels(target: &mut Vec<Block>, identifier: &str, new_block: &Block) {
        if new_block.identifier() != identifier {
            return;
        }
        if target.iter().any(|e| e.labels() == new_block.labels()) {
            warn!(
                "{} {} has already been defined, skipping. This can be expected.",
                identifier,
                new_block
                    .labels()
                    .iter()
                    .map(|l| format!("\"{}\"", l.as_str()).to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        } else {
            target.push(new_block.clone());
        }
    }

    pub fn add_terraform(&mut self, new_block: &Block) {
        if new_block.identifier() != "terraform" {
            return;
        }
        self.terraform.push(new_block.clone());
    }

    pub fn terraform(&self) -> Vec<Block> {
        if self.terraform.is_empty() {
            return vec![];
        }
        vec![BlockBuilder::new("terraform")
            .add_attributes(self.required_version())
            .add_blocks(self.required_providers())
            .add_blocks(self.provider_meta())
            .build()]
    }

    fn required_version(&self) -> Vec<Attribute> {
        let mut attributes: Vec<Attribute> = Vec::with_capacity(1);
        let version_constraints = self
            .terraform
            .iter()
            .flat_map(|b| b.body().attributes())
            .filter(|a| a.key() == "required_version" && matches!(a.expr(), Expression::String(_)))
            .map(|a| match a.expr() {
                Expression::String(val) => Some(val),
                _ => None,
            })
            .filter(Option::is_some)
            .map(|o| o.unwrap().clone())
            .collect::<Vec<String>>();
        if !version_constraints.is_empty() {
            attributes.push(Attribute::new(
                "required_version",
                version_constraints.join(", "),
            ));
        }
        attributes
    }

    fn required_providers(&self) -> Vec<Block> {
        let mut blocks = Vec::with_capacity(1);
        let mut providers: HashMap<String, Object<ObjectKey, Expression>> = HashMap::new();
        let version_key = ObjectKey::Identifier(Identifier::new("version").unwrap());
        let configured_aliases_key =
            ObjectKey::Identifier(Identifier::new("configured_aliases").unwrap());
        self.terraform
            .iter()
            .flat_map(|t| t.body().blocks())
            .filter(|b| b.identifier() == "required_providers")
            .flat_map(|b| b.body().attributes())
            .for_each(|a| {
                if let Expression::Object(map) = a.expr() {
                    providers
                        .entry(a.key().to_string())
                        .and_modify(|m| {
                            let expr = map.get(&version_key).unwrap();
                            if let Expression::String(ver) = expr {
                                m.entry(version_key.clone())
                                    .and_modify(|v| {
                                        if let Expression::String(current) = v {
                                            *v = Expression::String(format!("{current}, {ver}"));
                                        }
                                    })
                                    .or_insert(expr.clone());
                            }
                            if let Some(new_expr) = map.get(&configured_aliases_key) {
                                if let Some(curr_expr) = m.get_mut(&configured_aliases_key) {
                                    if let Expression::Array(arr) = curr_expr {
                                        if let Expression::Array(new_arr) = new_expr {
                                            arr.extend(new_arr.iter().cloned());
                                        }
                                    }
                                } else {
                                    m.insert(configured_aliases_key.clone(), new_expr.clone());
                                }
                            }
                        })
                        .or_insert(map.clone());
                }
            });
        if !providers.is_empty() {
            blocks.push(
                BlockBuilder::new("required_providers")
                    .add_attributes(
                        providers
                            .iter()
                            .map(|e| Attribute::new(e.0.as_str(), e.1.clone())),
                    )
                    .build(),
            );
        }

        blocks
    }

    fn provider_meta(&self) -> Vec<Block> {
        self.terraform
            .iter()
            .flat_map(|t| t.body().blocks())
            .filter(|b| b.identifier() == "provider_meta")
            .cloned()
            .collect()
    }

    pub fn add_provider_configuration(&mut self, new_block: &Block) {
        if new_block.identifier() != "provider" {
            return;
        }
        if !self.providers.iter().any(|p| {
            p.labels() == new_block.labels()
                && TfProviderMgmt::get_alias(p) == TfProviderMgmt::get_alias(new_block)
        }) {
            self.providers.push(new_block.clone());
        }
    }

    fn get_alias(provider_block: &Block) -> Option<Expression> {
        provider_block.body().attributes().find_map(|a| {
            if a.key() == "alias" {
                Some(a.expr.clone())
            } else {
                None
            }
        })
    }

    pub fn provider_configuration(&self) -> Vec<Block> {
        self.providers.clone()
    }

    pub fn add_variable(&mut self, new_block: &Block) {
        TfProviderMgmt::add_check_duplicate_on_labels(&mut self.variables, "variable", new_block);
    }

    pub fn variables(&self) -> Vec<Block> {
        self.variables.clone()
    }

    pub fn add_locals(&mut self, new_block: &Block) {
        if new_block.identifier() != "locals" {
            return;
        }
        for attribute in new_block.body().attributes() {
            if !self.locals.iter().any(|a| a.key() == attribute.key()) {
                self.locals.push(attribute.clone());
            }
        }
    }

    pub fn locals(&self) -> Vec<Block> {
        if self.locals.is_empty() {
            return vec![];
        }
        vec![BlockBuilder::new("locals")
            .add_attributes(self.locals.clone())
            .build()]
    }

    pub fn add_output(&mut self, new_block: &Block) {
        TfProviderMgmt::add_check_duplicate_on_labels(&mut self.output, "output", new_block);
    }

    pub fn output(&self) -> Vec<Block> {
        self.output.clone()
    }

    pub fn add_data(&mut self, new_block: &Block) {
        TfProviderMgmt::add_check_duplicate_on_labels(&mut self.data, "data", new_block);
    }

    pub fn data(&self) -> Vec<Block> {
        self.data.clone()
    }

    pub fn add_block(&mut self, new_block: &Block) {
        self.add_terraform(new_block);
        self.add_provider_configuration(new_block);
        self.add_locals(new_block);
        self.add_variable(new_block);
        self.add_data(new_block);
        self.add_output(new_block);
    }

    pub fn build(&self) -> Body {
        Body::builder()
            .add_blocks(self.terraform())
            .add_blocks(self.provider_configuration())
            .add_blocks(self.locals())
            .add_blocks(self.variables())
            .add_blocks(self.data())
            .add_blocks(self.output())
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCase {
        inputs: Vec<&'static str>,
        expected: &'static str,
    }

    #[cfg(test)]
    mod test_terraform {
        use super::*;

        #[test]
        fn required_version() {
            test_terraform(TestCase {
                inputs: vec![
                    r#"
                        terraform {
                            required_version = "~>1.0"
                        }
                    "#,
                    r#"
                        terraform {
                            required_version = ">=1.5.0"
                        }
                    "#,
                ],
                expected: r#"
                    terraform {
                        required_version = "~>1.0, >=1.5.0"
                    }
                "#,
            });
        }

        #[test]
        fn provider_meta() {
            test_terraform(TestCase {
                inputs: vec![
                    r#"
                        terraform {
                            provider_meta "my-provider" {
                                hello = "world"
                            }
                        }
                    "#,
                ],
                expected: r#"
                    terraform {
                        provider_meta "my-provider" {
                            hello = "world"
                        }
                    }
                "#,
            });
        }

        #[test]
        fn required_provider_single() {
            test_terraform(TestCase {
                inputs: vec![
                    r#"
                        terraform {
                            required_providers {
                                aws = {
                                    version = ">= 2.1.0"
                                    source = "hashicorp/aws"
                                }
                            }
                        }
                    "#,
                ],
                expected: r#"
                    terraform {
                        required_providers {
                            aws = {
                                version = ">= 2.1.0"
                                source = "hashicorp/aws"
                            }
                        }
                    }
                "#,
            });
        }

        #[test]
        fn required_provider_version_constraint() {
            test_terraform(TestCase {
                inputs: vec![
                    r#"
                        terraform {
                            required_providers {
                                aws = {
                                    version = ">= 2.1.0"
                                    source = "hashicorp/aws"
                                }
                            }
                        }
                    "#,
                    r#"
                        terraform {
                            required_providers {
                                aws = {
                                    version = ">= 2.5.0"
                                    source = "hashicorp/aws"
                                }
                            }
                        }
                    "#,
                ],
                expected: r#"
                    terraform {
                        required_providers {
                            aws = {
                            version = ">= 2.1.0, >= 2.5.0"
                            source = "hashicorp/aws"
                            }
                        }
                    }
                "#,
            });
        }

        #[test]
        fn required_proivder_configured_aliases_add() {
            test_terraform(TestCase {
                inputs: vec![
                    r#"
                        terraform {
                            required_providers {
                                aws = {
                                    version = ">= 2.1.0"
                                    source = "hashicorp/aws"
                                }
                            }
                        }
                    "#,
                    r#"
                        terraform {
                            required_providers {
                                aws = {
                                    version = ">= 2.5.0"
                                    source = "hashicorp/aws"
                                    configured_aliases = [aws.usw1]
                                }
                            }
                        }
                    "#,
                ],
                expected: r#"
                    terraform {
                        required_providers {
                            aws = {
                                version = ">= 2.1.0, >= 2.5.0"
                                source = "hashicorp/aws"
                                configured_aliases = [aws.usw1]
                            }
                        }
                    }
                "#,
            });
        }

        #[test]
        fn required_provider_configured_aliases_modify() {
            test_terraform(TestCase {
                inputs: vec![
                    r#"
                        terraform {
                            required_providers {
                                aws = {
                                    version = ">= 2.1.0"
                                    source = "hashicorp/aws"
                                    configured_aliases = [aws.usw1]
                                }
                            }
                        }
                    "#,
                    r#"
                        terraform {
                            required_providers {
                                aws = {
                                    version = ">= 2.5.0"
                                    source = "hashicorp/aws"
                                    configured_aliases = [aws.usw2]
                                }
                            }
                        }
                    "#,
                ],
                expected: r#"
                    terraform {
                        required_providers {
                            aws = {
                                version = ">= 2.1.0, >= 2.5.0"
                                source = "hashicorp/aws"
                                configured_aliases = [aws.usw1, aws.usw2]
                            }
                        }
                    }
                "#,
            });
        }

        fn test_terraform(test_case: TestCase) {
            let mut provider_mgr = TfProviderMgmt::new();
            for i in test_case.inputs {
                hcl::parse(i)
                    .unwrap()
                    .blocks()
                    .for_each(|b| provider_mgr.add_terraform(b));
            }
            let expected: Vec<Block> = hcl::parse(test_case.expected)
                .unwrap()
                .into_blocks()
                .collect();
            assert_eq!(provider_mgr.terraform(), expected)
        }
    }

    #[cfg(test)]
    mod test_provider_configuration {
        use super::*;

        #[test]
        fn add_one() {
            test_provider_configuration(TestCase {
                inputs: vec![
                    r#"
                        provider "aws" {
                            region     = "us-west-2"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                        }
                    "#,
                ],
                expected: r#"
                    provider "aws" {
                        region     = "us-west-2"
                        access_key = "my-access-key"
                        secret_key = "my-secret-key"
                    }
                "#,
            });
        }

        #[test]
        fn add_two_in_same() {
            test_provider_configuration(TestCase {
                inputs: vec![
                    r#"
                        provider "aws" {
                            region     = "us-west-2"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                        }

                        provider "helm" {
                            kubernetes = {
                                config_path = "~/.kube/config"
                            }

                            registries = [
                                {
                                url      = "oci://localhost:5000"
                                username = "username"
                                password = "password"
                                },
                                {
                                url      = "oci://private.registry"
                                username = "username"
                                password = "password"
                                }
                            ]
                        }
                    "#,
                ],
                expected: r#"
                    provider "aws" {
                        region     = "us-west-2"
                        access_key = "my-access-key"
                        secret_key = "my-secret-key"
                    }

                    provider "helm" {
                        kubernetes = {
                            config_path = "~/.kube/config"
                        }

                        registries = [
                            {
                            url      = "oci://localhost:5000"
                            username = "username"
                            password = "password"
                            },
                            {
                            url      = "oci://private.registry"
                            username = "username"
                            password = "password"
                            }
                        ]
                    }
                "#,
            });
        }

        #[test]
        fn add_two() {
            test_provider_configuration(TestCase {
                inputs: vec![
                    r#"
                        provider "aws" {
                            region     = "us-west-2"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                        }
                    "#,
                    r#"
                        provider "helm" {
                            kubernetes = {
                                config_path = "~/.kube/config"
                            }

                            registries = [
                                {
                                url      = "oci://localhost:5000"
                                username = "username"
                                password = "password"
                                },
                                {
                                url      = "oci://private.registry"
                                username = "username"
                                password = "password"
                                }
                            ]
                        }
                    "#,
                ],
                expected: r#"
                    provider "aws" {
                        region     = "us-west-2"
                        access_key = "my-access-key"
                        secret_key = "my-secret-key"
                    }

                    provider "helm" {
                        kubernetes = {
                            config_path = "~/.kube/config"
                        }

                        registries = [
                            {
                            url      = "oci://localhost:5000"
                            username = "username"
                            password = "password"
                            },
                            {
                            url      = "oci://private.registry"
                            username = "username"
                            password = "password"
                            }
                        ]
                    }
                "#,
            });
        }

        #[test]
        fn drop_duplicates_no_alias() {
            test_provider_configuration(TestCase {
                inputs: vec![
                    r#"
                        provider "aws" {
                            region     = "us-west-2"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                        }
                    "#,
                    r#"
                        provider "aws" {
                            region     = "us-west-1"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                        }
                    "#,
                ],
                expected: r#"
                    provider "aws" {
                        region     = "us-west-2"
                        access_key = "my-access-key"
                        secret_key = "my-secret-key"
                    }
                "#,
            });
        }

        #[test]
        fn drop_duplicates_same_alias() {
            test_provider_configuration(TestCase {
                inputs: vec![
                    r#"
                        provider "aws" {
                            region     = "us-west-1"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                            alias = "usw1"
                        }
                    "#,
                    r#"
                        provider "aws" {
                            region     = "us-west-1"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                            alias = "usw1"
                        }
                    "#,
                ],
                expected: r#"
                    provider "aws" {
                        region     = "us-west-1"
                        access_key = "my-access-key"
                        secret_key = "my-secret-key"
                        alias = "usw1"
                    }
                "#,
            });
        }

        #[test]
        fn accept_duplicate_with_different_alias() {
            test_provider_configuration(TestCase {
                inputs: vec![
                    r#"
                        provider "aws" {
                            region     = "us-west-2"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                        }
                    "#,
                    r#"
                        provider "aws" {
                            region     = "us-west-1"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                            alias = "usw1"
                        }
                    "#,
                ],
                expected: r#"
                    provider "aws" {
                        region     = "us-west-2"
                        access_key = "my-access-key"
                        secret_key = "my-secret-key"
                    }

                    provider "aws" {
                        region     = "us-west-1"
                        access_key = "my-access-key"
                        secret_key = "my-secret-key"
                        alias = "usw1"
                    }
                "#,
            });
        }

        fn test_provider_configuration(test_case: TestCase) {
            let mut provider_mgr = TfProviderMgmt::new();
            for i in test_case.inputs {
                hcl::parse(i)
                    .unwrap()
                    .blocks()
                    .for_each(|b| provider_mgr.add_provider_configuration(b));
            }
            let expected: Vec<Block> = hcl::parse(test_case.expected)
                .unwrap()
                .into_blocks()
                .collect();
            assert_eq!(provider_mgr.provider_configuration(), expected);
        }
    }

    #[cfg(test)]
    mod test_variables {
        use super::*;
        #[test]
        fn add_one() {
            test_variables(TestCase {
                inputs: vec![
                    r#"
                        variable "hello" {
                            type=string
                        }
                    "#,
                ],
                expected: r#"
                    variable "hello" {
                        type=string
                    }
                "#,
            });
        }

        #[test]
        fn add_two_in_same() {
            test_variables(TestCase {
                inputs: vec![
                    r#"
                        variable "hello" {
                            type=string
                        }

                        variable "bye" {
                            type=bool
                        }
                    "#,
                ],
                expected: r#"
                    variable "hello" {
                        type=string
                    }

                    variable "bye" {
                        type=bool
                    }
                "#,
            });
        }

        #[test]
        fn two_inputs() {
            test_variables(TestCase {
                inputs: vec![
                    r#"
                        variable "hello" {
                            type=string
                        }
                    "#,
                    r#"
                        variable "bye" {
                            type=bool
                        }
                    "#,
                ],
                expected: r#"
                    variable "hello" {
                        type=string
                    }
                    
                    variable "bye" {
                        type=bool
                    }
                "#,
            });
        }

        #[test]
        fn only_variables() {
            test_variables(TestCase {
                inputs: vec![
                    r#"
                        variable "hello" {
                            type=string
                        }
                    "#,
                    r#"
                        provider "bye" {
                            type=bool
                        }
                    "#,
                ],
                expected: r#"
                    variable "hello" {
                        type=string
                    }
                "#,
            });
        }

        #[test]
        fn no_ducplicates() {
            test_variables(TestCase {
                inputs: vec![
                    r#"
                        variable "hello" {
                            type=string
                        }
                    "#,
                    r#"
                        variable "hello" {
                            type=string
                        }
                    "#,
                ],
                expected: r#"
                    variable "hello" {
                        type=string
                    }
                "#,
            });
        }

        fn test_variables(test_case: TestCase) {
            let mut provider_mgr = TfProviderMgmt::new();
            for i in test_case.inputs {
                hcl::parse(i)
                    .unwrap()
                    .blocks()
                    .for_each(|b| provider_mgr.add_variable(b));
            }
            let expected: Vec<Block> = hcl::parse(test_case.expected)
                .unwrap()
                .into_blocks()
                .collect();
            assert_eq!(provider_mgr.variables(), expected);
        }
    }

    #[cfg(test)]
    mod test_locals {
        use super::*;

        #[test]
        fn add_one() {
            test_locals(TestCase {
                inputs: vec![
                    r#"
                        locals {
                            kalle = "one"
                        }
                    "#,
                ],
                expected: r#"
                    locals {
                        kalle = "one"
                    }
                "#,
            });
        }

        #[test]
        fn add_two_from_same_input() {
            test_locals(TestCase {
                inputs: vec![
                    r#"
                        locals {
                            kalle = "one"
                            olle = "two"
                        }
                    "#,
                ],
                expected: r#"
                    locals {
                        kalle = "one"
                        olle = "two"
                    }
                "#,
            });
        }

        #[test]
        fn add_two_from_different_input() {
            test_locals(TestCase {
                inputs: vec![
                    r#"
                        locals {
                            kalle = "one"
                        }
                    "#,
                    r#"
                        locals {
                            olle = "two"
                        }
                    "#,
                ],
                expected: r#"
                    locals {
                        kalle = "one"
                        olle = "two"
                    }
                "#,
            });
        }

        #[test]
        fn only_locals() {
            test_locals(TestCase {
                inputs: vec![
                    r#"
                        locals {
                            kalle = "one"
                        }
                    "#,
                    r#"
                        variable "test" {
                            type=string
                        }
                    "#,
                ],
                expected: r#"
                    locals {
                        kalle = "one"
                    }
                "#,
            });
        }

        #[test]
        fn no_duplicates() {
            test_locals(TestCase {
                inputs: vec![
                    r#"
                        locals {
                            kalle = "one"
                        }
                    "#,
                    r#"
                        locals {
                            kalle = "two"
                        }
                    "#,
                ],
                expected: r#"
                    locals {
                        kalle = "one"
                    }
                "#,
            });
        }

        #[test]
        fn complex() {
            test_locals(TestCase {
                inputs: vec![
                    r#"
                        locals {
                            kalle = "one"
                        }
                    "#,
                    r#"
                        locals {
                            subnet_cidrs = [
                                for i in range(local.az_count) :
                                cidrsubnet(local.vpc_cidr, 8, i)
                            ]
                        }
                    "#,
                ],
                expected: r#"
                    locals {
                        kalle = "one"
                        subnet_cidrs = [
                            for i in range(local.az_count) :
                            cidrsubnet(local.vpc_cidr, 8, i)
                        ]
                    }
                "#,
            });
        }

        fn test_locals(test_case: TestCase) {
            let mut provider_mgr = TfProviderMgmt::new();
            for i in test_case.inputs {
                hcl::parse(i)
                    .unwrap()
                    .blocks()
                    .for_each(|b| provider_mgr.add_locals(b));
            }
            let expected: Vec<Block> = hcl::parse(test_case.expected)
                .unwrap()
                .into_blocks()
                .collect();
            assert_eq!(provider_mgr.locals(), expected)
        }
    }

    #[cfg(test)]
    mod test_output {
        use super::*;

        #[test]
        fn only_output() {
            test_output(TestCase {
                inputs: vec![
                    r#"
                        output "instance_public_ip" {
                            value = aws_instance.web.public_ip
                            description = "Public IP address of the instance."

                            precondition {
                                condition     = length([for rule in aws_security_group.web.ingress : rule if rule.to_port == 80 || rule.to_port == 443]) > 0
                                error_message = "Security group must allow HTTP (port 80) or HTTPS (port 443) ingress traffic."
                            }
                        }

                        variable "session_token" {
                            type      = string
                            ephemeral = true
                        }
                    "#,
                ],
                expected: r#"
                    output "instance_public_ip" {
                        value = aws_instance.web.public_ip
                        description = "Public IP address of the instance."

                        precondition {
                            condition     = length([for rule in aws_security_group.web.ingress : rule if rule.to_port == 80 || rule.to_port == 443]) > 0
                            error_message = "Security group must allow HTTP (port 80) or HTTPS (port 443) ingress traffic."
                        }
                    }
                "#,
            });
        }

        #[test]
        fn no_duplicates() {
            test_output(TestCase {
                inputs: vec![
                    r#"
                        output "instance_public_ip" {
                            value = aws_instance.web.public_ip
                            description = "Public IP address of the instance."

                            precondition {
                                condition     = length([for rule in aws_security_group.web.ingress : rule if rule.to_port == 80 || rule.to_port == 443]) > 0
                                error_message = "Security group must allow HTTP (port 80) or HTTPS (port 443) ingress traffic."
                            }
                        }
                    "#,
                    r#"
                        output "instance_public_ip" {
                            value = aws_instance.web.public_ip
                            description = "Public IP address of the instance."

                            precondition {
                                condition     = length([for rule in aws_security_group.web.ingress : rule if rule.to_port == 80 || rule.to_port == 443]) > 0
                                error_message = "Security group must allow HTTP (port 80) or HTTPS (port 443) ingress traffic."
                            }
                        }
                    "#,
                ],
                expected: r#"
                    output "instance_public_ip" {
                        value = aws_instance.web.public_ip
                        description = "Public IP address of the instance."

                        precondition {
                            condition     = length([for rule in aws_security_group.web.ingress : rule if rule.to_port == 80 || rule.to_port == 443]) > 0
                            error_message = "Security group must allow HTTP (port 80) or HTTPS (port 443) ingress traffic."
                        }
                    }
                "#,
            });
        }

        fn test_output(test_case: TestCase) {
            let mut provider_mgr = TfProviderMgmt::new();
            for i in test_case.inputs {
                hcl::parse(i)
                    .unwrap()
                    .blocks()
                    .for_each(|b| provider_mgr.add_output(b));
            }
            let expected: Vec<Block> = hcl::parse(test_case.expected)
                .unwrap()
                .into_blocks()
                .collect();
            assert_eq!(provider_mgr.output(), expected);
        }
    }

    #[cfg(test)]
    mod test_data {
        use super::*;

        #[test]
        fn only_data() {
            test_data(TestCase {
                inputs: vec![
                    r#"
                        data "aws_ami" "example_ami" {
                            most_recent = true
                            owners      = ["amazon"]
                            filter {
                                name   = "name"
                                values = ["amzn2-ami-hvm-*"]
                            }
                            filter {
                                name   = "architecture"
                                values = ["x86_64"]
                            }
                            depends_on = [aws_subnet.example_subnet]
                        }
                        variable "session_token" {
                            type      = string
                            ephemeral = true
                        }
                    "#,
                ],
                expected: r#"
                    data "aws_ami" "example_ami" {
                        most_recent = true
                        owners      = ["amazon"]
                        filter {
                            name   = "name"
                            values = ["amzn2-ami-hvm-*"]
                        }
                        filter {
                            name   = "architecture"
                            values = ["x86_64"]
                        }
                        depends_on = [aws_subnet.example_subnet]
                    }
                "#,
            });
        }

        #[test]
        fn no_duplicates() {
            test_data(TestCase {
                inputs: vec![
                    r#"
                        data "aws_ami" "example_ami" {
                            most_recent = true
                            owners      = ["amazon"]
                            filter {
                                name   = "name"
                                values = ["amzn2-ami-hvm-*"]
                            }
                            filter {
                                name   = "architecture"
                                values = ["x86_64"]
                            }
                            depends_on = [aws_subnet.example_subnet]
                        }
                    "#,
                    r#"
                        data "aws_ami" "example_ami" {
                            most_recent = true
                            owners      = ["amazon"]
                            filter {
                                name   = "name"
                                values = ["amzn2-ami-hvm-*"]
                            }
                            filter {
                                name   = "architecture"
                                values = ["x86_64"]
                            }
                            depends_on = [aws_subnet.example_subnet]
                        }
                    "#,
                ],
                expected: r#"
                    data "aws_ami" "example_ami" {
                        most_recent = true
                        owners      = ["amazon"]
                        filter {
                            name   = "name"
                            values = ["amzn2-ami-hvm-*"]
                        }
                        filter {
                            name   = "architecture"
                            values = ["x86_64"]
                        }
                        depends_on = [aws_subnet.example_subnet]
                    }
                "#,
            });
        }

        fn test_data(test_case: TestCase) {
            let mut provider_mgr = TfProviderMgmt::new();
            for i in test_case.inputs {
                hcl::parse(i)
                    .unwrap()
                    .blocks()
                    .for_each(|b| provider_mgr.add_data(b));
            }
            let expected: Vec<Block> = hcl::parse(test_case.expected)
                .unwrap()
                .into_blocks()
                .collect();
            assert_eq!(provider_mgr.data(), expected);
        }
    }

    #[cfg(test)]
    mod test_add_and_build {
        use super::*;

        #[test]
        fn all_blocks() {
            test_add_and_build(TestCase {
                inputs: vec![
                    r#"
                        terraform {
                            required_version = "~>1.0"
                        }
                    "#,
                    r#"
                        terraform {
                            required_providers {
                                aws = {
                                    version = ">= 2.5.0"
                                    source = "hashicorp/aws"
                                }
                            }
                        }
                    "#,
                    r#"
                        data "aws_ami" "example_ami" {
                            most_recent = true
                            owners      = ["amazon"]
                            filter {
                                name   = "name"
                                values = ["amzn2-ami-hvm-*"]
                            }
                            filter {
                                name   = "architecture"
                                values = ["x86_64"]
                            }
                            depends_on = [aws_subnet.example_subnet]
                        }
                    "#,
                    r#"
                        variable "session_token" {
                            type      = string
                            ephemeral = true
                        }
                    "#,
                    r#"
                        output "instance_public_ip" {
                            value = aws_instance.web.public_ip
                            description = "Public IP address of the instance."

                            precondition {
                                condition     = length([for rule in aws_security_group.web.ingress : rule if rule.to_port == 80 || rule.to_port == 443]) > 0
                                error_message = "Security group must allow HTTP (port 80) or HTTPS (port 443) ingress traffic."
                            }
                        }
                    "#,
                    r#"
                        locals {
                            kalle = "one"
                            subnet_cidrs = [
                                for i in range(local.az_count) :
                                cidrsubnet(local.vpc_cidr, 8, i)
                            ]
                        }
                    "#,
                    r#"
                        provider "aws" {
                            region     = "us-west-2"
                            access_key = "my-access-key"
                            secret_key = "my-secret-key"
                        }
                    "#,
                ],
                expected: r#"
                    terraform {
                        required_version = "~>1.0"
                        required_providers {
                                aws = {
                                    version = ">= 2.5.0"
                                    source = "hashicorp/aws"
                                }
                            }
                    }
                    provider "aws" {
                        region     = "us-west-2"
                        access_key = "my-access-key"
                        secret_key = "my-secret-key"
                    }
                    locals {
                        kalle = "one"
                        subnet_cidrs = [
                            for i in range(local.az_count) :
                            cidrsubnet(local.vpc_cidr, 8, i)
                        ]
                    }
                    variable "session_token" {
                        type      = string
                        ephemeral = true
                    }
                    data "aws_ami" "example_ami" {
                        most_recent = true
                        owners      = ["amazon"]
                        filter {
                            name   = "name"
                            values = ["amzn2-ami-hvm-*"]
                        }
                        filter {
                            name   = "architecture"
                            values = ["x86_64"]
                        }
                        depends_on = [aws_subnet.example_subnet]
                    }
                    output "instance_public_ip" {
                        value = aws_instance.web.public_ip
                        description = "Public IP address of the instance."

                        precondition {
                            condition     = length([for rule in aws_security_group.web.ingress : rule if rule.to_port == 80 || rule.to_port == 443]) > 0
                            error_message = "Security group must allow HTTP (port 80) or HTTPS (port 443) ingress traffic."
                        }
                    }
                "#,
            });
        }

        fn test_add_and_build(test_case: TestCase) {
            let mut provider_mgr = TfProviderMgmt::new();
            for i in test_case.inputs {
                hcl::parse(i)
                    .unwrap()
                    .blocks()
                    .for_each(|b| provider_mgr.add_block(b));
            }
            let expected = hcl::parse(test_case.expected).unwrap();
            assert_eq!(provider_mgr.build(), expected);
        }
    }
}
