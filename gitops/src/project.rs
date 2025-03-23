use env_common::interface::GenericCloudHandler;
use env_defs::CloudProvider;
use serde_json::Map;
use std::env;

async fn get_project_map() -> Map<String, serde_json::Value> {
    let handler = GenericCloudHandler::default().await;
    let project_map_item = handler.get_project_map().await.unwrap();
    project_map_item
        .get("data")
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

pub async fn get_project_id_for_repository_path(
    full_repository_path: &str,
) -> Result<String, anyhow::Error> {
    let project_map: Map<String, serde_json::Value> = match env::var("PROJECT_MAP") {
        Ok(project_map_str) => serde_json::from_str(&project_map_str).unwrap(),
        Err(_) => get_project_map().await,
    };

    for (key, value) in project_map.iter() {
        let key = key.replace("*", ".*");
        let key = key.replace("/", "\\/");
        let re = regex::Regex::new(&key).unwrap();
        if re.is_match(full_repository_path) {
            println!(
                "Found project ID for repository path: {}, result: {}",
                full_repository_path, value
            );
            let project_id = value.get("project_id").unwrap();
            return Ok(project_id.as_str().unwrap().to_string());
        }
    }

    Err(anyhow::anyhow!(
        "No project ID has been configured for repository path: {}",
        full_repository_path
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_project_id_for_repository_path() {
        env::set_var(
            "PROJECT_MAP",
            r#"{
            "SomeGroup/path123/*": {
                "project_id": "111111111"
            },
            "SomeGroup/path987/*": {
                "project_id": "222222222"
            },
            "SomeGroup/strictpath987/project987": {
                "project_id": "333333333"
            },
            "SomeGroup/path567/proj*": {
                "project_id": "444444444"
            }
        }"#,
        );

        assert_eq!(
            get_project_id_for_repository_path("SomeGroup/path123/project123")
                .await
                .unwrap(),
            "111111111".to_string()
        );

        assert_eq!(
            get_project_id_for_repository_path("SomeGroup/path12345")
                .await
                .unwrap_err()
                .to_string(),
            "No project ID has been configured for repository path: SomeGroup/path12345"
        );

        assert_eq!(
            get_project_id_for_repository_path("SomeGroup/path987/project987")
                .await
                .unwrap(),
            "222222222".to_string()
        );
        assert_eq!(
            get_project_id_for_repository_path("SomeGroup/strictpath987")
                .await
                .unwrap_err()
                .to_string(),
            "No project ID has been configured for repository path: SomeGroup/strictpath987"
        );

        assert_eq!(
            get_project_id_for_repository_path("SomeGroup/strictpath987/project987")
                .await
                .unwrap(),
            "333333333".to_string()
        );

        assert_eq!(
            get_project_id_for_repository_path("SomeGroup/path567/proj")
                .await
                .unwrap(),
            "444444444".to_string()
        );

        assert_eq!(
            get_project_id_for_repository_path("SomeGroup/path567/project123")
                .await
                .unwrap(),
            "444444444".to_string()
        );
    }
}
