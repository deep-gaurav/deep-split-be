#!/bin/bash
set -x

# Health check function to make GraphQL API call
health_check() {
 local response=$(curl -s -X POST \
  -H "Content-Type: application/json" \
  -d '{"query": "{ currencies { displayName } }"}' \
  http://localhost:33371)
  
 # Check if the 'currencies' array is not empty in the response
 if [[ $response == *"currencies"* && $response != *"[]"* ]]; then
  echo "Health check passed. Server is running."
 else
  echo "Health check failed. Server might not be running or didn't return expected data."
  exit 1
 fi
}


# Create named volumes if they don't exist
podman volume create data || true
podman volume create deepsplit_be || true

# Backup volumes before making changes
podman volume backup data backup_data.tar || true
podman volume backup deepsplit_be backup_deepsplit_be.tar || true

# Stop and remove existing containers (if any)
podman stop litestream-dev deepsplit_be-dev || true
podman rm litestream-dev deepsplit_be-dev || true

# Check if the database already exists in the volume
if podman volume inspect data | grep -q '"data/deepsplit.sqlite"'; then
  echo "Using existing database from volume"
else
  # Try to restore database from S3
  podman run --rm \
    -v data:/data \
    -v config:/config \
    litestream/litestream:latest \
    restore -o /data/deepsplit-restored.sqlite -config /config/litestream.yml -if-replica-exists /data/deepsplit.sqlite

  # Check if restoration succeeded
  if podman volume inspect data | grep -q '"data/deepsplit-restored.sqlite"'; then
    echo "Database restored from S3"
    podman exec -i litestream-dev mv /data/deepsplit-restored.sqlite /data/deepsplit.sqlite
  else
    # Create a new database within the volume
    podman exec -i litestream-dev touch /data/deepsplit.sqlite
    echo "Empty database created in volume"
  fi
fi

# Second Stage: Run backend (using named volume and copying local files)
podman run -d \
  --name deepsplit_be-dev \
  -v data:/data \
  -v config:/config \
  -v deepsplit_be:/deepsplit_be \
  --env-file $(pwd)/.env \
  -e DATABASE_URL=sqlite:/data/deepsplit.sqlite \
  -e GEO_ASN_COUNTRY_CSV=/config/geo-whois-asn-country-ipv4-num.csv \
  -e SERVICE_JSON=/config/billdivide-app-firebase-adminsdk-99qtd-ecb3c349aa.json \
  -p 127.0.0.1:33371:8000 \
  alpine:latest \
  sh -c "cp -r /deepsplit_be/* /deepsplit_be/ && printenv && /deepsplit_be"


# Health check after starting the containers
health_check

if [ $? -ne 0 ]; then
  echo "Health check failed. Litestream container (litestream-dev) will not be started."
  podman volume restore --force backup_data.tar
  podman volume restore --force backup_deepsplit_be.tar
  # Restart containers to use the restored volumes
  podman restart deepsplit_be-dev
  podman run -d \
    --name litestream-dev \
    -v data:/data \
    -v config:/config \
    litestream/litestream:latest \
    replicate -config /config/litestream.yml

  echo "Containers restarted with restored volumes."
  exit 1

else
  echo "Health check passed. Starting Litestream container..."
  # Run the command to start litestream container here
  # Example:
  podman run -d \
    --name litestream-dev \
    -v data:/data \
    -v config:/config \
    litestream/litestream:latest \
    replicate -config /config/litestream.yml
fi