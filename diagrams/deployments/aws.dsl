
aws = deploymentEnvironment "AWS" {


    deploymentNode "Python script" {
        
        containerInstance infrabridge.interfaces_python
    }

    deploymentNode "CLI" {
        containerInstance infrabridge.interfaces_cli
    }

    deploymentNode "Kubernetes" {
        tags "Kubernetes - api"

        deploymentNode "Namespace" {
            tags "Kubernetes - ns"

            # deploymentNode "Pod" {
            #     tags "Kubernetes - pod"

            #     # containerInstance cli
            # }

            deploymentNode "CRD" {
                tags "Kubernetes - crd"

                containerInstance infrabridge.kubernetes_crd
            }

            deploymentNode "Controller" {
                tags "Kubernetes - pod"

                containerInstance infrabridge.kubernetes_controller
            }

            # deploymentNode "Other Pods" {
            #     tags "Kubernetes - pod"

            #     containerInstance kubernetes_other_pod
            # }
        }
    }

    deploymentNode "AWS" {
        tags "Amazon Web Services - Cloud"

        deploymentNode "AWS Account" {
            tags "Amazon Web Services - AWS Organizations Account"
                
            deploymentNode "us-east-1" {
                tags "Amazon Web Services - Region"
            
                # route53 = infrastructureNode "Route 53" {
                #     tags "Amazon Web Services - Route 53"
                # }
                # elb = infrastructureNode "Elastic Load Balancer" {
                #     tags "Amazon Web Services - Elastic Load Balancing"
                # }

                aws_s3 = deploymentNode "Amazon S3" {
                    tags "Amazon Web Services - Simple Storage Service"
                
                    containerInstance infrabridge.storage_modules {
                        tags "Amazon Web Services - Simple Storage Service Bucket"
                    }

                    containerInstance infrabridge.storage_tf_state {
                        tags "Amazon Web Services - Simple Storage Service Bucket"
                    }
                }

                aws_ecs = deploymentNode "Amazon ECS" {
                    tags "Amazon Web Services - Elastic Container Service"

                    containerInstance infrabridge.runner_environment {
                        tags "Amazon Web Services - Elastic Container Service Task"
                    }
                }

                aws_lambda = deploymentNode "Amazon Lambda" {
                    tags "Amazon Web Services - Lambda"
                
                    lambda_api_layer_modules = containerInstance infrabridge.api_layer_modules {
                        tags "Amazon Web Services - AWS Lambda Lambda Function"
                    }
                
                    lambda_api_layer_deployments = containerInstance infrabridge.api_layer_deployments {
                        tags "Amazon Web Services - AWS Lambda Lambda Function"
                    }
                
                    lambda_api_layer_events = containerInstance infrabridge.api_layer_events {
                        tags "Amazon Web Services - AWS Lambda Lambda Function"
                    }
                
                    lambda_api_layer_environments = containerInstance infrabridge.api_layer_environments {
                        tags "Amazon Web Services - AWS Lambda Lambda Function"
                    }
                
                    lambda_api_layer_infra = containerInstance infrabridge.api_layer_infra {
                        tags "Amazon Web Services - AWS Lambda Lambda Function"
                    }
                }

                aws_dynamodb = deploymentNode "Amazon DynamoDB" {
                    tags "Amazon Web Services - DynamoDB"

                    dynamodb_database_modules = containerInstance infrabridge.database_modules {
                        tags "Amazon Web Services - DynamoDB Table"
                    }
                    containerInstance infrabridge.database_events {
                        tags "Amazon Web Services - DynamoDB Table"
                    }
                    containerInstance infrabridge.database_environments {
                        tags "Amazon Web Services - DynamoDB Table"
                    }
                    containerInstance infrabridge.database_deployments {
                        tags "Amazon Web Services - DynamoDB Table"
                    }
                    containerInstance infrabridge.database_tf_locks {
                        tags "Amazon Web Services - DynamoDB Table"
                    }
                }
            }
        }
    }
}