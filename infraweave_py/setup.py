from setuptools import setup, find_packages
import os

this_directory = os.path.abspath(os.path.dirname(__file__))

try:
    with open(os.path.join(this_directory, 'README.md'), encoding='utf-8') as f:
        long_description = f.read()
except Exception:
    long_description = ''

setup(
    name="infraweave_py",
    version="0.0.1",
    description="Implement InfraWeave in Python",
    long_description=long_description,
    long_description_content_type='text/markdown',
    author="InfraWeave Team",
    author_email="opensource@infraweave.com",
    packages=find_packages(),
    install_requires=[],
    classifiers=[
        "Programming Language :: Python :: 3",
        "Operating System :: OS Independent",
    ],
    python_requires='>=3.10',
)
