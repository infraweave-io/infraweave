from .gen_utils import get_name

get_toc = lambda modules_dict: f'''

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
   :caption: Available Modules

   {'\n   '.join(['tf_' + get_name(module_name) for module_name in modules_dict.keys()])}

.. toctree::
   :hidden:
   :maxdepth: 2
   :caption: Python SDK

   {'\n   '.join([get_name(module_name) for module_name in modules_dict.keys()])}

.. toctree::
   :hidden:
   :maxdepth: 2
   :caption: CLI

   {'\n   '.join(['cli/' + get_name(module_name) for module_name in modules_dict.keys()])}

.. toctree::
   :hidden:
   :maxdepth: 2
   :caption: Kubernetes

   kubernetes
   {'\n   '.join(['kubernetes/' + get_name(module_name) for module_name in modules_dict.keys()])}
'''