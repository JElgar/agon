# Load environment from .env
ifneq (,$(wildcard ./.env))
	include .env
	export
endif

init:
	[[ -d openapi_client ]] || cargo new --lib --name openapi openapi_client

generate-schema:
	make init
	cargo run -p agon_service -- generate-schema
	openapi-generator-cli generate -i schema.json -g rust -o openapi_client
	echo "disable_all_formatting = true" > openapi_client/.rustfmt.toml
	# Post-process: for discriminated unions the enum is `#[serde(tag = "type")]`
	# (serde consumes `type` to pick the variant), but the generator ALSO emits a
	# required `type` field on each variant struct — so deserializing fails with
	# "missing field `type`". Add `#[serde(default)]` so the (single-valued,
	# already-correct) field defaults when absent. See docs/openapi-client.md.
	find openapi_client/src/models -name '*.rs' -exec \
		perl -0pi -e 's/#\[serde\(rename = "type"\)\]\n(\s*)pub r#type: Type,/#[serde(rename = "type", default)]\n$1pub r#type: Type,/g' {} +

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
