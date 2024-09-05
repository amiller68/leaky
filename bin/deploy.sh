#!/bin/bash

# Initialize dirty flag
dirty=false

# Check for --dirty flag or dirty=true argument
for arg in "$@"
do
    case $arg in
        --dirty)
        dirty=true
        shift # Remove --dirty from processing
        ;;
        dirty=true)
        dirty=true
        shift # Remove dirty=true from processing
        ;;
    esac
done

# Fetch the latest changes from origin
git fetch origin main

# Check if we're up to date with origin/main, but only if dirty flag is not set
if [ "$dirty" = false ]; then
    if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]; then
        echo "Error: Current branch is not up-to-date with origin/main"
        exit 1
    fi
else
    echo "Warning: Skipping up-to-date check due to dirty flag"
fi

cd iac
./bin/service.sh