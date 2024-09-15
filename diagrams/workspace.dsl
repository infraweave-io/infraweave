workspace {

    !identifiers hierarchical

    model {
        # a = softwareSystem "A"
        genericApplication = softwareSystem "Application X"{
            description "A generic application"
            tags "Generic Application"
            app = container "InfraBridge CLI" "" "Spring Boot"
            appdatabase = container "Database" "" "Relational database schema"
        }
        infrabridge = softwareSystem "InfraBridge Platform" "A platform for hosting modules and deploying infrastructure"{

            group "Databases" {
                database_deployments = container "DB - Deployments" {
                    tags "NoSQL Database"
                    description "Stores information about deployments"
                }
                database_environments = container "DB - Environments"{
                    tags "NoSQL Database"
                    description "Stores information about environments"
                }
                database_events = container "DB - Events" {
                    tags "NoSQL Database"
                    description "Stores information about events"
                }
                database_modules = container "DB - Modules" {
                    tags "NoSQL Database"
                    description "Stores information about modules"
                }
                database_tf_locks = container "DB - TF Locks" {
                    tags "NoSQL Database"
                    description "Stores information about Terraform locks"
                }
            }

            group "Storage" {
                storage_modules = container "Storage - Modules" {
                    tags "Storage"
                    description "Stores TF modules"
                }
                storage_tf_state = container "Storage - TF State" {
                    tags "Storage"
                    description "Stores TF state"
                }
            }

            group "Runner environment" {
                runner_environment = container "Runner Environment" {
                    tags "Runner"
                    description "Environment for running modules"

                    -> infrabridge.storage_modules "Reads terraform modules from"
                    -> infrabridge.database_tf_locks "Temporarily locks terraform state in"
                    -> infrabridge.storage_tf_state "Reads and stores current terraform state in"
                    -> infrabridge.database_deployments "Stores information about current deployment in"
                    -> infrabridge.database_events "Stores information about current events in"
                }
            }

            group "API layer" {
                api_layer_modules = container "API - Modules" {
                    tags "API"
                    description "API for modules"

                    -> infrabridge.database_modules "Reads from and writes module metadata to"
                    -> infrabridge.storage_modules "Stores terraform modules in"
            
                    perspectives {
                        "Ownership" "Team 1"
                    }
                }

                api_layer_events = container "API - Events" {
                    tags "API"
                    description "API for events"
                    perspectives {
                        "Ownership" "Team 1"
                        "Developer Team" "Deployment"
                    }
                }

                api_layer_deployments = container "API - Deployments" {
                    tags "API"
                    description "API for deployments"

                    -> infrabridge.database_deployments "Stores information about deployments in"

                    perspectives {
                        "Ownership" "Team 2"
                    }
                }

                api_layer_environments = container "API - Environments" {
                    tags "API"
                    description "API for environments"
                    perspectives {
                        "Ownership" "Team 2"
                    }
                }

                api_layer_infra = container "API - Infrastructure" {
                    tags "API"
                    description "API for infra management"

                    -> infrabridge.runner_environment "Runs terraform modules in"
                }
            }

            group "Kubernetes" {
                kubernetes_crd = container "CRD" {
                    # tags "Kubernetes"
                    description "Custom Resource Definition"
                }
                kubernetes_controller = container "Controller" {
                    # tags "Kubernetes"
                    description "Controller for InfraBridge"

                    -> infrabridge.api_layer_deployments "Request API to store deployment information"
                    -> infrabridge.kubernetes_crd "Handles CRD management"
                }
                kubernetes_other_pod = container "Other Pods" {
                    # tags "Kubernetes"
                    description "Other pods"
                }
            }

            
            crd_templator = container "CRD Templator" {
                # tags "Kubernetes"
                description "Custom Resource Definition"

                group "InfraBridge CRD Templator" {
                    !include lib/crd_templator.dsl
                }
            }

            group "InfraBridge Interfaces" {
                interfaces_cli = container "CLI" {
                    tags "Interfaces"
                    description "InfraBridge command line interface"
                    technology "Rust"

                    group "environment" {
                        !include lib/env_libraries.dsl
                    }

                    group "cli/main.rs" {
                        cli_main = component "cli/main.rs" {
                            description "Used to interface with InfraBridge"
                            technology "Rust"
                            -> env_deployments "Uses"
                            -> env_environments "Sets up"
                            -> env_infra "Runs"
                            -> env_module "Uses"
                            -> env_resources "Uses"
                            -> env_status "Uses"
                        }
                    }

                    -> infrabridge.api_layer_deployments "Request API to store deployment information"

                    perspectives {
                        "Developer Team" "Deployment"
                        "Platform Team" "Publishing modules"
                    }
                }

                interfaces_python = container "Python Module" {
                    tags "Interfaces"
                    description "InfraBridge Python Module"
                    technology "Rust"

                    group "environment" {
                        !include lib/env_libraries.dsl
                    }

                    group "module/__main__.py" {
                        cli_main = component "module/__main__.py" {
                            description "Used to interface with InfraBridge"
                            technology "Python"
                            -> env_deployments "Uses"
                            -> env_environments "Sets up"
                            -> env_infra "Runs"
                            -> env_module "Uses"
                            -> env_resources "Uses"
                            -> env_status "Uses"
                        }
                    }

                    -> infrabridge.api_layer_deployments "Request API to store deployment information"

                    perspectives {
                        "Developer Team" "Deployment"
                    }
                }

                interfaces_kubernetes = container "Kubernetes" {
                    # tags "Interfaces"
                    description "InfraBridge Kubernetes Operator"
                    technology "Rust"

                    group "environment" {
                        !include lib/env_libraries.dsl
                    }

                    group "crd_templator" {
                        !include lib/crd_templator.dsl
                    }

                    group "infrabridge_operator" {
                        main = component "infrabridge_operator/main.rs" {
                            description "Used to interface with InfraBridge"
                            technology "Rust"
                            -> env_deployments "Uses"
                            -> env_environments "Sets up"
                            -> env_infra "Runs"
                            -> env_module "Uses"
                            -> env_resources "Uses"
                            -> env_status "Uses"
                        }
                        crd = component "infrabridge_operator/crd.rs" {
                            description "Used to interface with InfraBridge"
                            technology "Rust"
                            # -> env_deployments "Uses"
                            # -> env_environments "Sets up"
                            # -> env_status "Uses"
                            -> crd_templator_generate "Generates"
                            -> crd_templator_read "Reads"
                        }
                    }

                    perspectives {
                        "Developer Team" "Deployment"
                    }
                }
            }

            -> genericApplication "Deploys infrastructure for"
        }
        developmentTeam = person "Development team" {
            description "A team developming applications that uses modules from the platform"
        
            -> infrabridge "Deploys infrastructure using"
            -> genericApplication "Develops"

            -> infrabridge.interfaces_cli "Deploys infrastructure using"
            -> infrabridge.interfaces_python "Deploys infrastructure using"
            -> infrabridge.interfaces_kubernetes "Deploys infrastructure using"


            perspectives {
                "Developer Team" "Deployment"
            }
        }
        platformTeam = person "Platform team" {
            description "A team responsible for the platform"

            -> infrabridge "Maintains and adds modules to"
            -> infrabridge.interfaces_cli "Publish modules using"

            perspectives {
                "Platform Team" "Publishing modules"
            }
        }

        # Relationships
        
        !include deployments/aws.dsl

        !include deployments/azure.dsl
    }

    

    views {

        systemContext infrabridge "Overview" {
            include *
            # autoLayout lr
        }

        deployment infrabridge aws "AWS" {
            include *
            # autoLayout lr
        }

        deployment infrabridge azure "Azure" {
            include *
            # autoLayout lr
        }

        # deployment infrabridge kubernetes "Kubernetes" {
        #     include *
        #     # autoLayout lr
        # }

        container infrabridge "Application" {
            include *
            exclude infrabridge.kubernetes_crd
            exclude infrabridge.kubernetes_controller
            exclude infrabridge.kubernetes_other_pod
            # autolayout lr
        }

        component infrabridge.interfaces_cli "CLI_Application" {
            include *
            # autoLayout lr
        }

        component infrabridge.interfaces_kubernetes "Kubernetes_Application" {
            include *
            # autoLayout lr
        }

        component infrabridge.interfaces_python "Python_Module" {
            include *
            # autoLayout lr
        }

        styles {
            element "Element" {
                color white
            }
            element "Generic Application" {
                background #111166
                shape roundedbox
            }
            element "cli" {
                background #116611
                shape roundedbox
            }
            element "Interfaces" {
                background #116611
                shape window
            }
            element "API" {
                # background #2D882D
                shape roundedbox
            }
            element "Storage" {
                background #eeeeee
                shape cylinder
            }
            element "Person" {
                background #116611
                shape person
            }
            element "Software System" {
                background #2D882D
            }
            element "NoSQL Database" {
                background #eeeeee
                shape cylinder
            }
            element "Amazon Web Services - Region" {
                background #2D882D
                icon https://static.structurizr.com/themes/amazon-web-services-2020.04.30/Region_light-bg@4x.png
                color #147eba
                stroke #147eba
            }
            element "Amazon Web Services - Cloud" {
                background #2D882D
                icon https://static.structurizr.com/themes/amazon-web-services-2020.04.30/AWS-Cloud_light-bg@4x.png
                color #232f3e
                stroke #232f3e
            }
        }

        theme https://static.structurizr.com/themes/amazon-web-services-2023.01.31/theme.json
        theme https://static.structurizr.com/themes/kubernetes-v0.3/theme.json
        theme https://static.structurizr.com/themes/microsoft-azure-2023.01.24/theme.json
    }
    
}
