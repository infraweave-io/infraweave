from .gen_utils import get_name, get_filename

def get_submodule_toc(module_library, key=''):
    submodule_toc = ''
    for module_name, module_list in module_library.items():
        submodule_toc += f'\n   {key}{get_name(module_name)}/index.rst'
    return submodule_toc
            

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
   {get_submodule_toc(modules_dict, 'tf/')}

.. toctree::
   :hidden:
   :maxdepth: 2
   :caption: Python SDK
   {get_submodule_toc(modules_dict, 'python/')}

.. toctree::
   :hidden:
   :maxdepth: 1
   :caption: CLI
   {get_submodule_toc(modules_dict, 'cli/')}

.. toctree::
   :hidden:
   :maxdepth: 2
   :caption: Kubernetes
   {get_submodule_toc(modules_dict, 'kubernetes/')}
'''