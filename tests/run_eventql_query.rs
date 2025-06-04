use eventsourcingdb_client_rust::container::Container;
use futures::stream::TryStreamExt;

#[tokio::test]
async fn run_empty_query() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    let rows = client
        .run_eventql_query("FROM e IN events ORDER BY e.time DESC TOP 100 PROJECT INTO e")
        .await
        .expect("Unable to run query");
    let rows: Result<Vec<_>, _> = rows.try_collect().await;
    assert!(rows.is_ok(), "Failed to run query: {:?}", rows);
    let rows = rows.expect("Failed to read rows");
    assert_eq!(rows.len(), 0);
}
