import re
import json
from .gen_utils import get_module_name, convert_tf_module_to_json_schema

def camel_to_kebab(s):
  # E.g. MyModule -> my-module and EKSCluster -> eks-cluster
  s = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1-\2', s)
  s = re.sub(r'(?<=[a-z])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])', '-', s)
  return s.lower()

def module_json_to_rst_table(module_name, hcl_string):
  json_data = convert_tf_module_to_json_schema(module_name, hcl_string)
  data = json.loads(json_data)
  properties = data.get("properties", {})
  
  # Start building the reST table
  table = ".. list-table:: Input Parameters\n"
  table += "   :widths: auto\n"
  table += "   :header-rows: 1\n\n"
  table += "   * - Input Name\n"
  table += "     - Default Value\n"
  table += "     - Description\n"
  
  # Iterate over properties and add each to the table
  for key, value in properties.items():
      default = value.get("default", "")
      description = value.get("description", "").strip() or "No description provided dja dkasn dkja sjdk askd jkasdjksa dkj askdj asjkd askjdakjsd."
      title = value.get("title", key)  # Use title if available, otherwise use the key
      
      # Format the default value for display
      default_value = f"``{default}``" if default else ""
      
      # Add the property row to the table
      table += f"   * - {get_module_name(title)}\n"
      table += f"     - {default_value}\n"
      table += f"     - {description}\n"
  return table

kubernetes_template = lambda module_name, hcl_string: f'''
{module_name}
=======

Example
-------

   .. code-block:: yaml
      :linenos:

      apiVersion: infrabridge.io/v1
      kind: {module_name}
      metadata:
        name: my-{camel_to_kebab(module_name)}
        namespace: default
      spec:
        bucketName: my-unique-bucket-name-3543tea
        region: eu-central-1

Input Parameters
----------------        

{module_json_to_rst_table(module_name, hcl_string)}

Hint
----
.. tip:: This is a **hint**.

Changelog
-------

0.0.2
^^^^^
..  code-block:: diff
    :caption: inputs

    -bucketName | string | Yes | N/A | The name of the bucket to create.
    +bucketName | string | Yes | some-default-name | The name of the bucket to create.

    -defined('TYPO3_MODE') or die();
    +defined('TYPO3') or die();

0.0.1
^^^^^

..  code-block:: diff
    :caption: ext_localconf.php.diff

     <?php

    -defined('TYPO3_MODE') or die();
    +defined('TYPO3') or die();


'''
