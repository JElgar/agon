//! Shared harness for the per-sport DynamoDB-Local round-trip tests (see
//! `netball_score_record_roundtrip.rs`'s doc comment for what these tests
//! prove and why). Not a test binary itself — `mod common;` from each sport's
//! test file.

use agon_services_core::dao::Dao;

/// A `Dao` plus the raw `Client`/table name it was built from — the raw
/// client is only for a test file's own low-level `PutItem` calls (e.g.
/// writing a hand-built legacy JSON shape); `Dao` itself deliberately doesn't
/// expose its internal client.
pub struct TestEnv {
    pub dao: Dao,
    pub client: aws_sdk_dynamodb::Client,
    pub table: String,
}

/// Connects to `AWS_ENDPOINT_URL` and makes sure the `agon` table exists
/// (mirrors `local/dynamodb/create-table.sh`'s schema so this needs no
/// separate script or `aws` CLI — just a reachable DynamoDB endpoint).
/// Returns `None` (rather than panicking) when `AWS_ENDPOINT_URL` isn't set,
/// so a test using this is a no-op skip — never an accidental hit on a real
/// table — outside an explicit local run.
pub async fn local_env() -> Option<TestEnv> {
    if std::env::var("AWS_ENDPOINT_URL").is_err() {
        return None;
    }
    let table = std::env::var("AGON_TABLE_NAME").unwrap_or_else(|_| "agon".to_string());
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_dynamodb::Client::new(&config);
    ensure_table_exists(&client, &table).await;
    Some(TestEnv {
        dao: Dao::new(client.clone(), table.clone()),
        client,
        table,
    })
}

async fn ensure_table_exists(client: &aws_sdk_dynamodb::Client, table: &str) {
    use aws_sdk_dynamodb::types::{
        AttributeDefinition, BillingMode, GlobalSecondaryIndex, KeySchemaElement, KeyType,
        Projection, ProjectionType, ScalarAttributeType,
    };

    if let Ok(existing) = client.describe_table().table_name(table).send().await {
        // A table created before the entity index (GSI4) existed: add it.
        let has_gsi4 = existing
            .table()
            .map(|t| {
                t.global_secondary_indexes()
                    .iter()
                    .any(|g| g.index_name() == Some("GSI4"))
            })
            .unwrap_or(false);
        if !has_gsi4 {
            add_entity_index(client, table).await;
        }
        return;
    }

    let attr = |name: &str| {
        AttributeDefinition::builder()
            .attribute_name(name)
            .attribute_type(ScalarAttributeType::S)
            .build()
            .unwrap()
    };
    let key = |name: &str, kind: KeyType| {
        KeySchemaElement::builder()
            .attribute_name(name)
            .key_type(kind)
            .build()
            .unwrap()
    };
    let gsi = |index: &str, pk: &str, sk: &str| {
        GlobalSecondaryIndex::builder()
            .index_name(index)
            .key_schema(key(pk, KeyType::Hash))
            .key_schema(key(sk, KeyType::Range))
            .projection(
                Projection::builder()
                    .projection_type(ProjectionType::All)
                    .build(),
            )
            .build()
            .unwrap()
    };

    let result = client
        .create_table()
        .table_name(table)
        .attribute_definitions(attr("PK"))
        .attribute_definitions(attr("SK"))
        .attribute_definitions(attr("GSI1PK"))
        .attribute_definitions(attr("GSI1SK"))
        .attribute_definitions(attr("GSI2PK"))
        .attribute_definitions(attr("GSI2SK"))
        .attribute_definitions(attr("GSI3PK"))
        .attribute_definitions(attr("GSI3SK"))
        .attribute_definitions(attr("GSI4PK"))
        .attribute_definitions(attr("GSI4SK"))
        .key_schema(key("PK", KeyType::Hash))
        .key_schema(key("SK", KeyType::Range))
        .global_secondary_indexes(gsi("GSI1", "GSI1PK", "GSI1SK"))
        .global_secondary_indexes(gsi("GSI2", "GSI2PK", "GSI2SK"))
        .global_secondary_indexes(gsi("GSI3", "GSI3PK", "GSI3SK"))
        .global_secondary_indexes(entity_index())
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await;

    // Tolerate a concurrent create from another sport's test binary racing
    // this same first-run table creation (both see "doesn't exist" via
    // `describe_table` above, then both call `create_table`) — anything but
    // "the table already exists now" is a real failure. Checked via the
    // typed service error, not `Display`/`to_string()` (which renders as the
    // uninformative "service error" — the useful detail is only in `Debug`).
    if let Err(err) = result {
        let is_already_exists = err
            .as_service_error()
            .is_some_and(|e| e.is_resource_in_use_exception());
        if !is_already_exists {
            panic!("create table for local test run: {err:?}");
        }
    }
}

/// GSI4, the keys-only entity index (see `dao::entity_index`).
fn entity_index() -> aws_sdk_dynamodb::types::GlobalSecondaryIndex {
    use aws_sdk_dynamodb::types::{
        GlobalSecondaryIndex, KeySchemaElement, KeyType, Projection, ProjectionType,
    };
    let key = |name: &str, kind: KeyType| {
        KeySchemaElement::builder()
            .attribute_name(name)
            .key_type(kind)
            .build()
            .unwrap()
    };
    GlobalSecondaryIndex::builder()
        .index_name("GSI4")
        .key_schema(key("GSI4PK", KeyType::Hash))
        .key_schema(key("GSI4SK", KeyType::Range))
        .projection(
            Projection::builder()
                .projection_type(ProjectionType::KeysOnly)
                .build(),
        )
        .build()
        .unwrap()
}

async fn add_entity_index(client: &aws_sdk_dynamodb::Client, table: &str) {
    use aws_sdk_dynamodb::types::{
        AttributeDefinition, CreateGlobalSecondaryIndexAction, GlobalSecondaryIndexUpdate,
        ScalarAttributeType,
    };
    let attr = |name: &str| {
        AttributeDefinition::builder()
            .attribute_name(name)
            .attribute_type(ScalarAttributeType::S)
            .build()
            .unwrap()
    };
    let gsi = entity_index();
    let create = CreateGlobalSecondaryIndexAction::builder()
        .index_name("GSI4")
        .set_key_schema(Some(gsi.key_schema().to_vec()))
        .set_projection(gsi.projection().cloned())
        .build()
        .unwrap();
    let result = client
        .update_table()
        .table_name(table)
        .attribute_definitions(attr("GSI4PK"))
        .attribute_definitions(attr("GSI4SK"))
        .global_secondary_index_updates(
            GlobalSecondaryIndexUpdate::builder().create(create).build(),
        )
        .send()
        .await;
    // Another test binary may have added it first.
    if let Err(err) = result
        && !format!("{err:?}").contains("already exists")
    {
        panic!("add GSI4 for local test run: {err:?}");
    }
}
