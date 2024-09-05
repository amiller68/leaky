#!/usr/bin/env bash

set -euo pipefail

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Function to print usage
print_usage() {
	echo "Usage: $0 [command]"
	echo "Commands:"
	echo "  up        Start all services"
	echo "  down      Stop all services"
	echo "  restart   Restart all services"
	echo "  logs      View logs of all services"
	echo "  ps        List running services"
	echo "  shell     Open a shell in a service container"
}

# Check if Docker is running
if ! docker info >/dev/null 2>&1; then
	echo -e "${RED}Error: Docker is not running.${NC}"
	exit 1
fi

# Main script logic
case ${1:-} in
up)
	echo -e "${GREEN}Starting services...${NC}"
	docker-compose up -d --build
	;;
down)
	echo -e "${GREEN}Stopping services...${NC}"
	docker-compose down
	;;
restart)
	echo -e "${GREEN}Restarting services...${NC}"
	docker-compose down
	docker-compose up -d --build
	;;
logs)
	echo -e "${GREEN}Viewing logs...${NC}"
	docker-compose logs -f
	;;
ps)
	echo -e "${GREEN}Listing running services...${NC}"
	docker-compose ps
	;;
shell)
	if [ -z ${2:-} ]; then
		echo -e "${RED}Error: Please specify a service name.${NC}"
		echo "Available services: ipfs, leaky, nginx"
		exit 1
	fi
	echo -e "${GREEN}Opening shell in $2 service...${NC}"
	docker-compose exec $2 /bin/sh
	;;
*)
	print_usage
	exit 1
	;;
esac
