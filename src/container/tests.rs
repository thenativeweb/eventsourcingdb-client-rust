use std::{collections::HashMap, time::Duration};

use testcontainers::{
    bollard::query_parameters::{ListContainersOptions, RemoveContainerOptions},
    core::client::docker_client_instance,
};

use super::{ContainerBuilder, START_LABEL};

/// Removes the containers of the given start, and returns how many there were.
async fn remove_containers_of(start_id: &str) -> usize {
    let docker = docker_client_instance()
        .await
        .expect("Failed to connect to Docker");
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters: Some(HashMap::from([(
                "label".to_string(),
                vec![format!("{START_LABEL}={start_id}")],
            )])),
            ..Default::default()
        }))
        .await
        .expect("Failed to list the containers");
    for container in &containers {
        let id = container.id.as_deref().expect("A container has an ID");
        docker
            .remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await
            .expect("Failed to remove a container");
    }
    containers.len()
}

#[tokio::test]
async fn a_start_that_runs_into_the_timeout_leaves_no_container_behind() {
    let start_id = format!("test-{:032x}", rand::random::<u128>());
    let builder = ContainerBuilder {
        image_tag: std::env::var("EVENTSOURCINGDB_IMAGE_TAG")
            .unwrap_or_else(|_| "preview".to_string()),
        // Docker can not start a container this fast, so the start runs into the timeout.
        startup_timeout: Duration::from_millis(1),
        ..ContainerBuilder::default()
    };

    let result = builder.start_with_id(&start_id).await;
    // Whatever the start left behind is removed before the assertions, so that the test leaves nothing behind.
    let left_behind = remove_containers_of(&start_id).await;

    let error = result.expect_err("Expected the start to run into the timeout");
    assert!(
        error.to_string().contains("timeout"),
        "Expected a startup timeout, but got: {error}"
    );
    assert_eq!(
        left_behind, 0,
        "Expected the failed start to remove its container"
    );
}

#[test]
fn containers_are_kept_only_if_testcontainers_is_told_to_keep_them() {
    use super::keeps_containers;

    assert!(keeps_containers(Some("keep")));
    assert!(!keeps_containers(Some("remove")));
    assert!(!keeps_containers(Some("")));
    assert!(!keeps_containers(None));
}
