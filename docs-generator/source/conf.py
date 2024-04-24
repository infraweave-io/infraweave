# Configuration file for the Sphinx documentation builder.
#
# For the full list of built-in configuration values, see the documentation:
# https://www.sphinx-doc.org/en/master/usage/configuration.html

# -- Project information -----------------------------------------------------
# https://www.sphinx-doc.org/en/master/usage/configuration.html#project-information

project = 'InfraBridge'
copyright = '2024, InfraBridge'
author = 'InfraBridge'

# -- General configuration ---------------------------------------------------
# https://www.sphinx-doc.org/en/master/usage/configuration.html#general-configuration

# Add 'autodoc_pydantic' to the list of extensions
extensions = [
    "sphinx.ext.autodoc",
    "sphinxcontrib.autodoc_pydantic",
    "sphinx.ext.doctest",
    "sphinx.ext.extlinks",
    "sphinx.ext.intersphinx",
    "sphinx.ext.todo",
    "sphinx.ext.mathjax",
    "sphinx.ext.viewcode",
    "sphinx_copybutton",
    "sphinx_immaterial",
    "sphinx_markdown_tables",
    "sphinx_search.extension",
    "myst_parser",
]

# autodoc-pydantic settings
autodoc_pydantic_model_show_validator_members = True
autodoc_pydantic_model_summary = True
autodoc_pydantic_settings_show_config_summary = True
autodoc_pydantic_field_list_validators = True

# Optional: if you want to customize even further
autodoc_pydantic_model_show_json = True
autodoc_pydantic_model_show_config = True
autodoc_pydantic_model_signature_prefix = "model "


templates_path = ['_templates']
exclude_patterns = ['_build', 'Thumbs.db', '.DS_Store']

# Automatically extract typehints when specified and place them in
# descriptions of the relevant function/method.
autodoc_typehints = "description"

# Don't show class signature with the class' name.
autodoc_class_signature = "separated"

# -- Options for HTML output -------------------------------------------------
# https://www.sphinx-doc.org/en/master/usage/configuration.html#options-for-html-output

html_theme = 'sphinx_immaterial'
extension = ["sphinx_immaterial"]
html_static_path = ['/tmp/_static']

html_logo = "infrabridge-logo.png"

import os
import sys
sys.path.insert(0, os.path.abspath('.'))
sys.path.insert(0, os.path.abspath('./python/'))

html_sidebars = {
    "**": ["logo-text.html", "globaltoc.html", "localtoc.html", "searchbox.html"]
}

html_theme_options = {
    "color_primary": "light-blue",
    'globaltoc_depth': 2,
    'globaltoc_collapse': False,
    "font": False, # Don't download fonts from Google
    "features": [
        "toc.follow",
        "navigation.instant",
    ],
}
