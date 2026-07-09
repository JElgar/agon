# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Architecture

Agon is a Rust-based team management API service built with the Poem web framework and poem-openapi. The project uses a workspace structure:

- **agon_core**: Shared library — the DAO (DynamoDB single-table access), search client, telemetry, and record types. Used by both binaries.
- **agon_service**: Main API service using Poem framework with OpenAPI documentation
- **agon_worker**: Async worker that consumes the DynamoDB stream (indexing to search, feed fan-out)
- **agon_tests**: Integration tests that use the generated OpenAPI client
- **agon_infra**: Pulumi-based infrastructure configuration for deployment
- **openapi_client**: Auto-generated Rust client from OpenAPI schema

### Core Technologies
- **Web Framework**: Poem with poem-openapi for automatic OpenAPI generation
- **Database**: DynamoDB, single-table design (`AGON_TABLE_NAME`, default `agon`). Accessed via `aws_sdk_dynamodb` in the `agon_core` DAO. See `docs/dynamodb-design.md`.
- **Search**: Meilisearch powers the discovery/search endpoints (users, teams, matches), kept in sync by `agon_worker`.
- **Authentication**: JWT bearer tokens validated via custom security scheme
- **Infrastructure**: Pulumi with TypeScript for cloud deployment
- **Observability**: OTLP export (logs/traces/metrics) from both binaries via
  `agon_core::telemetry`, to a self-hosted Grafana + Loki/Tempo/Prometheus
  stack. See `docs/observability.md`. Export is off unless
  `OTEL_EXPORTER_OTLP_ENDPOINT` is set, so local runs log JSON to stdout only.

### Data Model
All entities live in one DynamoDB table addressed by typed `Pk` + `Sk` keys (see
`agon_core/src/dao/keys.rs`), e.g. a user profile is `USER#<id>` / `#PROFILE`.
Users, teams, matches, memberships, follows, likes, comments, notifications and
feed entries are all item collections in this table; relationships are modeled
as edge items and GSI projections rather than SQL joins. Identity: the JWT `sub`
is mapped to a stable internal user id via an `AUTH#<sub>` guard item — the
internal id (not the `sub`) is what every other key references, so the auth
provider can change without rewriting user-keyed data.

## Development Commands

### Setup
```bash
cp .env.example .env
docker compose up -d  # Starts local Meilisearch (DynamoDB is a real/cloud table)
```

### Building
```bash
make build  # Generates OpenAPI schema and builds entire workspace
```

### Running the Service
```bash
make run  # Starts Docker services and runs the API server on port 7000
```

### Testing
```bash
make test  # Runs integration tests in agon_tests package
```

### OpenAPI Schema Generation
```bash
make generate-schema  # Generates schema.json and rebuilds openapi_client
```

## Key Implementation Details

### Authentication Flow
- JWT tokens validated using `JWT_SECRET` environment variable
- The JWT `sub` claim is resolved to the caller's stable internal user id via the
  `AUTH#<sub>` guard (`Dao::get_user_id_by_sub`); handlers use `require_uid` and
  key off that internal id, never the raw `sub`
- Signup (`POST /users`) mints a fresh internal id and writes the `AUTH#<sub>`
  mapping; the account email comes from the token's `email` claim, not the body
- All API endpoints (except `/ping`) require bearer token authentication

### Service Architecture
- **DAO Layer**: `agon_core/src/dao/` handles all DynamoDB operations (keys in `keys.rs`, record shapes in `records.rs`, per-entity ops in the sibling modules)
- **API Layer**: `agon_service/src/main.rs` defines OpenAPI endpoints and request/response types
- **Async Worker**: `agon_worker` consumes the DynamoDB stream for search indexing and feed fan-out
- **Auto-generated Client**: Tests use the generated client in `openapi_client/`

### Environment Configuration
Required environment variables (see `.env.example`):
- `AGON_TABLE_NAME`: DynamoDB table name (default `agon`)
- `AWS_REGION` / `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`: AWS credentials for DynamoDB
- `MEILI_URL` / `MEILI_MASTER_KEY`: Meilisearch connection
- `JWT_SECRET`: Secret key for JWT validation
- `AGON_SERVICE_URL`: Service URL for tests

### Database Operations
- Single-table DynamoDB access via `aws_sdk_dynamodb`; typed `Pk`/`Sk` keys, never hand-written key strings
- Uniqueness guards enforced with conditional puts (`EMAIL#<email>`, `AUTH#<sub>`); multi-item writes use `TransactWriteItems` (e.g. user + email-guard + auth-guard on signup)
- Custom ID generation using base64url-encoded random bytes (`new_id`)