variable "region" {
  default = "us-east-1"
}

data "aws_availability_zones" "available" {
  state = "available"
}

resource "aws_route_table" "intra" {
  count  = length(data.aws_availability_zones.available.names)
  vpc_id = "vpc-123456"
}

# This mimics the user's scenario:
# 1. 'data.aws_availability_zones.available' is read.
# 2. 'aws_route_table.intra' uses it in 'count'.
# 3. Expected Graph Edge: aws_route_table.intra -> data.aws_availability_zones.available
# 4. Expected Attribute: "count" (instead of empty depends_on)
