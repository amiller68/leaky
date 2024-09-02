#!/usr/bin/env bash

set -o errexit
set -o nounset

CHROMA_CONTAINER_NAME="blossom-chroma"
CHROMA_VOLUME_NAME="blossom-chroma-data"

CHROMA_DATABASE_URL="http://localhost:8000"

CONTAINER_RUNTIME="podman"
if which docker &>/dev/null; then
	CONTAINER_RUNTIME="docker"
fi

function database-url {
	echo ${CHROMA_DATABASE_URL}
}

function collection-name {
	echo "blossom-embeddings"
}

function run {
	start-chroma-container
}

# Helpers:

function start-chroma-container {
	ensure-chroma-container-exists
	${CONTAINER_RUNTIME} start ${CHROMA_CONTAINER_NAME}
}

function ensure-chroma-container-exists {
	${CONTAINER_RUNTIME} pull chromadb/chroma
	create-chroma-container
}

function create-chroma-container {
	if ${CONTAINER_RUNTIME} ps -a | grep -q ${CHROMA_CONTAINER_NAME} &>/dev/null; then
		return
	fi

	${CONTAINER_RUNTIME} volume create ${CHROMA_VOLUME_NAME} || true

	${CONTAINER_RUNTIME} run \
		--name ${CHROMA_CONTAINER_NAME} \
		--publish 8000:8000 \
		--volume ${CHROMA_VOLUME_NAME}:/chroma/.chroma/index \
		--detach \
		chromadb/chroma
}

function clean() {
	${CONTAINER_RUNTIME} stop ${CHROMA_CONTAINER_NAME} || true
	${CONTAINER_RUNTIME} rm -fv ${CHROMA_CONTAINER_NAME} || true
	${CONTAINER_RUNTIME} volume rm ${CHROMA_VOLUME_NAME} || true
}

$1
