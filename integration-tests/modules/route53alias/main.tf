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
  alias {
    name = var.alias_name
    zone_id = var.alias_zone_id
    evaluate_target_health = var.alias_evalute_target_health
  }
  provider = aws.us-east-1
}
