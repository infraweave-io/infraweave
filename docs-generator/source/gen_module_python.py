from .gen_utils import get_name, get_filename

python_template = lambda module, module_list, show_toc: f'''
{module.module_name} ({module.version})
=======

{latest_badge(module, module_list)}

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

.. autopydantic_model:: {get_filename(module)}.{get_name(module.module_name)}.{module.module_name}


{toc_text(show_toc, module_list)}

{'\n\n'.join([f':doc:`Version ({module.version}) <{get_filename(module)}>`' for module in module_list])}

'''

def toc_text(show_toc, module_list):
  toc = f'''
Versions
--------

.. toctree::
   :hidden:
   :maxdepth: 1
   :caption: Versions

{'\n'.join([f'   {get_filename(module)}' for module in module_list])}
''' if show_toc else ''
  print(toc)
  return toc

def latest_badge(module, module_list):
  latest = module_list[-1]
  is_latest = module.version == latest.version
  return f'''
.. success::
   :no-title:

   This is the latest version''' if is_latest else f'''
.. warning::
   :no-title:
   
   This is not the latest version.'''

