from source.generator import run
from source.input import ModuleDef

module_library = {
    'S3Bucket': [
        ModuleDef(
            module_name="S3Bucket",
            description="This module creates an S3 bucket",
            version="0.0.4",
            timestamp="2024-04-21T18:25:53Z",
            tf_outputs=[],
            tf_variables=[
                {
                    "default": "my-s3-bucket",
                    "description": "",
                    "name": "bucket_name",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "",
                    "name": "environment",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "",
                    "name": "deployment_id",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                }
            ]
        ),
        ModuleDef(
            module_name="S3Bucket",
            description="This module creates an S3 bucket",
            version="0.0.5",
            timestamp="2024-04-21T18:25:53Z",
            tf_outputs=[],
            tf_variables=[
                {
                    "default": "my-s3-bucket",
                    "description": "",
                    "name": "bucket_name",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "",
                    "name": "environment",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "value of the module name",
                    "name": "module_name",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "",
                    "name": "region",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "",
                    "name": "deployment_id",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                }
            ]
        )
    ],
    'IAMRole': [
        ModuleDef(
            module_name="IAMRole",
            description="This module creates an IAM Role",
            version="0.2.3",
            timestamp="2024-04-21T18:25:53Z",
            tf_outputs=[],
            tf_variables=[
                {
                    "default": "my-s3-bucket",
                    "description": "",
                    "name": "bucket_name",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "description": "",
                    "name": "environment",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "",
                    "name": "deployment_id",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                }
            ]
        ),
        ModuleDef(
            module_name="IAMRole",
            description="This module creates an IAM Role",
            version="0.2.4",
            timestamp="2024-04-21T18:25:53Z",
            tf_outputs=[],
            tf_variables=[
                {
                    "default": "my-s3-bucket",
                    "description": "",
                    "name": "bucket_name",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "",
                    "name": "environment",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "value of the module name",
                    "name": "module_name",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "",
                    "name": "region",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                },
                {
                    "default": "",
                    "description": "",
                    "name": "deployment_id",
                    "nullable": True,
                    "sensitive": False,
                    "type": "string"
                }
            ]
        )
    ],
}


run(module_library)
