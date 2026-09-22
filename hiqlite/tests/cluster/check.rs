use crate::execute_query::TestData;
use crate::{debug, log};
use hiqlite::macros::params;
use hiqlite::{Client, Error};

use crate::Cache;
use crate::cache::{KEY, KEY_2, VALUE, VALUE_2};

pub async fn is_client_db_healthy(client: &Client, id: Option<u64>) -> Result<(), Error> {
    client.wait_until_healthy_db().await;
    client.wait_until_healthy_cache().await;

    // F-116: "healthy" means a leader is known, not that this node has caught up. A node that
    // rejoins from an empty volume replays the leader's log from the start, and the first
    // membership entry in it is the original single-node bootstrap, so a check taken during the
    // replay saw one member and failed. The membership is waited for, bounded, not sampled.
    log(format!("Checking DB health Node {:?}", id));
    wait_for_members(
        || async { Ok(client.metrics_db().await?.membership_config.nodes().count()) },
        "db",
        id,
    )
    .await?;

    log(format!("Checking Cache health {:?}", id));
    client.wait_until_healthy_cache().await;
    log(format!("Cache {:?} is healthy", id));
    wait_for_members(
        || async { Ok(client.metrics_cache().await?.membership_config.nodes().count()) },
        "cache",
        id,
    )
    .await?;

    // we will do the select 1 to catch leader switches that may have
    // happened in between and trigger a client stream switch that way
    log("client batch");
    client.batch("SELECT 1;").await?;
    log("client batch returned");

    // make sure our before inserted data exists
    let data: Result<Vec<TestData>, Error> = client
        .query_map("SELECT * FROM test WHERE id >= $1", params!(11))
        .await;
    debug(&data);
    let data = data?;

    assert_eq!(data.len(), 6);
    assert_eq!(data[0].id, 11);
    assert_eq!(data[1].id, 12);
    assert_eq!(data[2].id, 13);
    assert_eq!(data[3].id, 21);
    assert_eq!(data[4].id, 22);
    assert_eq!(data[5].id, 23);

    log(format!("Database healthy {:?}", id));

    let v: String = client.get(Cache::One, KEY).await?.unwrap();
    assert_eq!(&v, VALUE);
    let v: String = client.get(Cache::Two, KEY_2).await?.unwrap();
    assert_eq!(&v, VALUE_2);

    let v: Option<String> = client.get(Cache::One, KEY_2).await?;
    assert!(v.is_none());
    let v: Option<String> = client.get(Cache::Two, KEY).await?;
    assert!(v.is_none());

    log(format!("Cache healthy {:?}", id));

    // everything should still be healthy
    client
        .is_healthy_db()
        .await
        .expect("db should still be healthy");
    client
        .is_healthy_cache()
        .await
        .expect("cache should still be healthy");

    Ok(())
}

/// Wait, at most thirty seconds, until this node's view of the membership has all three nodes.
async fn wait_for_members<F, Fut>(members: F, what: &str, id: Option<u64>) -> Result<(), Error>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<usize, Error>>,
{
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let members = members().await?;
        if members == 3 {
            return Ok(());
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} membership on node {id:?} still has {members} member(s) after 30 seconds"
        );
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}
