//! Local stand-in for the EventBridge Pipe in `agon_infra/index.ts`: tails
//! DynamoDB Local's stream for the `agon` table and forwards each record to a
//! local SQS-compatible queue (ElasticMQ), in the exact envelope shape
//! `agon_worker/src/event.rs` expects — see that module's doc comment for the
//! shape. Dev tooling only; not part of the real async pipeline (that's
//! DynamoDB Streams -> EventBridge Pipe -> SQS, see agon_infra/index.ts).
//!
//! Env (reuses the same names the rest of the local stack already uses —
//! see the root .env.example):
//!   AGON_TABLE_NAME              - table to tail (default "agon")
//!   AGON_EVENTS_QUEUE_URL         - queue to forward records to (required)
//!   AWS_ENDPOINT_URL_DYNAMODB_STREAMS - DynamoDB Local's address (required —
//!     see "Why raw HTTP" below for why this one call needs its own client)
//!   AWS_REGION / AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY / AWS_ENDPOINT_URL_SQS
//!     - standard AWS SDK env vars for everything else (stream discovery,
//!     SQS), resolved independently per service from one shared config
//!     (verified this works before writing any of this — see
//!     local/README.md).
//!
//! ## Why raw HTTP for GetRecords
//!
//! DynamoDB Local has a real interop bug: it renders an empty Map attribute
//! in a stream record as bare `{}` instead of the tagged `{"M": {}}` real
//! DynamoDB (and the wire spec) uses. The generated `aws-sdk-dynamodbstreams`
//! client deserializes `AttributeValue` as a strict tagged union and rejects
//! the untagged `{}` outright — "Union did not contain a valid variant" —
//! which fails the *entire* GetRecords response, not just that one
//! attribute. Since `agon_core`'s records commonly carry an empty map early
//! in their life (e.g. a fresh user's `stats: {}`), this isn't a rare edge
//! case locally — it's most records, most of the time.
//!
//! Stream discovery (ListStreams/DescribeStream/GetShardIterator) doesn't
//! carry arbitrary item data, so it's unaffected and still goes through the
//! normal SDK client below. Only GetRecords needs a workaround: a raw signed
//! HTTP call, parsed as untyped JSON so a bare `{}` doesn't blow up
//! deserialization — patched to `{"M": {}}` (see `patch_empty_maps`) before
//! forwarding, so it's valid DynamoDB-JSON by the time `agon_worker`'s
//! `serde_dynamo` sees it, not just "happened to not crash this parser."
//!
//! DynamoDB Local doesn't validate SigV4 signature *content* (confirmed
//! empirically — a garbage signature is accepted same as a real one), so the
//! "signed" request below is real-shaped but not cryptographically real.
//! This only works against DynamoDB Local; it would need actual SigV4 to
//! work against real AWS, which is out of scope for a tool that only ever
//! exists to bridge a local table.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use aws_sdk_dynamodbstreams::types::ShardIteratorType;
use serde_json::{Value, json};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const SHARD_DISCOVERY_INTERVAL: Duration = Duration::from_secs(15);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let table_name = std::env::var("AGON_TABLE_NAME").unwrap_or_else(|_| "agon".to_string());
    let queue_url = std::env::var("AGON_EVENTS_QUEUE_URL")
        .expect("AGON_EVENTS_QUEUE_URL must be set (the local SQS queue to forward records to)");
    let streams_endpoint = std::env::var("AWS_ENDPOINT_URL_DYNAMODB_STREAMS")
        .expect("AWS_ENDPOINT_URL_DYNAMODB_STREAMS must be set (DynamoDB Local's address)");
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "local".to_string());

    let config = aws_config::load_from_env().await;
    let streams = aws_sdk_dynamodbstreams::Client::new(&config);
    let sqs = aws_sdk_sqs::Client::new(&config);
    let http = reqwest::Client::new();

    let stream_arn = latest_stream_arn(&streams, &table_name)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "no active stream for table `{table_name}`: {e:?} — did local/dynamodb/create-table.sh run, and is streaming enabled on it?"
            )
        });
    tracing::info!(%stream_arn, %queue_url, "bridging DynamoDB stream to SQS");

    // shard id -> current iterator (None once exhausted/closed, awaiting removal)
    let mut shard_iterators: HashMap<String, Option<String>> = HashMap::new();
    let mut last_discovery = std::time::Instant::now() - SHARD_DISCOVERY_INTERVAL;

    loop {
        if last_discovery.elapsed() >= SHARD_DISCOVERY_INTERVAL {
            discover_shards(&streams, &stream_arn, &mut shard_iterators).await;
            last_discovery = std::time::Instant::now();
        }

        let mut any_records = false;
        let shard_ids: Vec<String> = shard_iterators.keys().cloned().collect();
        for shard_id in shard_ids {
            let Some(Some(iterator)) = shard_iterators.get(&shard_id).cloned() else {
                continue;
            };

            match get_records(&http, &streams_endpoint, &region, &iterator).await {
                Ok((records, next_iterator)) => {
                    if !records.is_empty() {
                        any_records = true;
                    }
                    for record in &records {
                        if let Err(e) = forward(&sqs, &queue_url, record).await {
                            tracing::error!(error = ?e, "failed to forward record; dropping it");
                        }
                    }
                    // A closed shard's final GetRecords response has no
                    // next iterator — stop polling it. Its children (a
                    // shard split) show up on the next discovery pass.
                    shard_iterators.insert(shard_id, next_iterator);
                }
                Err(e) => {
                    tracing::warn!(error = ?e, %shard_id, "get_records failed; will retry");
                }
            }
        }

        shard_iterators.retain(|_, it| it.is_some());
        if !any_records {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
}

/// Finds the table's current stream ARN.
/// `local/dynamodb/create-table.sh` enables streaming at creation.
async fn latest_stream_arn(
    streams: &aws_sdk_dynamodbstreams::Client,
    table_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let out = streams
        .list_streams()
        .table_name(table_name)
        .send()
        .await?;
    let arn = out
        .streams()
        .iter()
        .filter_map(|s| s.stream_arn())
        .last()
        .map(str::to_string);
    arn.ok_or_else(|| "no stream found".into())
}

/// Adds shard iterators for any shard not already tracked. New shards start
/// from `LATEST` (skip history); a shard already in the map keeps whatever
/// iterator it has (possibly `None` if it just closed, cleaned up above).
async fn discover_shards(
    streams: &aws_sdk_dynamodbstreams::Client,
    stream_arn: &str,
    shard_iterators: &mut HashMap<String, Option<String>>,
) {
    let out = match streams.describe_stream().stream_arn(stream_arn).send().await {
        Ok(out) => out,
        Err(e) => {
            tracing::warn!(error = ?e, "describe_stream failed; will retry");
            return;
        }
    };
    let Some(desc) = out.stream_description() else {
        return;
    };
    if !matches!(
        desc.stream_status(),
        Some(aws_sdk_dynamodbstreams::types::StreamStatus::Enabled)
    ) {
        return;
    }

    let known: HashSet<String> = shard_iterators.keys().cloned().collect();
    for shard in desc.shards() {
        let Some(shard_id) = shard.shard_id() else {
            continue;
        };
        if known.contains(shard_id) {
            continue;
        }
        let iter_out = streams
            .get_shard_iterator()
            .stream_arn(stream_arn)
            .shard_id(shard_id)
            .shard_iterator_type(ShardIteratorType::Latest)
            .send()
            .await;
        match iter_out {
            Ok(it) => {
                tracing::info!(%shard_id, "tracking new shard");
                shard_iterators.insert(shard_id.to_string(), it.shard_iterator().map(str::to_string));
            }
            Err(e) => tracing::warn!(error = ?e, %shard_id, "get_shard_iterator failed; skipping for now"),
        }
    }
}

/// Raw GetRecords call — see the module doc comment for why this doesn't use
/// the generated SDK client. Returns `(records, next_shard_iterator)`.
async fn get_records(
    http: &reqwest::Client,
    endpoint: &str,
    region: &str,
    shard_iterator: &str,
) -> Result<(Vec<Value>, Option<String>), Box<dyn std::error::Error>> {
    // DynamoDB Local checks this header's *shape*, not its cryptographic
    // validity (confirmed empirically — see the module doc comment), so a
    // fixed, plausible-looking value is sufficient; it doesn't need a real
    // timestamp or a real signature.
    let resp = http
        .post(endpoint)
        .header("content-type", "application/x-amz-json-1.0")
        .header("x-amz-target", "DynamoDBStreams_20120810.GetRecords")
        .header(
            "authorization",
            format!(
                "AWS4-HMAC-SHA256 Credential=local/00000000/{region}/dynamodb/aws4_request, \
                 SignedHeaders=content-type;host;x-amz-target, Signature=0"
            ),
        )
        .json(&json!({ "ShardIterator": shard_iterator }))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;

    let records = resp
        .get("Records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let next_iterator = resp
        .get("NextShardIterator")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok((records, next_iterator))
}

/// Builds the envelope `agon_worker/src/event.rs`'s `Envelope` deserializes
/// (see that module's doc comment) and sends it as one SQS message. Images
/// come through as raw JSON straight off the stream response — see the
/// module doc comment for why, and `patch_empty_maps` for the one fixup
/// they need.
async fn forward(
    sqs: &aws_sdk_sqs::Client,
    queue_url: &str,
    record: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let ddb = record.get("dynamodb").ok_or("record missing dynamodb")?;
    let keys = ddb.get("Keys").ok_or("record missing keys")?;
    let pk = keys
        .get("PK")
        .and_then(|v| v.get("S"))
        .and_then(Value::as_str)
        .ok_or("missing PK")?;
    let sk = keys
        .get("SK")
        .and_then(|v| v.get("S"))
        .and_then(Value::as_str)
        .ok_or("missing SK")?;
    let event_name = record
        .get("eventName")
        .and_then(Value::as_str)
        .ok_or("record missing eventName")?;

    let mut old_image = ddb.get("OldImage").cloned().unwrap_or(Value::Null);
    let mut new_image = ddb.get("NewImage").cloned().unwrap_or(Value::Null);
    patch_image(&mut old_image);
    patch_image(&mut new_image);

    let envelope = json!({
        "event": event_name,
        "pk": pk,
        "sk": sk,
        "old_image": old_image,
        "new_image": new_image,
    });

    sqs.send_message()
        .queue_url(queue_url)
        .message_body(envelope.to_string())
        .send()
        .await?;
    Ok(())
}

/// The entry point for a top-level image (`OldImage`/`NewImage`): a *bare*
/// map of attribute name -> attribute value, with no `{"M": ...}` wrapper of
/// its own — unlike everything `patch_empty_maps` recurses into below, which
/// only ever sees already-wrapped attribute values. Missing this distinction
/// was the bug in the first version of this fix: patching only ran on values
/// already inside an `M`/`L`, so a field directly on the image (`stats: {}`,
/// not nested in anything) was never touched.
fn patch_image(image: &mut Value) {
    let Value::Object(map) = image else {
        return;
    };
    for v in map.values_mut() {
        patch_empty_maps(v);
    }
}

/// Recursively rewrites every bare `{}` found where a DynamoDB attribute
/// value is expected into `{"M": {}}` — see the module doc comment. Walks
/// into `M` (map) and `L` (list) values, the only two attribute types that
/// can nest others.
fn patch_empty_maps(value: &mut Value) {
    let Value::Object(map) = value else {
        return;
    };
    if map.is_empty() {
        *value = json!({ "M": {} });
        return;
    }
    if let Some(inner) = map.get_mut("M").and_then(Value::as_object_mut) {
        for v in inner.values_mut() {
            patch_empty_maps(v);
        }
    }
    if let Some(inner) = map.get_mut("L").and_then(Value::as_array_mut) {
        for v in inner.iter_mut() {
            patch_empty_maps(v);
        }
    }
}
