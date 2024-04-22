from .gen_utils import get_name

python_template = lambda module_name, module_json: f'''
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
