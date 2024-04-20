from .gen_toc import get_toc

index_rst_template = lambda modules_dict: f'''
Startpage - InfraBridge Documentation
========================

Welcome to the InfraBridge documentation. Here you can find information on how to get started, as well as detailed API documentation.

Getting Started
---------------

There are several ways to interact with InfraBridge. You can use the Python SDK, the CLI, or the Kubernetes API. Below you will find links to the documentation for each of these methods.

See the `here <kubernetes.html>`_ for more details.

{get_toc(modules_dict)}
'''