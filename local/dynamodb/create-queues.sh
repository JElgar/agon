#!/bin/sh
# Creates the two SQS queues agon_worker needs (agon_worker/src/config.rs —
# AGON_EVENTS_QUEUE_URL, AGON_ASSET_EVENTS_QUEUE_URL) against the local
# ElasticMQ instance. Runs as a one-shot compose service (see
# docker-compose.yml's `elasticmq-init`); idempotent.
#
# `agon-asset-events` never receives real messages locally — nothing feeds it
# (that's S3 -> EventBridge -> SQS in production; there's no local S3 here).
# It still has to exist: agon_worker's Config::from_env fails to start at all
# without both queue URLs set to *something* real.
set -eu

ENDPOINT="${AWS_ENDPOINT_URL_SQS:-http://elasticmq:9324}"

until aws sqs list-queues --endpoint-url "$ENDPOINT" >/dev/null 2>&1; do
  echo "Waiting for ElasticMQ at $ENDPOINT..."
  sleep 1
done

for q in agon-events agon-asset-events; do
  aws sqs create-queue --endpoint-url "$ENDPOINT" --queue-name "$q" >/dev/null 2>&1 \
    && echo "Created queue '$q'" || echo "Queue '$q' already exists"
done

echo "ElasticMQ ready at $ENDPOINT"
