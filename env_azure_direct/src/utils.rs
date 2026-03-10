pub async fn get_region() -> String {
    std::env::var("REGION").expect("REGION not set")
}
