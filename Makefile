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

generate:
	make generate-schema
	cd agon_ui && npm run generate

build:
	make generate-schema
	cargo build

test:
	cargo test -p agon_tests

run:
	docker compose up -d
	cargo run -p agon_service -- run-server abc.com

reset-db:
	cd agon_service && sqlx migrate revert && sqlx migrate run
