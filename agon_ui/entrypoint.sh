#!/usr/bin/env sh

set -ex

echo "Starting entrypoint script"
echo "VITE_SUPABASE_URL ${VITE_SUPABASE_URL}"

# Find the file where environment variables need to be replaced.
runtimeEnvFile=$(ls -t /usr/share/nginx/html/assets/runtime-env*.js | head -n1)

# Replace environment variables
envsubst < "$runtimeEnvFile" > ./runtime-env_temp
cp ./runtime-env_temp "$runtimeEnvFile"
rm ./runtime-env_temp

nginx -g "daemon off;"
