#!/bin/sh
# Creates the `agon` table against DynamoDB Local, mirroring the real table's
# schema in agon_infra/index.ts (`dynamoTable`) — single table, PK/SK keys,
# three overloaded GSIs, TTL on `ttl`, streaming enabled (NEW_AND_OLD_IMAGES,
# same as real). Runs as a one-shot compose service (see docker-compose.yml's
# `dynamodb-local-init`); idempotent, so safe to re-run against an
# already-bootstrapped instance.
#
# If the real table's schema ever changes, mirror the change here too — this
# has no way to detect drift from the Pulumi definition automatically.
set -eu

ENDPOINT="${AWS_ENDPOINT_URL:-http://dynamodb-local:8000}"

# `depends_on` (no healthcheck on dynamodb-local — see docker-compose.yml)
# only waits for the container to start, not for the JVM inside it to be
# accepting connections yet. Poll instead of racing it.
until aws dynamodb list-tables --endpoint-url "$ENDPOINT" >/dev/null 2>&1; do
  echo "Waiting for DynamoDB Local at $ENDPOINT..."
  sleep 1
done

aws dynamodb create-table \
  --endpoint-url "$ENDPOINT" \
  --table-name agon \
  --billing-mode PAY_PER_REQUEST \
  --attribute-definitions \
    AttributeName=PK,AttributeType=S \
    AttributeName=SK,AttributeType=S \
    AttributeName=GSI1PK,AttributeType=S \
    AttributeName=GSI1SK,AttributeType=S \
    AttributeName=GSI2PK,AttributeType=S \
    AttributeName=GSI2SK,AttributeType=S \
    AttributeName=GSI3PK,AttributeType=S \
    AttributeName=GSI3SK,AttributeType=S \
  --key-schema \
    AttributeName=PK,KeyType=HASH \
    AttributeName=SK,KeyType=RANGE \
  --global-secondary-indexes \
    'IndexName=GSI1,KeySchema=[{AttributeName=GSI1PK,KeyType=HASH},{AttributeName=GSI1SK,KeyType=RANGE}],Projection={ProjectionType=ALL}' \
    'IndexName=GSI2,KeySchema=[{AttributeName=GSI2PK,KeyType=HASH},{AttributeName=GSI2SK,KeyType=RANGE}],Projection={ProjectionType=ALL}' \
    'IndexName=GSI3,KeySchema=[{AttributeName=GSI3PK,KeyType=HASH},{AttributeName=GSI3SK,KeyType=RANGE}],Projection={ProjectionType=ALL}' \
  --stream-specification StreamEnabled=true,StreamViewType=NEW_AND_OLD_IMAGES \
  >/dev/null 2>&1 && echo "Created table 'agon'" || echo "Table 'agon' already exists"

# TTL isn't part of CreateTable (real AWS or Local) — a separate call, same as
# Pulumi's `ttl: {...}` on the table resource issues under the hood.
aws dynamodb update-time-to-live \
  --endpoint-url "$ENDPOINT" \
  --table-name agon \
  --time-to-live-specification "Enabled=true,AttributeName=ttl" \
  >/dev/null 2>&1 || true

echo "DynamoDB Local ready at $ENDPOINT"
