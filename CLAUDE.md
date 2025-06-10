# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Architecture

Agon is a Rust-based team management API service built with the Poem web framework and poem-openapi. The project uses a workspace structure with three main components:

- **agon_service**: Main API service using Poem framework with OpenAPI documentation
- **agon_tests**: Integration tests that use the generated OpenAPI client
- **agon_infra**: Pulumi-based infrastructure configuration for deployment
- **openapi_client**: Auto-generated Rust client from OpenAPI schema

### Core Technologies
- **Web Framework**: Poem with poem-openapi for automatic OpenAPI generation
- **Database**: PostgreSQL with SQLx for type-safe queries
- **Authentication**: JWT bearer tokens validated via custom security scheme
- **Migrations**: SQLx migrations in `agon_service/migrations/`
- **Infrastructure**: Pulumi with TypeScript for cloud deployment

### Database Schema
The service manages users and teams with a many-to-many relationship:
- `users`: User profiles linked by JWT subject claims
- `teams`: User-created teams with metadata
- `team_members`: Junction table for team membership

## Development Commands

### Setup
```bash
cp .env.example .env
docker compose up -d  # Starts PostgreSQL and Adminer
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

### Database Management
```bash
make reset-db  # Reverts and re-runs migrations
cd agon_service && sqlx migrate run  # Run pending migrations
```

### OpenAPI Schema Generation
```bash
make generate-schema  # Generates schema.json and rebuilds openapi_client
```

## Key Implementation Details

### Authentication Flow
- JWT tokens validated using `JWT_SECRET` environment variable
- User identity extracted from JWT `sub` claim
- All API endpoints (except `/ping`) require bearer token authentication

### Service Architecture
- **DAO Layer**: `agon_service/src/dao/mod.rs` handles all database operations
- **API Layer**: `agon_service/src/main.rs` defines OpenAPI endpoints and request/response types
- **Auto-generated Client**: Tests use the generated client in `openapi_client/`

### Environment Configuration
Required environment variables (see `.env.example`):
- `DATABASE_URL`: PostgreSQL connection string
- `JWT_SECRET`: Secret key for JWT validation
- `AGON_SERVICE_URL`: Service URL for tests

### Database Operations
- Uses SQLx with compile-time checked queries
- Transaction support for complex operations (e.g., team creation with membership)
- Custom ID generation using base64-encoded random bytes