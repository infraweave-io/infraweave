
output "repositories" {
  value = { for repo in aws_codecommit_repository.module : repo.repository_name => {
    name          = repo.repository_name
    clone_url_http = repo.clone_url_http
  }}
}