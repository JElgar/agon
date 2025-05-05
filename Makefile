test:
	[[ -d openapi_client ]] || cargo new --lib --name openapi openapi_client
	cargo run -p agon_service -- generate-schema
	openapi-generator-cli generate -i schema.json -g rust -o openapi_client
	cargo test -p agon_tests

run:
	cargo run -p agon_service -- run-server abc.com

rest-db:
	cd agon_service && sqlx migrate revert && sqlx migrate run
