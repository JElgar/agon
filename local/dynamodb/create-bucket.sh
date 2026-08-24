#!/bin/sh
# Creates the `agon-assets` bucket against MinIO — matches
# AGON_ASSETS_BUCKET's default (see .env.example). Runs as a one-shot
# compose service (see docker-compose.yml's `minio-init`); idempotent.
set -eu

ENDPOINT="${AWS_ENDPOINT_URL_S3:-http://minio:9000}"

until aws s3api list-buckets --endpoint-url "$ENDPOINT" >/dev/null 2>&1; do
  echo "Waiting for MinIO at $ENDPOINT..."
  sleep 1
done

aws s3api create-bucket --endpoint-url "$ENDPOINT" --bucket agon-assets >/dev/null 2>&1 \
  && echo "Created bucket 'agon-assets'" || echo "Bucket 'agon-assets' already exists"

echo "MinIO ready at $ENDPOINT"
