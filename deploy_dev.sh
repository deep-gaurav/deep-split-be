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
podman volume create server_binary || true

# Backup volumes before making changes
podman volume export data -o backup_data.tar || true
podman volume export server_binary -o backup_server_binary.tar || true

# Stop and remove existing containers (if any)
podman stop litestream-dev deepsplit_be-dev || true
podman rm deepsplit_be-dev || true

podman create \
  --name litestream-dev \
  -v data:/data \
  -v $(pwd)/config:/config \
  litestream/litestream:latest \
  replicate -config /config/litestream.yml || true

podman run --rm \
  -v data:/data \
  -v $(pwd)/config:/config \
  litestream/litestream:latest \
  restore -o /data/deepsplit-restored.sqlite -config /config/litestream.yml -if-replica-exists /data/deepsplit.sqlite


podman run --rm \
    -v data:/data \
    alpine:latest \
    sh -c '\
        if [ -f /data/deepsplit.sqlite ]; then \
            echo "Using existing database from volume"; \
        else \
            if [ -f /data/deepsplit-restored.sqlite ]; then \
                echo "Database restored from S3"; \
                mv /data/deepsplit-restored.sqlite /data/deepsplit.sqlite; \
            else \
                touch /data/deepsplit.sqlite; \
                echo "Empty database created"; \
            fi; \
        fi'

# Copy new binary to server
podman run --rm \
    -v $(pwd)/deepsplit_be:/deepsplit_be \
    -v server_binary:/server_binary \
    alpine:latest \
    sh -c "cp /deepsplit_be /server_binary/deepsplit_be"

# Second Stage: Run backend 
podman run -d \
  --name deepsplit_be-dev \
  -v data:/data \
  -v $(pwd)/config:/config \
  -v server_binary:/server_binary \
  --env-file $(pwd)/.env \
  -e DATABASE_URL=sqlite:/data/deepsplit.sqlite \
  -e GEO_ASN_COUNTRY_CSV=/config/geo-whois-asn-country-ipv4-num.csv \
  -e SERVICE_JSON=/config/billdivide-app-firebase-adminsdk-99qtd-ecb3c349aa.json \
  -p 127.0.0.1:33371:8000 \
  alpine:latest \
  sh -c "printenv && /server_binary/deepsplit_be"


# Health check after starting the containers
health_check

if [ $? -ne 0 ]; then
  echo "Health check failed. Litestream container (litestream-dev) will not be started."
  # Restore old data
  podman volume import backup_data.tar

  # Restore old binary
  podman volume import backup_server_binary.tar

  # Restart containers backend container with restored volumes
  podman restart deepsplit_be-dev

  # Start litestream container 
  podman start -d litestream-dev

  echo "Containers restarted with restored volumes."
  exit 1

else
  echo "Health check passed. Starting Litestream container..."
  # Run the command to start litestream container since it wasnt started anywhere
  podman start litestream-dev
  rm backup_server_binary.tar || true
  rm backup_data.tar || true
fi