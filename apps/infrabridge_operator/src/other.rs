use kube::{api::Api, Client as KubeClient};

use log::{error, info, warn};

use kube::api::ApiResource;
use kube::api::DynamicObject;
use kube::api::GroupVersionKind;

use tokio::time::{self, Duration};

use std::collections::BTreeMap;

use crate::defs::KUBERNETES_GROUP;
use crate::kind::apply_infra;
use env_aws::read_status;

async fn periodic_status_check(delay_seconds: u64, deployment_id: String) {
    let mut interval = time::interval(Duration::from_secs(delay_seconds));

    loop {
        interval.tick().await;
        // Execute the task
        match read_status(deployment_id.clone()).await {
            Ok(status) => info!("Status: {:?}", status),
            Err(e) => error!("Failed to read status: {:?}", e),
        }
    }
}

fn schedule_status_check(delay_seconds: u64, deployment_id: String) {
    // Schedule a status check
    info!("Scheduling future job...");

    // Spawn a new asynchronous task for the delayed job
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;

        // After the delay, run the future job
        let status = match read_status(deployment_id).await {
            Ok(status) => status,
            Err(e) => {
                error!("Failed to read status: {:?}", e);
                return;
            }
        };
        info!("Status: {:?}", status);
    });
}

pub async fn resume_dependants_apply(
    kube_client: KubeClient,
    kind: String,
    name: String,
    namespace: String,
) {
    let resource_dependants =
        match find_resource_dependants(kube_client.clone(), &kind, &name, &namespace).await {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to find dependants for: {}: {}", &name, e);
                vec![]
            }
        };

    if resource_dependants.len() == 0 {
        warn!("No dependants for: {}", &name);
        // Early exit if no dependants
        return;
    }

    warn!("Resource dependants: {:?}", resource_dependants);
    warn!("Starting apply for dependants...");

    let client = KubeClient::try_default().await.unwrap();

    for dep in resource_dependants {
        let kind = dep.0;
        let name = dep.1;

        let api_resource = ApiResource::from_gvk_with_plural(
            &GroupVersionKind {
                group: KUBERNETES_GROUP.into(),
                version: "v1".into(),
                kind: kind.clone(),
            },
            &(kind.to_lowercase() + "s"),
        );

        let api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), &namespace, &api_resource);
        let resource = api.get(&name).await;

        match resource {
            Ok(res) => {
                apply_infra(&client, res, &kind).await;
            }
            Err(e) => {
                error!("Failed to find dependant: {}", e);
            }
        }
    }
}

async fn find_resource_dependants(
    client: KubeClient,
    kind: &str,
    name: &str,
    namespace: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let all_kinds = vec!["s3bucket", "iamrole", "lambda", "dynamodb"]; // TODO: Get all kinds using internal state which is set on restart

    let mut dependants: Vec<(String, String)> = Vec::new();

    for check_kind in all_kinds {
        let dependants_for_kind =
            match find_dependants_for_kind(client.clone(), check_kind, kind, name, namespace).await
            {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to find dependants for kind: {}: {}", check_kind, e);
                    vec![]
                }
            };
        dependants.extend(dependants_for_kind);
    }

    Ok(dependants)
}

async fn find_dependants_for_kind(
    client: KubeClient,
    dependent_kind: &str,
    kind: &str,
    name: &str,
    namespace: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut dependants: Vec<(String, String)> = Vec::new();

    let plural = dependent_kind.to_lowercase() + "s";
    let api_resource = ApiResource::from_gvk_with_plural(
        &GroupVersionKind {
            group: KUBERNETES_GROUP.into(),
            version: "v1".into(),
            kind: dependent_kind.to_string(),
        },
        &plural,
    );
    let api: Api<DynamicObject> = Api::namespaced_with(client, &namespace, &api_resource);

    let res_list = api.list(&Default::default()).await?;
    let depends_on_key = &format!("{}/{}", KUBERNETES_GROUP, "dependsOn");
    for res in res_list.items {
        let annotations = res
            .metadata
            .annotations
            .clone()
            .unwrap_or_else(|| BTreeMap::new());
        let depends_on_str = annotations
            .get(depends_on_key)
            .map(|s| s.clone())
            .unwrap_or("".to_string());

        let depends_on: Vec<String> = depends_on_str.split(",").map(|s| s.to_string()).collect();
        for dep in depends_on.iter() {
            let parts: Vec<&str> = dep.split("::").collect();
            if parts.len() == 2
                && parts[0].to_lowercase() == kind.to_lowercase()
                && parts[1].to_lowercase() == name.to_lowercase()
            {
                let res_copy = res.clone();
                let current_kind = res_copy.types.unwrap().kind;
                let current_name = res_copy
                    .metadata
                    .name
                    .unwrap_or_else(|| "noname".to_string());
                dependants.push((current_kind, current_name));
            }
        }
    }
    Ok(dependants)
}
