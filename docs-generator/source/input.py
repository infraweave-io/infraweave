
# {
#  "module": "s3bucket",
#  "environment_version": "dev#000.000.004",
#  "description": "",
#  "environment": "dev",
#  "manifest": {
#   "apiVersion": "infrabridge.io/v1",
#   "kind": "Module",
#   "metadata": {
#    "name": "s3bucket"
#   },
#   "spec": {
#    "moduleName": "S3Bucket",
#    "outputs": [
#     {
#      "name": "bucketArn",
#      "type": "string"
#     }
#    ],
#    "parameters": [
#     {
#      "name": "bucketName",
#      "type": "string"
#     },
#     {
#      "name": "region",
#      "type": "string"
#     },
#     {
#      "name": "policy",
#      "type": "object"
#     }
#    ],
#    "provider": "aws",
#    "source": {
#     "bucket": "terraform-modules-4732h",
#     "path": "s3bucket/release-0.1.0.zip",
#     "type": "S3"
#    },
#    "version": "0.0.4"
#   }
#  },
#  "module_name": "S3Bucket",
#  "reference": "",
#  "s3_key": "s3bucket/s3bucket-0.0.4.zip",
#  "tf_outputs": [
#  ],
#  "tf_variables": [
#   {
#    "default": "my-s3-bucket",
#    "description": "",
#    "name": "bucket_name",
#    "nullable": true,
#    "sensitive": false,
#    "type": "string"
#   },
#   {
#    "default": "",
#    "description": "",
#    "name": "environment",
#    "nullable": true,
#    "sensitive": false,
#    "type": "string"
#   },
#   {
#    "default": "",
#    "description": "value of the module name",
#    "name": "module_name",
#    "nullable": true,
#    "sensitive": false,
#    "type": "string"
#   },
#   {
#    "default": "",
#    "description": "",
#    "name": "region",
#    "nullable": true,
#    "sensitive": false,
#    "type": "string"
#   },
#   {
#    "default": "",
#    "description": "",
#    "name": "deployment_id",
#    "nullable": true,
#    "sensitive": false,
#    "type": "string"
#   }
#  ],
#  "timestamp": "2024-04-21T18:25:53Z",
#  "version": "0.0.4"
# }

class ModuleDef:
    def __init__(
            self,
            module_name, 
            description, 
            version,
            timestamp, 
            tf_outputs, 
            tf_variables, 
        ):
        self.module_name = module_name
        self.description = description
        self.tf_outputs = tf_outputs
        self.tf_variables = tf_variables
        self.timestamp = timestamp
        self.version = version        
