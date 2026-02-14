mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod operator_tests {
    use super::*;
    use env_common::interface::GenericCloudHandler;
    use env_defs::{CloudProvider, CloudProviderCommon};
    use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
    use kube::{
        api::{Api, ApiResource, DynamicObject, GroupVersionKind, PostParams},
        config::{KubeConfigOptions, Kubeconfig},
        Client, Config,
    };
    use operator::operator::{list_and_apply_modules, start_infraweave_controllers};
    use pretty_assertions::assert_eq;
    use rustls::crypto::CryptoProvider;
    use std::{env, fs, time::Duration};
    use testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt};
    use testcontainers_modules::k3s::{K3s, KUBE_SECURE_PORT};
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_operator() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let home_dir = dirs::home_dir().expect("Failed to get home directory");
            let conf_dir = home_dir.join("k3s_conf_test");
            fs::create_dir_all(&conf_dir).expect("Failed to create config directory");

            let k3s = K3s::default()
                .with_conf_mount(&conf_dir)
                .with_tag("v1.31.5-k3s1")
                .with_privileged(true)
                .with_userns_mode("host");

            let k3s_container = k3s.start().await.unwrap();
            let client = get_kube_client(&k3s_container).await.unwrap();

            let current_dir = env::current_dir().expect("Failed to get current directory");

            env_common::publish_provider(
                &handler,
                current_dir
                    .join("providers/aws-5/")
                    .to_str()
                    .unwrap(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            env_common::publish_module(
                &handler,
                current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap(),
                "dev",
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            let exists = crd_exists(&client, "s3buckets.infraweave.io").await;
            assert_eq!(exists, false); // Operator has not started yet, no CRD should exist

            // Try operator function to list and apply all existing modules as CRDs
            match list_and_apply_modules(&handler, client.clone()).await {
                Ok(_) => println!("Successfully listed and applied modules"),
                Err(e) => eprintln!("Failed to list and apply modules: {:?}", e),
            }

            // Start the controllers
            start_infraweave_controllers(&handler, client.clone());

            sleep(Duration::from_secs(3)).await;

            let exists = crd_exists(&client, "s3buckets.infraweave.io").await;
            assert_eq!(exists, true); // Operator has started, CRD should exist

            let deployment_id = "test-bucket123";
            let namespace = "default";
            let environment = format!("k8s-cluster-id/{}", namespace);

            let yaml_claim = format!(
                r#"
apiVersion: infraweave.io/v1
kind: S3Bucket
metadata:
  name: {}
  namespace: {}
spec:
  moduleVersion: 0.1.2-dev+test.10
  region: us-west-2
  variables:
    bucketName: "test-bucket"
    tags:
      region: "us-west-1"
      environment: "dev"
"#,
                deployment_id, namespace
            );

            let cr_claim: DynamicObject =
                serde_yaml::from_str(&yaml_claim).expect("Failed to parse YAML");
            let ar = ApiResource::from_gvk(&GroupVersionKind {
                group: "infraweave.io".to_string(),
                version: "v1".to_string(),
                kind: "S3Bucket".to_string(),
            });
            let crd_api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &ar);
            let post_params = PostParams::default();
            match crd_api.create(&post_params, &cr_claim).await {
                Ok(response) => println!("Custom resource created: {:?}", response.metadata.name),
                Err(err) => eprintln!("Failed to create custom resource: {}", err),
            }

            // Give the watcher extra time to start successfully with retries
            sleep(Duration::from_secs(20)).await;

            let claim_res = crd_api.get(deployment_id).await;
            assert_eq!(claim_res.is_ok(), true);
            let claim = claim_res.unwrap();

            // Check if status exists before unwrapping
            let status = claim.data.get("status").unwrap_or_else(|| {
                panic!(
                    "Status field not found on resource. This might indicate the watcher failed to start. Resource data: {:?}",
                    claim.data
                )
            });

            let resource_status = status.get("resourceStatus").unwrap_or_else(|| {
                panic!(
                    "resourceStatus field not found in status. Status data: {:?}",
                    status
                )
            });

            assert_eq!(
                resource_status,
                "Apply - initiated"
            );

            // Set deployment status to successful in database to simulate successful deployment (since start_runner is mocked)
            let lambda_endpoint_url = "http://127.0.0.1:8081";
            let handler2 = GenericCloudHandler::custom(lambda_endpoint_url).await;

            let all_deployments = handler2.get_all_deployments(&environment, false).await.unwrap();
            println!("All deployments: {:?}", all_deployments);
            let deployment = all_deployments.first();
            assert_eq!(deployment.is_some(), true);
            let mut deployment = deployment.unwrap().clone();
            deployment.status = "successful".to_string();
            handler2.set_deployment(&deployment, false).await.unwrap();

            sleep(Duration::from_secs(11)).await; // Refreshes every 10 seconds, hence guaranteeing a refresh

            let claim_res = crd_api.get(deployment_id).await;
            assert_eq!(claim_res.is_ok(), true);
            let claim = claim_res.unwrap();

            let status = claim.data.get("status").unwrap_or_else(|| {
                panic!(
                    "Status field not found on resource after refresh. Resource data: {:?}",
                    claim.data
                )
            });

            let resource_status = status.get("resourceStatus").unwrap_or_else(|| {
                panic!(
                    "resourceStatus field not found in status after refresh. Status data: {:?}",
                    status
                )
            });

            assert_eq!(
                resource_status,
                "Apply - successful"
            );

            // TODO: Use real runner in test_api.py so change records can be fetched

            // let job_id = claim.data.get("status").unwrap().get("jobId").unwrap().as_str().unwrap();
            // assert_eq!(job_id, "123");

            // let change_type = "apply";
            // let change_record = handler2.get_change_record(&environment, &deployment_id, job_id, &change_type).await.unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn test_admission_webhook() {
        test_scaffold(|| async move {
            use axum::body::Body;
            use axum::http::Request;
            use operator::webhook::create_webhook_router;
            use tower::ServiceExt;

            // Create a handler for webhook validation
            let lambda_endpoint_url = "http://127.0.0.1:8081";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;

            let app = create_webhook_router(handler);

            // Test that the webhook validates and accepts a valid claim
            let admission_review = serde_json::json!({
                "apiVersion": "admission.k8s.io/v1",
                "kind": "AdmissionReview",
                "request": {
                    "uid": "integration-test-uid",
                    "kind": {
                        "group": "infraweave.io",
                        "version": "v1",
                        "kind": "S3Bucket"
                    },
                    "resource": {
                        "group": "infraweave.io",
                        "version": "v1",
                        "resource": "s3buckets"
                    },
                    "operation": "CREATE",
                    "object": {
                        "apiVersion": "infraweave.io/v1",
                        "kind": "S3Bucket",
                        "metadata": {
                            "name": "test-bucket-webhook",
                            "namespace": "default"
                        },
                        "spec": {
                            "moduleVersion": "0.1.2-dev+test.10",
                            "region": "us-west-2",
                            "variables": {
                                "bucketName": "my-webhook-test-bucket"
                            }
                        }
                    }
                }
            });

            let response = app
                .oneshot(
                    Request::builder()
                        .uri("/validate")
                        .method("POST")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&admission_review).unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), 200, "Webhook should return 200 OK");

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let review_response: serde_json::Value = serde_json::from_slice(&body).unwrap();

            assert_eq!(
                review_response["response"]["uid"].as_str().unwrap(),
                "integration-test-uid"
            );
            println!(
                "Validation response: {}",
                serde_json::to_string_pretty(&review_response).unwrap()
            );
        })
        .await;
    }

    async fn crd_exists(client: &Client, crd_name: &str) -> bool {
        let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
        match crds.get(crd_name).await {
            Ok(crd) => {
                println!("CRD '{}' exists!", crd.metadata.name.unwrap_or_default());
                true
            }
            Err(kube::Error::Api(err)) if err.code == 404 => {
                println!("CRD '{}' does not exist.", crd_name);
                false
            }
            Err(err) => {
                eprintln!("Error checking for CRD '{}': {}", crd_name, err);
                false
            }
        }
    }

    pub async fn get_kube_client(
        container: &ContainerAsync<K3s>,
    ) -> Result<kube::Client, Box<dyn std::error::Error + 'static>> {
        if CryptoProvider::get_default().is_none() {
            rustls::crypto::ring::default_provider()
                .install_default()
                .expect("Error initializing rustls provider");
        }

        let conf_yaml = container.image().read_kube_config()?;

        let mut config = Kubeconfig::from_yaml(&conf_yaml).expect("Error loading kube config");

        let port = container.get_host_port_ipv4(KUBE_SECURE_PORT).await?;
        config.clusters.iter_mut().for_each(|cluster| {
            if let Some(server) = cluster.cluster.as_mut().and_then(|c| c.server.as_mut()) {
                *server = format!("https://127.0.0.1:{}", port)
            }
        });

        let client_config =
            Config::from_custom_kubeconfig(config, &KubeConfigOptions::default()).await?;

        Ok(kube::Client::try_from(client_config)?)
    }
}
