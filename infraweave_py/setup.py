from setuptools import setup, find_packages

setup(
    name="infraweave_py",
    version="0.0.1",
    description="Implement InfraWeave in Python",
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
