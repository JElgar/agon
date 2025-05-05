# Agon

## Run


```
cp .env.example .env
docker compose up -d
make run
```

## Test

TODO Fix

Currently the first time you run make test, there is a bit of a weird dependency issue.

The test package expects the generated openapi client package to exists. When you try to generate the schema with the service package it fails because the tests package is failing.

```
make test
```
