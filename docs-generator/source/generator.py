import re
import os
from pydantic import create_model, Field
from datamodel_code_generator import InputFileType, generate
import io
from contextlib import redirect_stdout
from sphinx.cmd.build import build_main
import sys
import zipfile
import os
import time
import shutil

from .gen_utils import get_name, get_filename, convert_tf_to_json_schema, convert_tf_module_to_json_schema
from .gen_module_python import python_template
from .gen_module_tf import tf_template
from .gen_module_kubernetes import kubernetes_template
from .gen_module_cli import cli_template
from .gen_index import index_rst_template

# Set the timezone to UTC
os.environ['TZ'] = 'UTC'
time.tzset()

def zip_folder(folder_path, output_path):
    with zipfile.ZipFile(output_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
        for root, dirs, files in os.walk(folder_path):
            for file in files:
                file_path = os.path.join(root, file)
                arcname = os.path.relpath(file_path, os.path.dirname(folder_path))
                zipf.write(file_path, arcname=arcname)

# def run_terraform_docs_from_string(modeule_name, tf_config_string):
#     with tempfile.TemporaryDirectory() as tmpdirname:
#         tf_filename = os.path.join(tmpdirname, 'main.tf')
#         with open(tf_filename, 'w') as file:
#             file.write(tf_config_string)
#         command = 'terraform-docs'
#         args = ['markdown', tmpdirname]
#         try:
#             result = subprocess.run([command] + args, capture_output=True, text=True, check=True)
#             markdown = result.stdout
#         except subprocess.CalledProcessError as e:
#             print("An error occurred while running terraform-docs:")
#             print(e.stderr)
#             markdown = ""
#         markdown = f"# {modeule_name}\n{markdown}"
#         return markdown

def generate_pydantic_class(variables, class_name="TerraformInputs"):
    """Generate a Pydantic model from Terraform variables, including default values and descriptions."""
    fields = {
        var_name: (type_info[0], Field(default=type_info[1], description=type_info[2]))
        for var_name, type_info in variables.items()
    }
    return create_model(class_name, **fields)

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

# module_name e.g. "S3Bucket"
# def convert_tf_to_py(module_name, hcl_string):
#     json_schema_str=convert_tf_to_json_schema(module_name, hcl_string)
#     print(json_schema_str)
#     print(convert_json_schema_to_pydantic(json_schema_str))
#     return convert_json_schema_to_pydantic(json_schema_str)

def convert_tf_module_to_py(module_name, module_json):
    json_schema_str=convert_tf_module_to_json_schema(module_name, module_json)
    print(json_schema_str)
    print(convert_json_schema_to_pydantic(json_schema_str))
    return convert_json_schema_to_pydantic(json_schema_str)

def generate_all_py_modules(module_library):
    for module_name, module_list in module_library.items():
        for module in module_list:
            ensure_directory(f'/tmp/source/python/{get_filename(module)}')
            module_json = module.tf_variables
            result = convert_tf_module_to_py(module_name, module_json)
            print(f'storing py file: {result}')
            print(f'/tmp/source/python/{get_filename(module)}/{get_name(module.module_name)}.py')
            with open(f'/tmp/source/python/{get_filename(module)}/{get_name(module.module_name)}.py', 'w') as f:
                f.write(result)

def generate_all_python_docs(module_library):
    for module_name, module_list in module_library.items():
        ensure_directory(f'/tmp/source/python/{get_name(module_name)}')
        latest_version_text = python_template(module_list[-1], module_list, show_toc=True)
        with open(f'/tmp/source/python/{get_name(module_name)}/index.rst', 'w') as f:
            f.write(latest_version_text)
        for module in module_list:
            ensure_directory(f'/tmp/source/python/{get_name(module_name)}')
            print(f'generating python docs for module:')
            result = python_template(module, module_list, show_toc=False)
            print(f'storing rst file: {result}')
            print(f'/tmp/source/python/{get_name(module_name)}/{get_filename(module)}.rst')
            with open(f'/tmp/source/python/{get_name(module_name)}/{get_filename(module)}.rst', 'w') as f:
                f.write(result)

def generate_all_tf_docs(module_library):
    for module_name, module_list in module_library.items():
        ensure_directory(f'/tmp/source/tf/{get_name(module_name)}')
        latest_version_text = tf_template(module_list[-1], module_list, show_toc=True)
        with open(f'/tmp/source/tf/{get_name(module_name)}/index.rst', 'w') as f:
            f.write(latest_version_text)
        for module in module_list:
            ensure_directory(f'/tmp/source/tf/{get_name(module_name)}')
            module_json = module.tf_variables
            result = tf_template(module, module_list, show_toc=False)
            print(f'storing rst file: {result}')
            print(f'/tmp/source/tf/{get_name(module_name)}/{get_filename(module)}.rst')
            with open(f'/tmp/source/tf/{get_name(module_name)}/{get_filename(module)}.rst', 'w') as f:
                f.write(result)

def generate_all_kubernetes_docs(module_library):
    ensure_directory('/tmp/source/kubernetes')
    for module_name, module_list in module_library.items():
        ensure_directory(f'/tmp/source/kubernetes/{get_name(module_name)}')
        latest_version_text = kubernetes_template(module_list[-1], module_list, show_toc=True)
        with open(f'/tmp/source/kubernetes/{get_name(module_name)}/index.rst', 'w') as f:
            f.write(latest_version_text)
        for module in module_list:
            ensure_directory(f'/tmp/source/kubernetes/{get_name(module_name)}')
            result = kubernetes_template(module, module_list, show_toc=False)
            print(f'storing rst file: {result}')
            print(f'/tmp/source/kubernetes/{get_name(module_name)}/{get_filename(module)}.rst')
            with open(f'/tmp/source/kubernetes/{get_name(module_name)}/{get_filename(module)}.rst', 'w') as f:
                f.write(result)

def generate_all_cli_docs(module_library):
    ensure_directory('/tmp/source/cli')
    for module_name, module_list in module_library.items():
        ensure_directory(f'/tmp/source/cli/{get_name(module_name)}')
        latest_version_text = cli_template(module_list[-1], module_list, show_toc=True)
        with open(f'/tmp/source/cli/{get_name(module_name)}/index.rst', 'w') as f:
            f.write(latest_version_text)
        for module in module_list:
            result = cli_template(module, module_list, show_toc=False)
            print(f'storing rst file: {result}')
            print(f'/tmp/source/cli/{get_name(module_name)}/{get_filename(module)}.rst')
            with open(f'/tmp/source/cli/{get_name(module_name)}/{get_filename(module)}.rst', 'w') as f:
                f.write(result)

def generate_all_md_files(module_library):
    for module_name, module_list in module_library.items():
        # result = run_terraform_docs_from_string(module_name, module_json)
        for module in module_list:
            result = f'''
# {module_name}

Variable | Type | Required | Default | Description
---------|------|----------|---------|------------
Cluster Name | string | No | cluster-name-example | N/A
Environment | string | Yes | N/A | N/A
Deployment Id | string | Yes | N/A | N/A
'''
            with open(f'/tmp/source/original_{get_name(module)}.md', 'w') as f:
                f.write(result)
            print(result)

def store_index_rst(module_library):
    result = index_rst_template(module_library)
    with open(f'/tmp/source/index.rst', 'w') as f:
        f.write(result)
    print(result)

def generate_webpage():
    import logging
    logging.info("Starting the webpage generation.")
    os.environ['HOME'] = '/tmp'
    os.environ['XDG_CACHE_HOME'] = '/tmp/.cache'

    # Sphinx arguments
    sys.argv = ["sphinx-build", "-b", "html", "/tmp/source", "/tmp/build"]

    try:
        build_main(sys.argv[1:])
    except Exception as e:
        logging.error(f"An error occurred: {e}")

    logging.info("Webpage generation completed.")

def ensure_directory(path):
    os.makedirs(path, exist_ok=True)  # Creates the directory if it does not exist


def run(module_library):
    ensure_directory('/tmp/build')
    shutil.copytree('./source', '/tmp/source', dirs_exist_ok=True)
    generate_all_py_modules(module_library)
    generate_all_python_docs(module_library)
    generate_all_tf_docs(module_library)
    generate_all_kubernetes_docs(module_library)
    generate_all_cli_docs(module_library)
    # generate_all_md_files(module_library)
    store_index_rst(module_library)
    os.environ['HOME'] = '/tmp'
    generate_webpage()
    # run_terraform_docs_from_string(module_library['S3Bucket'])

def zip_directory(folder_path, output_filename):
    shutil.make_archive(output_filename, 'zip', folder_path)

def run_and_zip(zip_path, modules):
    run(modules)
    zip_directory('/tmp/build', zip_path)
