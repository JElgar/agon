#!/usr/bin/env sh

set -ex

# Find the file where environment variables need to be replaced.
runtimeEnvFile=$(ls -t /usr/share/nginx/html/assets/runtime-env*.js | head -n1)

# Replace environment variables
envsubst < "$runtimeEnvFile" > ./runtime-env_temp
cp ./runtime-env_temp "$runtimeEnvFile"
rm ./runtime-env_temp
