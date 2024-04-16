import re
import os
import tempfile
import hcl2
from pydantic import BaseModel, create_model, Field
from io import StringIO
from datamodel_code_generator import InputFileType, generate
import io
from contextlib import redirect_stdout
from sphinx.cmd.build import build_main
import sys
import subprocess
import zipfile

def zip_folder(folder_path, output_path):
    with zipfile.ZipFile(output_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
        for root, dirs, files in os.walk(folder_path):
            for file in files:
                file_path = os.path.join(root, file)
                arcname = os.path.relpath(file_path, os.path.dirname(folder_path))
                zipf.write(file_path, arcname=arcname)

def run_terraform_docs_from_string(modeule_name, tf_config_string):
    with tempfile.TemporaryDirectory() as tmpdirname:
        tf_filename = os.path.join(tmpdirname, 'main.tf')
        with open(tf_filename, 'w') as file:
            file.write(tf_config_string)
        command = 'terraform-docs'
        args = ['markdown', tmpdirname]
        try:
            result = subprocess.run([command] + args, capture_output=True, text=True, check=True)
            markdown = result.stdout
        except subprocess.CalledProcessError as e:
            print("An error occurred while running terraform-docs:")
            print(e.stderr)
            markdown = ""
        markdown = f"# {modeule_name}\n{markdown}"
        return markdown


def parse_terraform_variable_type(tf_type):
    """Map Terraform type strings to Python types."""
    type_mappings = {
        'string': str,
        'number': float,
        'bool': bool,
        'list': list,       # Example: `list(string)` or simply `list`
        'map': dict,        # Example: `map(string)` or simply `map`
        # Custom handling for object and tuple might require defining specific Pydantic models
    }
    # Extract base type without specific type details (e.g., list(string) -> list)
    base_type = tf_type.split('(')[0].strip()
    return type_mappings.get(base_type, str)  # Default to str if type not mapped


def parse_terraform_file(file_path):
    """Parse a Terraform configuration file to extract variables along with their types, defaults, and descriptions."""
    with open(file_path, 'r') as file:
        data = hcl2.load(file)
        variables = {}
        for item in data.get('variable', []):
            var_name = list(item.keys())[0]
            details = list(item.values())[0]
            var_type = details.get('type', 'string')
            default_value = details.get('default', ...)
            description = details.get('description', '')  # Extract description
            python_type = parse_terraform_variable_type(var_type)
            variables[var_name] = (python_type, default_value, description)
        return variables


def generate_pydantic_class(variables, class_name="TerraformInputs"):
    """Generate a Pydantic model from Terraform variables, including default values and descriptions."""
    fields = {
        var_name: (type_info[0], Field(default=type_info[1], description=type_info[2]))
        for var_name, type_info in variables.items()
    }
    return create_model(class_name, **fields)


def read_directory(directory_path):
    """Read all .tf files in a directory and aggregate their variable definitions with default values."""
    aggregated_variables = {}
    for filename in os.listdir(directory_path):
        if filename.endswith('.tf'):
            file_path = os.path.join(directory_path, filename)
            variables = parse_terraform_file(file_path)
            aggregated_variables.update(variables)
    return aggregated_variables


def read_text(hcl_string):
    # Use StringIO to create a file-like object
    file = StringIO(hcl_string)
    data = hcl2.load(file)
    variables = {}
    for item in data.get('variable', []):
        var_name = list(item.keys())[0]
        details = list(item.values())[0]
        var_type = details.get('type', 'string')
        default_value = details.get('default', ...)
        description = details.get('description', '')
        python_type = parse_terraform_variable_type(var_type)
        variables[var_name] = (python_type, default_value, description)
    return variables

def convert_json_schema_to_pydantic(json_schema_str):
    f = io.StringIO()
    with redirect_stdout(f):
        generate(
            json_schema_str,
            input_file_type=InputFileType.JsonSchema,
        )
    return replace_base_model(f.getvalue())

def replace_base_model(code: str, new_base: str = "CustomBaseModel", original_base: str = "BaseModel"):
    # Replace the BaseModel in class definitions
    code = re.sub(rf"(class \s*\w+\s*\(){original_base}(\))", rf"\1{new_base}\2", code)

    # Function to handle the replacement of the import statement
    def replace_import(match):
        imports = match.group(2).split(',')
        # Remove spaces and filter out the original_base
        filtered_imports = [imp.strip() for imp in imports if imp.strip() != original_base]
        if filtered_imports:
            return f"from pydantic import {', '.join(filtered_imports)}\nfrom {new_base.lower()} import {new_base}"
        else:
            return f"from {new_base.lower()} import {new_base}"

    # Replace or modify the import statement while keeping other imports from pydantic
    code = re.sub(rf"(from \s*pydantic \s*import)(.*\b{original_base}\b.*)", replace_import, code)
    return code

def convert_tf_to_json_schema(module_name, hcl_string):
    variables = read_text(hcl_string)
    InputsModel = generate_pydantic_class(variables, class_name=module_name)
    return InputsModel.schema_json(indent=2)

# module_name e.g. "S3Bucket"
def convert_tf_to_py(module_name, hcl_string):
    json_schema_str=convert_tf_to_json_schema(module_name, hcl_string)
    print(convert_json_schema_to_pydantic(json_schema_str))
    return convert_json_schema_to_pydantic(json_schema_str)

get_name = lambda module_name: module_name.lower()

def generate_all_py_files(modules_dict):
    for module_name, module_str in modules_dict.items():
        result = convert_tf_to_py(module_name, module_str)
        with open(f'source/{get_name(module_name)}.py', 'w') as f:
            f.write(result)

def generate_all_rst_files(modules_dict):
    for module_name, module_str in modules_dict.items():
        result = generate_rst(module_name)
        with open(f'source/{get_name(module_name)}.rst', 'w') as f:
            f.write(result)
        print(result)

def generate_all_md_files(modules_dict):
    for module_name, module_str in modules_dict.items():
        result = run_terraform_docs_from_string(module_name, module_str)
        with open(f'source/original_{get_name(module_name)}.md', 'w') as f:
            f.write(result)
        print(result)

def store_index_rst(modules_dict):
    result = index_rst_template(modules_dict)
    with open(f'source/index.rst', 'w') as f:
        f.write(result)
    print(result)

def generate_rst(module_name):
    return rst_template(module_name, "hcl_string")

def generate_webpage():
    # Arguments to be passed to Sphinx
    # Simulating command line arguments: sphinx-build -b html source build
    sys.argv = [
        "sphinx-build",   # command name, doesn't impact execution
        "-b", "html",     # Output format
        "source",         # Source directory
        "build"           # Output directory
    ]
    # Run the Sphinx build
    build_main(sys.argv[1:])  # Pass arguments to function, excluding the command name


rst_template=lambda module_name, hcl_string: f'''
{module_name}
=======

Example
-------

   .. code-block:: python
      :linenos:

      s3 = S3Bucket(
        name="mybucket"
      )


Note
----
.. tip:: This is a **note**.


API Documentation
-----------------

.. autopydantic_model:: {get_name(module_name)}.{module_name}

'''

index_rst_template=lambda modules_dict: f'''
Startpage - Python SDK Documentation
========================

Welcome to the Python SDK documentation. Here you can find information on how to get started, as well as detailed API documentation.

Getting Started
---------------

.. toctree::
   :hidden:
   :maxdepth: 2
   :caption: Getting Started Guide

   installation
   welcome
   markdown

.. toctree::
   :hidden:
   :maxdepth: 2
   :caption: Terraform Modules

   {'\n   '.join(['original_'+get_name(module_name) for module_name in modules_dict.keys()])}

.. toctree::
   :hidden:
   :maxdepth: 2
   :caption: Python SDK Modules

   {'\n   '.join([get_name(module_name) for module_name in modules_dict.keys()])}
'''

if __name__ == "__main__":
    modules_dict = {
        'S3Bucket': '''
    variable "bucket_name" {
    type    = string
    default = "my-s3-bucket"
    }

    variable "environment" {
    type    = string
    }

    variable "module_name" {
    type    = string
    description = "value of the module name"
    }

    variable "region" {
    type    = string
    }

    variable "deployment_id" {
    type    = string
    }
    ''',
           'IamRole': '''
    variable "iamrole_name" {
    type    = string
    default = "iam-role-name-example"
    }

    variable "environment" {
    type    = string
    }

    variable "deployment_id" {
    type    = string
    }
    ''',
           'EKSCluster': '''
    variable "cluster_name" {
    type    = string
    default = "cluster-name-example"
    }

    variable "environment" {
    type    = string
    }

    variable "deployment_id" {
    type    = string
    }
    ''',
}
    generate_all_py_files(modules_dict)
    generate_all_rst_files(modules_dict)
    generate_all_md_files(modules_dict)
    store_index_rst(modules_dict)
    generate_webpage()
    # run_terraform_docs_from_string(modules_dict['S3Bucket'])
    # zip_folder('build', 'build/build.zip')