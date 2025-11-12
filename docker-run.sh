#!/bin/bash

set -e

export TGI_RPCSERVER=${TGI_RPCSERVER-"localhost"}
export POSTGRES_USER=${POSTGRES_USER-postgres}
export POSTGRES_PASSWORD=${POSTGRES_PASSWORD-postgres}
export POSTGRES_DB=${POSTGRES_DB-postgres}
export API_ADDRESS=${API_ADDRESS-"localhost"}
export KATNIP_ADDRESS=${KATNIP_ADDRESS-explorer.tondi.org}
export API_PORT=${API_PORT-4575}
export WEB_PORT=${WEB_PORT-8080}
export TONDID_VERSION=${TONDID_VERSION-latest}
export TONDI_LIVE_ADDRESS=${TONDI_LIVE_ADDRESS-tondi.live}
export REACT_APP_EXPLORER_ADDRESS=${REACT_APP_EXPLORER_ADDRESS-explorer.tondi.org}
export TGI_POSTGRES_MOUNT=${TGI_POSTGRES_MOUNT-"~/.tgi-postgres"}

# Verify that all the required environment variables are set
declare -A REQUIRED_VARIABLES
REQUIRED_VARIABLES["POSTGRES_USER"]="${POSTGRES_USER}"
REQUIRED_VARIABLES["POSTGRES_PASSWORD"]="${POSTGRES_PASSWORD}"
REQUIRED_VARIABLES["POSTGRES_DB"]="${POSTGRES_DB}"
REQUIRED_VARIABLES["API_ADDRESS"]="${API_ADDRESS}"
REQUIRED_VARIABLES["KATNIP_ADDRESS"]="${KATNIP_ADDRESS}"
REQUIRED_VARIABLES["API_PORT"]="${API_PORT}"
REQUIRED_VARIABLES["WEB_PORT"]="${WEB_PORT}"
REQUIRED_VARIABLES["TONDID_VERSION"]="${TONDID_VERSION}"
REQUIRED_VARIABLES["TONDI_LIVE_ADDRESS"]="${TONDI_LIVE_ADDRESS}"

REQUIRED_VARIABLE_NOT_SET=false
for REQUIRED_VARIABLE_NAME in "${!REQUIRED_VARIABLES[@]}"; do
  if [ -z "${REQUIRED_VARIABLES[$REQUIRED_VARIABLE_NAME]}" ]; then
    echo "${REQUIRED_VARIABLE_NAME} is not set";
    REQUIRED_VARIABLE_NOT_SET=true
    fi
done

if [ true = "${REQUIRED_VARIABLE_NOT_SET}" ]; then
  echo
  echo "The following environment variables are required:"
  for REQUIRED_VARIABLE_NAME in "${!REQUIRED_VARIABLES[@]}"; do
    echo "${REQUIRED_VARIABLE_NAME}"
  done
  exit 1
fi

# Build tondi-graph-inspector
./docker-build.sh

function docker_compose() {
    if ! command -v docker-compose &> /dev/null
    then
      docker compose "$@"
    else
      docker-compose "$@"
    fi
}

# Start postgres
docker_compose up -d persistent-postgres

# Wait for postgres to finish initializing
sleep 10s

# Start processing, api, and web
docker_compose up -d processing
docker_compose up -d api
docker_compose up -d web

# Print logs for all services
docker_compose logs -f
