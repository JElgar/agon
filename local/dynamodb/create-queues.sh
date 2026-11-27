#!/bin/sh
# Creates the SQS queues agon_worker needs (agon_worker/src/config.rs —
# AGON_EVENTS_QUEUE_URL, AGON_ASSET_EVENTS_QUEUE_URL, AGON_ACTIONS_QUEUE_URL,
# plus the optional AGON_OPENSEARCH_SYNC_QUEUE_URL) against the local
# ElasticMQ instance. Runs as a one-shot compose service (see
# docker-compose.yml's `elasticmq-init`); idempotent.
#
# `agon-asset-events` never receives real messages locally — nothing feeds it
# (that's S3 -> EventBridge -> SQS in production; there's no local S3 here).
# `agon-actions` is required (every fanned-out handler goes through it, see
# `agon_worker::action_consumer`) so it has to exist here too, same reasoning
# as `agon-asset-events`. `agon-opensearch-sync` is created unconditionally as
# well, even though it's only used when OPENSEARCH_URL is also set, since it's
# cheap to always have and saves a second conditional path in this script.
set -eu

ENDPOINT="${AWS_ENDPOINT_URL_SQS:-http://elasticmq:9324}"

until aws sqs list-queues --endpoint-url "$ENDPOINT" >/dev/null 2>&1; do
  echo "Waiting for ElasticMQ at $ENDPOINT..."
  sleep 1
done

for q in agon-events agon-asset-events agon-actions agon-opensearch-sync; do
  aws sqs create-queue --endpoint-url "$ENDPOINT" --queue-name "$q" >/dev/null 2>&1 \
    && echo "Created queue '$q'" || echo "Queue '$q' already exists"
done

echo "ElasticMQ ready at $ENDPOINT"
