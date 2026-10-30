mod utils;
use futures::stream::TryStreamExt;
use utils::create_test_container;

#[tokio::test]
async fn run_empty_query() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let rows = client
        .run_eventql_query("FROM e IN events ORDER BY e.time DESC TOP 100 PROJECT INTO e")
        .await
        .expect("Unable to run query");
    let rows: Result<Vec<_>, _> = rows.try_collect().await;
    assert!(rows.is_ok(), "Failed to run query: {rows:?}");
    let rows = rows.expect("Failed to read rows");
    assert_eq!(rows.len(), 0);
}

#[tokio::test]
async fn run_query_whose_stream_outlives_its_text() {
    // The stream does not borrow the text of the query, so a function can
    // make the query itself and return the stream.
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let rows = {
        let query = format!("FROM e IN events TOP {} PROJECT INTO e", 100);
        client
            .run_eventql_query(&query)
            .await
            .expect("Unable to run query")
    };
    let rows: Vec<_> = rows.try_collect().await.expect("Failed to read rows");

    assert_eq!(rows.len(), 0);
}
