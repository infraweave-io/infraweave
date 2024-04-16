# Use an official Python runtime as a parent image
FROM python:3.12-slim

# Set the working directory in the container
WORKDIR /app

# Copy the requirements file into the container at /app
COPY requirements.txt /app/

# Install packages specified in requirements.txt
RUN pip install --no-cache-dir -r requirements.txt

# Install terraform-docs
RUN apt update; apt install -y curl
RUN cd /tmp/ && \
    curl -sSLo ./terraform-docs.tar.gz https://terraform-docs.io/dl/v0.17.0/terraform-docs-v0.17.0-$(uname)-amd64.tar.gz && \
    tar -xzf terraform-docs.tar.gz && \
    chmod +x terraform-docs && \
    mv terraform-docs /usr/local/bin/terraform-docs

# Copy the rest of your Sphinx project into the container at /app
RUN mkdir /app/source
RUN mkdir /app/source/_static
COPY *.py /app/
COPY conf.py /app/source
COPY *.rst /app/source
COPY infrabridge-logo.png /app/source

CMD ["python", "/app/generator.py"]
