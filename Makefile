# Load environment from .env
ifneq (,$(wildcard ./.env))
	include .env
	export
endif

# The dev-only test-signing private key, for agon_tests / `generate-token`.
# Deliberately NOT in .env: .env is also read directly by `docker compose`
# (auto-loaded for variable substitution) whose parser rejects a `define`/
# `endef` block outright ("key cannot contain a space") — and without that,
# Make's own simple `VAR=value` line-based parsing has no way to hold a real
# multi-line PEM (a `"...\n...\n..."`-escaped single line looks appealing but
# doesn't work either: neither Make's `include` nor the Rust code that reads
# this var unescape `\n`, so it fails to parse as PEM — this used to be
# .env.example's documented format, and it was always broken for local use).
# A plain file has none of these problems. `?=` so `test-staging`'s own
# recipe-level override (a real key from Pulumi) still wins.
AGON_TEST_JWT_PRIVATE_KEY ?= $(shell cat local/agon-test-key.pem 2>/dev/null)

init:
	[[ -d openapi_client ]] || cargo new --lib --name openapi openapi_client

generate-schema:
	make init
	cargo run -p agon_service -- generate-schema
	openapi-generator-cli generate -i schema.json -g rust -o openapi_client
	echo "disable_all_formatting = true" > openapi_client/.rustfmt.toml
	# Post-process: for a discriminated union whose variants are flat objects
	# (not another nested union — see the `LiveEventInput` fix below for
	# that case), the enum is `#[serde(tag = "<name>")]` (serde consumes
	# `<name>` to pick the variant), but the generator ALSO emits a required
	# `<name>` field on each variant struct — so deserializing can fail
	# (`missing field`, or — once the variant has enough other fields that
	# the same broken reconstruction misaligns a *different*, non-optional
	# one instead — a spurious type-mismatch on that field, e.g. "invalid
	# type: null, expected a string" on a plain required `side_id: String`
	# that was never actually null on the wire). Add `#[serde(default)]` so
	# the (single-valued, already-correct) discriminator field defaults
	# when the reconstruction doesn't see it. This API uses three such
	# discriminator names: `type` (e.g. `Score`), `sport` (`MatchFormat`),
	# and `kind` (the football/cricket/netball live-event unions nested
	# inside `LiveEventInput`) — all three need the same fix, not just
	# `type`. See docs/openapi-client.md.
	find openapi_client/src/models -name '*.rs' -exec \
		perl -0pi -e 's/#\[serde\(rename = "type"\)\]\n(\s*)pub r#type: Type,/#[serde(rename = "type", default)]\n$$1pub r#type: Type,/g' {} +
	find openapi_client/src/models -name '*.rs' -exec \
		perl -0pi -e 's/#\[serde\(rename = "sport"\)\]\n(\s*)pub sport: Sport,/#[serde(rename = "sport", default)]\n$$1pub sport: Sport,/g' {} +
	find openapi_client/src/models -name '*.rs' -exec \
		perl -0pi -e 's/#\[serde\(rename = "kind"\)\]\n(\s*)pub kind: Kind,/#[serde(rename = "kind", default)]\n$$1pub kind: Kind,/g' {} +
	# Post-process: `LiveEventInput` nests a second discriminated union (each
	# sport's own `kind`-tagged event union) inside its own `sport`-tagged
	# variants. The generator handles that inner `oneOf` correctly wherever
	# it's referenced directly (`FootballLiveEvent`/`CricketLiveEvent`/
	# `NetballLiveEvent` each come out as a proper enum with every kind), but
	# for the *outer* variant it instead flattens `allOf[{sport}, {$ref to
	# the inner oneOf}]` into one struct that merges every kind's fields
	# together and collapses the inner discriminator down to a single-value
	# enum (whichever kind happened to be generated last) — so deserializing
	# any other kind fails with "unknown variant". Point the outer variants
	# at the correctly-generated standalone enums instead of the broken
	# merged structs; serde's internally-tagged (`tag = "..."`) enums nest
	# fine (both discriminators live on the same flat JSON object), so this
	# is a pure type-reference swap, no behavior change beyond fixing the
	# bug. See docs/openapi-client.md.
	perl -pi -e 's/models::LiveEventInput(Football|Cricket|Netball)LiveEvent/models::$$1LiveEvent/g' \
		openapi_client/src/models/live_event_input.rs

generate:
	make generate-schema
	cd agon_ui && npm run generate

build:
	make generate-schema
	cargo build

test:
	cargo test --manifest-path agon_tests/Cargo.toml

# Run the integration tests against a deployed environment. Fetches the ES256
# test signing key from the Pulumi stack (Pulumi is the single source of truth);
# the deployed service trusts its matching public JWK via `agonStaticJwks`, so
# tokens the tests mint are accepted. Override the target env:
#   make test-staging STAGING_URL=https://agon.staging.get-agon.com/api STACK=staging
STACK ?= staging
STAGING_URL ?= https://agon.staging.get-agon.com/api

test-staging:
	AGON_SERVICE_URL=$(STAGING_URL) \
	AGON_TEST_JWT_PRIVATE_KEY="$$(cd agon_infra && pulumi config get agonTestJwtPrivateKey --stack $(STACK))" \
	cargo test --manifest-path agon_tests/Cargo.toml -- --test-threads=1

run:
	cargo run -p agon_service -- run-server abc.com

# agon_worker: consumes the DynamoDB stream + runs the Temporal workflows
# (feed fan-out, accept-invitation). Needs the `full` docker-compose profile
# up (Temporal) — see local/README.md. Requires protobuf-compiler (`protoc`)
# to build, unlike agon_service. Locally, also needs run-stream-bridge
# running alongside it (a separate terminal) or it'll just idle — see that
# target's comment.
run-worker:
	cargo run -p agon_worker

# Local stand-in for the EventBridge Pipe in front of agon_worker's queue —
# see local/dynamodb-stream-bridge/src/main.rs's doc comment for what it does
# and why it's a separate tool rather than a compose service. Needs the
# `dynamodb` (or `full`) docker-compose profile up. A separate, non-workspace
# crate (see its Cargo.toml) — first run compiles its own dependency tree.
run-stream-bridge:
	cargo run --manifest-path local/dynamodb-stream-bridge/Cargo.toml --release

# Full browser UI end-to-end tests (Playwright) — see agon_ui/e2e/README.md
# for what's covered and how the test account works. Reads E2E_TEST_EMAIL /
# E2E_TEST_PASSWORD / E2E_BASE_URL from the environment (or .env); use
# test-ui-e2e-staging below to pull the test account from Pulumi instead.
test-ui-e2e:
	npm --prefix agon_ui run test:e2e

# Same, against the deployed staging UI, fetching the fixed test account from
# Pulumi config (single source of truth — see e2eTestEmail/e2eTestPassword in
# agon_infra/index.ts, same pattern as test-staging's signing key above).
UI_STAGING_URL ?= https://agon.staging.get-agon.com

test-ui-e2e-staging:
	E2E_BASE_URL=$(UI_STAGING_URL) \
	E2E_TEST_EMAIL="$$(cd agon_infra && pulumi config get e2eTestEmail --stack $(STACK))" \
	E2E_TEST_PASSWORD="$$(cd agon_infra && pulumi config get e2eTestPassword --stack $(STACK))" \
	E2E_SECONDARY_EMAIL="$$(cd agon_infra && pulumi config get e2eSecondaryEmail --stack $(STACK))" \
	npm --prefix agon_ui run test:e2e

# Same, against a fully local stack instead — see agon_ui/e2e/README.md's
# "Running fully local" section for the full setup (docker-compose `full`
# profile, agon_service + agon_worker running locally, agon_ui/.env pointed
# at it). This only seeds the local Supabase test accounts and runs the
# suite; it doesn't bring up the rest of the stack for you.
E2E_LOCAL_EMAIL ?= e2e@example.com
E2E_LOCAL_PASSWORD ?= local-e2e-test-password
E2E_LOCAL_SECONDARY_EMAIL ?= e2e-2@example.com

test-ui-e2e-local:
	E2E_TEST_EMAIL=$(E2E_LOCAL_EMAIL) E2E_TEST_PASSWORD=$(E2E_LOCAL_PASSWORD) \
	node agon_ui/e2e/local-seed-user.mjs
	E2E_TEST_EMAIL=$(E2E_LOCAL_SECONDARY_EMAIL) E2E_TEST_PASSWORD=$(E2E_LOCAL_PASSWORD) \
	node agon_ui/e2e/local-seed-user.mjs
	E2E_TEST_EMAIL=$(E2E_LOCAL_EMAIL) E2E_TEST_PASSWORD=$(E2E_LOCAL_PASSWORD) \
	E2E_SECONDARY_EMAIL=$(E2E_LOCAL_SECONDARY_EMAIL) \
	npm --prefix agon_ui run test:e2e
