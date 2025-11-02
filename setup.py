"""
Setup configuration for custom MkDocs plugins.
"""

from setuptools import setup, find_packages

setup(
    name='mkdocs-infraweave-plugins',
    version='0.1.0',
    description='Custom MkDocs plugins for InfraWeave documentation',
    packages=find_packages(where='docs'),
    package_dir={'': 'docs'},
    install_requires=[
        'mkdocs>=1.0',
        'mkdocstrings[python]>=0.24.0',
    ],
    entry_points={
        'mkdocs.plugins': [
            'changelog = plugins.changelog_plugin:ChangelogPlugin',
            'python-docs = plugins.python_docs_plugin:PythonDocsPlugin',
        ]
    },
    python_requires='>=3.8',
)
