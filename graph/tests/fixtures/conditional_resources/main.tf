resource "null_resource" "enabled_example" {
  count = 1
}

resource "null_resource" "disabled_example" {
  count = 0
}

data "null_data_source" "enabled_data" {
}
