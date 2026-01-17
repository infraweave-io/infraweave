resource "local_file" "database" {
    content = "data"
    filename = "${path.module}/db.txt"
}

output "db_endpoint" {
    value = local_file.database.filename
}