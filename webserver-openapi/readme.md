# Web Server

This package provides a read-only REST-API interface to interact with InfraWeave platform. It is mainly built to be used with the Backstage plugin, however is meant to be generic for any other integration.

It is built for being run with access to the central account, and should not be exposed directly to the internet as authentication is not enabled by default (there is a commented section which can be used as an example).
