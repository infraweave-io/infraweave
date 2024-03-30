locals {
  sanitized_region = replace(lower(var.region), " ", "")
  sanitized_environment = replace(lower(var.environment), " ", "")
}

resource "azuredevops_project" "project" {
  name               = "ExampleProject"
  description        = "An example project"
  visibility         = "private"
  version_control    = "Git"
  work_item_template = "Agile"
}

resource "azuredevops_git_repository" "repo" {
  project_id = azuredevops_project.project.id
  name       = "example-repo"
  initialization {
    init_type = "Clean"
  }
}

resource "azuredevops_git_repository_branch" "example" {
  repository_id = azuredevops_git_repository.repo.id
  name          = "main"
  ref_branch    = azuredevops_git_repository.repo.default_branch
}

resource "azuredevops_build_definition" "build" {
  project_id = azuredevops_project.project.id
  name       = "example-build"
  path       = "\\"
  repository {
    repo_type   = "TfsGit"
    repo_id     = azuredevops_git_repository.repo.id
    branch_name = "main"
    yml_path    = "azure-pipelines.yml"
  }

  ci_trigger {
    use_yaml = true
  }

  variable {
    name    = "DEPLOYMENT_ID"
    value   = "NOT_SET"
  }

  variable {
    name    = "TF_STORAGE_ACCOUNT"
    value   = ""
  }

  variable {
    name    = "TF_CONTAINER"
    value   = ""
  }

  variable {
    name    = "TF_STORAGE_ACCESS_KEY"
    value   = ""
  }

  variable {
    name    = "SIGNAL"
    value   = ""
  }

  variable {
    name    = "EVENT"
    value   = ""
  }

  variable {
    name    = "MODULE_NAME"
    value   = ""
  }

  variable {
    name    = "ENVIRONMENT"
    value   = ""
  }

  variable {
    name    = "REGION"
    value   = ""
  }

  variable {
    name    = "TF_DYNAMODB_TABLE"
    value   = ""
  }

  variable {
    name    = "TF_VARS_JSON"
    value   = ""
  }

  variable {
    name    = "PROJECT_NAME"
    value   = ""
  }
}

## FUNCTION


resource "azurerm_resource_group" "example" {
  name     = "example-resources"
  location = "East US"
}

resource "azurerm_storage_account" "example" {
  name                     = "examplefuncstormar2"
  resource_group_name      = azurerm_resource_group.example.name
  location                 = azurerm_resource_group.example.location
  account_tier             = "Standard"
  account_replication_type = "LRS"
}

resource "azurerm_storage_container" "example" {
  name                  = "function-releases"
  storage_account_name  = azurerm_storage_account.example.name
  container_access_type = "private"
}

resource "azurerm_storage_container" "tf-modules" {
  name                  = "tf-modules"
  storage_account_name  = azurerm_storage_account.example.name
  container_access_type = "private"
}

resource "azurerm_service_plan" "example" {
  name                = "example-function-app-service-plan"
  location            = azurerm_resource_group.example.location
  resource_group_name = azurerm_resource_group.example.name

  sku_name = "Y1"
  os_type = "Linux"
}


### Python 

# Archive your Azure Function
# data "archive_file" "init" {
#   type        = "zip"
#   source_dir  = "${path.module}/functions"
#   output_path = "${path.module}/function.zip"
# }

# Upload the function code to Azure Blob Storage
# resource "azurerm_storage_blob" "example" {
#   name                   = "function.zip"
#   storage_account_name   = azurerm_storage_account.example.name
#   storage_container_name = azurerm_storage_container.example.name
#   type                   = "Block"
#   source                 = "${path.module}/function.zip" #data.archive_file.init.output_path
# }

# resource "azurerm_role_assignment" "example" {
#   scope                = azurerm_storage_account.example.id
#   role_definition_name = "Storage Blob Data Reader"
#   principal_id         = azurerm_function_app.example.identity[0].principal_id
# }

resource "null_resource" "deploy_function_app" {
  triggers = {
    always_run = "${timestamp()}"
  }

  depends_on = [azurerm_function_app.example]

  provisioner "local-exec" {
    command = <<EOF
cd ${path.module}/functions
python3 -m pip install -r requirements.txt --target="./.python_packages/lib/site-packages"
zip -r functionapp.zip . -x "*.git*" "*.vscode*" "*__pycache__*" "*env/*"
az functionapp deployment source config-zip --resource-group ${azurerm_resource_group.example.name} --name ${azurerm_function_app.example.name} --src ./functionapp.zip
rm functionapp.zip
rm -rf .python_packages
EOF
  }
}

# Function App
resource "azurerm_function_app" "example" {
  name                       = "example-function-appmar"
  location                   = azurerm_resource_group.example.location
  resource_group_name        = azurerm_resource_group.example.name
  app_service_plan_id        = azurerm_service_plan.example.id
  storage_account_name       = azurerm_storage_account.example.name
  storage_account_access_key = azurerm_storage_account.example.primary_access_key
  os_type                    = "linux"
  version                    = "~4"

  app_settings = {
    FUNCTIONS_WORKER_RUNTIME     = "python"
    WEBSITE_RUN_FROM_PACKAGE     = "1"

    "organization" = "mariusg94"
    "project"      = azuredevops_project.project.id
    "pipeline_id"   = azuredevops_build_definition.build.id
    "pat"          = var.devops_job_pat

    "tf_storage_account"    = azurerm_storage_account.example.name
    "tf_storage_container"  = azurerm_storage_container.terraform_state.name
    "tf_storage_access_key" = azurerm_storage_account.example.primary_access_key
    # "tf_storage_table"      = azurerm_storage_table.terraform_locks.name
    "deployment_id" = "OVERRIDE_ME_IN_REQUEST"
    "environment"  = var.environment
    "region"       = var.region
    "tf_dynamodb_table"  = azurerm_storage_table.terraform_locks.name

    "STORAGE_TABLE_EVENTS_TABLE_NAME" = azurerm_storage_table.terraform_locks.name

    STORAGE_TABLE_MODULES_TABLE_NAME = azurerm_storage_table.modules.name
    AZURE_MODULES_TABLE_CONN_STR = "DefaultEndpointsProtocol=https;AccountName=${azurerm_storage_account.example.name};AccountKey=${azurerm_storage_account.example.primary_access_key};EndpointSuffix=core.windows.net"

    STORAGE_TABLE_ENVIRONMENTS_TABLE_NAME = azurerm_storage_table.environments.name
    AZURE_ENVIRONMENTS_TABLE_CONN_STR = "DefaultEndpointsProtocol=https;AccountName=${azurerm_storage_account.example.name};AccountKey=${azurerm_storage_account.example.primary_access_key};EndpointSuffix=core.windows.net"


    # Application Insights settings
    APPINSIGHTS_INSTRUMENTATIONKEY = azurerm_application_insights.example.instrumentation_key
    APPLICATIONINSIGHTS_CONNECTION_STRING = "InstrumentationKey=${azurerm_application_insights.example.instrumentation_key}"
    ApplicationInsightsAgent_EXTENSION_VERSION = "~2"
  }

  site_config {
    linux_fx_version = "python|3.11"
  }

  identity {
    type = "SystemAssigned"
  }
}

resource "azurerm_application_insights" "example" {
  name                = "example-appinsights"
  location            = azurerm_resource_group.example.location
  resource_group_name = azurerm_resource_group.example.name
  application_type    = "web"
}

# Generate SAS Token for the blob
data "azurerm_storage_account_sas" "example" {
  connection_string = azurerm_storage_account.example.primary_connection_string
  https_only        = true
  start             = formatdate("YYYY-MM-DD'T'HH:mm:ss'Z'", timeadd(timestamp(), "-1h"))
  expiry            = formatdate("YYYY-MM-DD'T'HH:mm:ss'Z'", timeadd(timestamp(), "1h"))
  resource_types {
    service   = true
    container = false
    object    = true
  }
  services {
    blob  = true
    queue = false
    table = false
    file  = false
  }
  permissions {
    read    = true
    write   = false
    delete  = false
    list    = true
    add     = false
    create  = false
    update  = false
    filter  = false
    tag     = false
    process = false
  }
}

# resource "azurerm_key_vault_access_policy" "example" {
#   key_vault_id = azurerm_key_vault.example.id
#   tenant_id    = data.azurerm_client_config.current.tenant_id
#   object_id    = azurerm_function_app.example.identity[0].principal_id

#   secret_permissions = [
#     "Get",
#   ]
# }

# data "azurerm_client_config" "current" {}

# resource "azurerm_key_vault" "example" {
#   name                = "keyvault-testmarius"
#   location            = azurerm_resource_group.example.location
#   resource_group_name = azurerm_resource_group.example.name
#   tenant_id           = data.azurerm_client_config.current.tenant_id

#   sku_name = "standard"

#   # soft_delete_enabled        = true
#   purge_protection_enabled   = false
# }


# resource "azurerm_key_vault_secret" "devops_pat" {
#   name         = "devops-pat"
#   value        = "your-pat-here"
#   key_vault_id = azurerm_key_vault.example.id
#   # Make sure to treat your PAT securely and consider the implications of storing it in your state file.
# }

# resource "random_id" "example" {
#   keepers = {
#     # Generate a new ID only when a new resource group is specified
#     resource_group = azurerm_resource_group.example.name
#   }

#   byte_length = 8
# }

# State bucket

resource "azurerm_resource_group" "tf_state" {
  name     = "tfstate-resources"
  location = var.region
}

# resource "azurerm_storage_account" "tf_state" {
#   name                     = "tfmar${local.sanitized_region}${local.sanitized_environment}"
#   resource_group_name      = azurerm_resource_group.example.name
#   location                 = azurerm_resource_group.example.location
#   account_tier             = "Standard"
#   account_replication_type = "LRS"
# }

resource "azurerm_storage_container" "terraform_state" {
  name                  = "tfstate${local.sanitized_region}${local.sanitized_environment}"
  storage_account_name  = azurerm_storage_account.example.name
  container_access_type = "private"
}

resource "azurerm_storage_table" "terraform_locks" {
  name                 = "tflocks${local.sanitized_region}${local.sanitized_environment}"
  storage_account_name = azurerm_storage_account.example.name
}

resource "azurerm_storage_table" "events" {
  name                 = "events${local.sanitized_region}${local.sanitized_environment}"
  storage_account_name = azurerm_storage_account.example.name
}

resource "azurerm_storage_table" "modules" {
  name                 = "modules${local.sanitized_region}${local.sanitized_environment}"
  storage_account_name = azurerm_storage_account.example.name
}

resource "azurerm_storage_table" "environments" {
  name                 = "environments${local.sanitized_region}${local.sanitized_environment}"
  storage_account_name = azurerm_storage_account.example.name
}
