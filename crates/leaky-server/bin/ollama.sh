#!/usr/bin/env bash

set -o errexit
set -o nounset

OLLAMA_CONTAINER_NAME="blossom-ollama"
OLLAMA_VOLUME_NAME="blossom-ollama-data"

DATA_DIR="./data"

# Pull models that aren't apart of the ollama repo

# NOTE: Nous-hermes-2-pro is not available through ollama
#  We'll download the model from huggingface and build the model file ourselves
NOUS_HERMES_2_PRO_REPO="NousResearch/Hermes-2-Pro-Mistral-7B-GGUF"
NOUS_HERMES_2_FILE="Hermes-2-Pro-Mistral-7B.Q4_K_M.gguf"

# Supervisor
# Should be able to handle decision making, function calls, and high level reasoning
OLLAMA_SUPERVISOR_MODEL="blossom-supervisor"

# Conversational
# Will handle conversation and dialogue
OLLAMA_CONVERSATIONAL_MODEL="blossom-conversational"

# Embedding
OLLAMA_EMBEDDING_MODEL="blossom-embedding"

# Image
# Will handle image processing
OLLAMA_IMAGE_MODEL="blossom-image"

OLLAMA_SERVER_URL="http://localhost:11434"

CONTAINER_RUNTIME="podman"
if which docker &>/dev/null; then
	CONTAINER_RUNTIME="docker"
fi

function supervisor-model {
	echo ${OLLAMA_SUPERVISOR_MODEL}
}

function conversational-model {
	echo ${OLLAMA_CONVERSATIONAL_MODEL}
}

function image-model {
	echo ${OLLAMA_IMAGE_MODEL}
}

function embedding-model {
	echo ${OLLAMA_EMBEDDING_MODEL}
}

function server-url {
	echo ${OLLAMA_SERVER_URL}
}

function run {
	start-ollama-container && build-models
}

# Model Building Utils

function build-models {
	# Pull the Nous-hermes-2-pro model
	pull-model ${NOUS_HERMES_2_PRO_REPO} ${NOUS_HERMES_2_FILE}

	# Create our supervisor role
	create-ollama-model ${OLLAMA_SUPERVISOR_MODEL}

	create-ollama-model ${OLLAMA_CONVERSATIONAL_MODEL}

	create-ollama-model ${OLLAMA_IMAGE_MODEL}

	create-ollama-model ${OLLAMA_EMBEDDING_MODEL}
}

function create-ollama-model {
	MODEL_NAME=$1
	MODEL_FILE_PATH=${DATA_DIR}/$1.ModelFile

	echo "Creating model ${MODEL_NAME} ..."

	# Check if the model already exists
	if exec-ollama ls | grep ${MODEL_NAME} &>/dev/null; then
		# If FORCE either not SET and SET TO false, then return
		if [ -z ${FORCE+x} ] || [ ${FORCE} == "false" ]; then
			return
		fi
	fi

	exec-ollama create ${MODEL_NAME} -f ${MODEL_FILE_PATH}
}

function exec-ollama {
	# TODO: do proper exec
	# ${CONTAINER_RUNTIME} exec ${OLLAMA_CONTAINER_NAME} ollama $@
	ollama $@
}

# TODO: document this
function pull-model {
	MODEL_REPO=$1
	MODEL_FILE=$2
	MODEL_DIR=${DATA_DIR}/${MODEL_REPO}
	MODEL_PATH=${MODEL_DIR}/${MODEL_FILE}

	echo "Pulling model: ${MODEL_PATH}"

	# Check of the model is already downloaded
	if [ -f ${MODEL_PATH} ]; then
		return
	fi

	echo "Downloading model ${MODEL_REPO} ${MODEL_FILE} ..."

	huggingface-cli download ${MODEL_REPO} ${MODEL_FILE} --local-dir ${MODEL_DIR} --local-dir-use-symlinks False
}

# Container Utils

function start-ollama-container {
	ensure-ollama-container-exists
	${CONTAINER_RUNTIME} start ${OLLAMA_CONTAINER_NAME}
}

function ensure-ollama-container-exists {
	${CONTAINER_RUNTIME} pull ollama/ollama
	create-ollama-container
}

function create-ollama-container {
	if ${CONTAINER_RUNTIME} ps -a | grep ${OLLAMA_CONTAINER_NAME} &>/dev/null; then
		return
	fi

	${CONTAINER_RUNTIME} volume create ${OLLAMA_VOLUME_NAME} || true

	${CONTAINER_RUNTIME} run \
		--name ${OLLAMA_CONTAINER_NAME} \
		--publish 11434:11434 \
		--volume ${OLLAMA_VOLUME_NAME}:/root/.ollama \
		--detach \
		ollama/ollama
}

function clean() {
	docker stop ${OLLAMA_CONTAINER_NAME} || true
	${CONTAINER_RUNTIME} rm -fv ${OLLAMA_CONTAINER_NAME} || true
	${CONTAINER_RUNTIME} volume rm -f ${OLLAMA_VOLUME_NAME} || true
}

$1
