provider "aws" {
  region = "eu-central-1"
}

terraform {
  backend "s3" { }
}