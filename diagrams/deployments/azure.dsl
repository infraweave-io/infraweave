azure = deploymentEnvironment "Azure" {


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

    deploymentNode "Azure" {
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

                deploymentNode "Azure Storage Accounts" {
                    tags "Microsoft Azure - Storage Accounts"
                
                    containerInstance infrabridge.storage_modules {
                        tags "Microsoft Azure - Blob Block"
                    }

                    containerInstance infrabridge.storage_tf_state {
                        tags "Microsoft Azure - Blob Block"
                    }
                }

                deploymentNode "Azure Container Apps" {
                    tags "Microsoft Azure - Container Apps Environments"

                    containerInstance infrabridge.runner_environment {
                        tags "Microsoft Azure - Container Apps Environments"
                    }
                }

                deploymentNode "Amazon Lambda" {
                    tags "Microsoft Azure - Function Apps"
                
                    containerInstance infrabridge.api_layer_modules {
                        tags "Microsoft Azure - Function Apps"
                    }
                
                    containerInstance infrabridge.api_layer_deployments {
                        tags "Microsoft Azure - Function Apps"
                    }
                
                    containerInstance infrabridge.api_layer_events {
                        tags "Microsoft Azure - Function Apps"
                    }
                
                    containerInstance infrabridge.api_layer_environments {
                        tags "Microsoft Azure - Function Apps"
                    }
                
                    containerInstance infrabridge.api_layer_infra {
                        tags "Microsoft Azure - Function Apps"
                    }
                }

                deploymentNode "Cosmos DB" {
                    tags "Microsoft Azure - Azure Cosmos DB"

                    containerInstance infrabridge.database_modules {
                        tags "Microsoft Azure - Azure Cosmos DB"
                    }
                    containerInstance infrabridge.database_events {
                        tags "Microsoft Azure - Azure Cosmos DB"
                    }
                    containerInstance infrabridge.database_environments {
                        tags "Microsoft Azure - Azure Cosmos DB"
                    }
                    containerInstance infrabridge.database_deployments {
                        tags "Microsoft Azure - Azure Cosmos DB"
                    }
                    containerInstance infrabridge.database_tf_locks {
                        tags "Microsoft Azure - Azure Cosmos DB"
                    }
                }
            }
        }
    }
}