from .gen_utils import get_name

tf_template = lambda module_name, hcl_string: f'''
{module_name}
=======

Example
-------

   .. code-block:: python
      :linenos:

      s3 = S3Bucket(
        name="mybucket"
      )


.. list-table:: Input Parameters
   :widths: 25 35 50
   :header-rows: 1

   * - Input Name
     - Default Value
     - Description
   * - bucket_name
     - 
     - The name of the bucket to create.
   * - region
     - ``us-west-2``
     - The region in which the bucket will be created.


Note
----
.. tip:: This is a **note**.


API Documentation
-----------------

.. autopydantic_model:: {get_name(module_name)}.{module_name}

'''
