package marius.terraform_plan

import input as tfplan

# --- Validate region ---

get_region(provider_name) = region{
    region_var_name:= trim_prefix(input.configuration.provider_config[provider_name].expressions.region.references[0], "var.")
    region:= tfplan.variables[region_var_name].value
}
get_region(provider_name) = region{
    region:= tfplan.configuration.provider_config[provider_name].expressions.region.constant_value
}

get_basename(path) = basename{
    arr:= split(path, "/")
    basename:= arr[count(arr)-1]
}

contains(arr, elem){
    arr[_] == elem
}

# This policy enforces a list of allowed regions on environments launched from it.
# It takes an array of allowed regions as an argument (in the data object):
#   allowed_regions: the list of regions names that are allowed for usage.
#
# An example of a data object for this policy looks like this:
# {
#   "allowed_regions": [
#       "us-east-1",
#       "eu-west-1" 
#    ]
# }
#
# This example allows the deployment only using regions "us-east-1" and "eu-west-1".
deny[reason] {
    not is_array(data.allowed_regions)
    reason:= "The data variable 'allowed_regions' has to be an array."
}

deny[reason] {
    is_array(data.allowed_regions)
    provider_name:= get_basename(tfplan.resource_changes[_].provider_name)
    region:= get_region(provider_name)
    not contains(data.allowed_regions, region)
    reason:= concat("",["Invalid region: '", region, "'. The allowed AWS regions are: ", sprintf("%s", [data.allowed_regions])])
}