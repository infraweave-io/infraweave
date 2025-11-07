terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
      configuration_aliases = [aws.us-east-1]
    }
  }
}

resource "aws_route53_zone" "this" {
  name = var.domain_name
  provider = aws.us-east-1
}

# Hardcoded some values just for this test
resource "aws_route53_record" "this" {
  zone_id = aws_route53_zone.this.zone_id
  name    = var.domain_name
  type    = "A"
  ttl     = var.ttl
  records = var.records
  provider = aws.us-east-1
}
