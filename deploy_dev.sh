#!/bin/bash

# Change to the desired directory
cd ~/development/


# Store the current container IDs
litestream_container=$(podman ps -q -f name=litestream-dev)
deepsplit_container=$(podman ps -q -f name=deepsplit_be-dev)

# Function to restore previous containers
restore_containers() {
  if [ -n "$litestream_container" ]; then
    podman start $litestream_container
  fi

  if [ -n "$deepsplit_container" ]; then
    podman start $deepsplit_container
  fi
}

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
    restore_containers
    exit 1
  fi
}

# Stop and remove existing containers
podman stop litestream-dev deepsplit_be-dev 2>/dev/null
podman rm litestream-dev deepsplit_be-dev 2>/dev/null

# First Stage: Restore or create the database
podman run --rm \
  -v $(pwd)/data:/data \
  -v $(pwd)/config:/config \
  litestream/litestream:latest \
  restore -o /data/deepsplit-restored.sqlite -config /config/litestream.yml -if-replica-exists /data/deepsplit.sqlite

# Check if the restored database exists
if [ -f data/deepsplit-restored.sqlite ]; then
  echo "Database restored from S3"
  mv data/deepsplit-restored.sqlite data/deepsplit.sqlite
else
  # Check if the database already exists in the volume
  if [ -f data/deepsplit.sqlite ]; then
    echo "Using existing database from volume"
  else
    # Create a new database if none exists
    touch data/deepsplit.sqlite
    echo "Empty database created"
  fi
fi

# Second Stage: Run litestream (development version)
podman run -d \
  --name litestream-dev \
  -v $(pwd)/data:/data \
  -v $(pwd)/config:/config \
  litestream/litestream:latest \
  replicate -config /config/litestream.yml

# Second Stage: Run backend (development version)
podman run -d \
  --name deepsplit_be-dev \
  -v $(pwd)/data:/data \
  -v $(pwd)/config:/config \
  -v $(pwd)/deepsplit_be:/deepsplit_be \
  --env-file $(pwd)/.env \
  -e DATABASE_URL=sqlite:/data/deepsplit.sqlite \
  -e GEO_ASN_COUNTRY_CSV=/config/geo-whois-asn-country-ipv4-num.csv \
  -e SERVICE_JSON=/config/billdivide-app-firebase-adminsdk-99qtd-ecb3c349aa.json \
  -p 127.0.0.1:33371:8000 \
  alpine:latest \
  sh -c "printenv && /deepsplit_be"

# Health check after starting the containers
health_check

# Remove the previous containers after successful health check
if [ -n "$litestream_container" ]; then
  podman rm $litestream_container
fi

if [ -n "$deepsplit_container" ]; then
  podman rm $deepsplit_container
fi
