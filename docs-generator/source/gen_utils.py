import hcl2
from pydantic import create_model, Field
from io import StringIO

get_name = lambda module_name: module_name.lower()

def convert_tf_to_json_schema(module_name, hcl_string):
    variables = read_tf_variables(hcl_string)
    InputsModel = generate_pydantic_class(variables, class_name=module_name)
    return InputsModel.schema_json(indent=2)


def generate_pydantic_class(variables, class_name="TerraformInputs"):
    """Generate a Pydantic model from Terraform variables, including default values and descriptions."""
    fields = {
        var_name: (type_info[0], Field(default=type_info[1], description=type_info[2]))
        for var_name, type_info in variables.items()
    }
    return create_model(class_name, **fields)


def read_tf_variables(hcl_string):
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
