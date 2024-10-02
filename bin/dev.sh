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
# Function to start leaky server in a new tmux session
start_leaky_server() {
    tmux new-session -d -s leaky_server 'IPFS_RPC_URL=http://localhost:5001 GET_CONTENT_FORWARDING_URL=http://localhost:3001 cargo watch -x "run --bin leaky-server"'
    echo -e "${GREEN}Leaky server started in a new tmux session.${NC}"
    echo "To attach to the session, use: tmux attach-session -t leaky_server"
}
# Function to stop leaky server tmux session
stop_leaky_server() {
    if tmux has-session -t leaky_server 2>/dev/null; then
        # Send SIGTERM to the process running in the tmux session
        tmux send-keys -t leaky_server C-c

        # Wait for a moment to allow the process to gracefully shutdown
        sleep 2

        # Check if the process is still running
        if tmux list-panes -t leaky_server -F '#{pane_pid}' | xargs ps -p >/dev/null 2>&1; then
            echo -e "${YELLOW}Process didn't stop gracefully. Forcing termination...${NC}"
            # Force kill the process
            tmux list-panes -t leaky_server -F '#{pane_pid}' | xargs kill -9
        fi

        # Kill the tmux session
        tmux kill-session -t leaky_server
        echo -e "${GREEN}Leaky server stopped and tmux session killed.${NC}"
    else
        echo -e "${RED}Leaky server tmux session not found.${NC}"
    fi
}
# Check if Docker is running
if ! docker info >/dev/null 2>&1; then
    echo -e "${RED}Error: Docker is not running.${NC}"
    exit 1
fi
# Main script logic
case ${1:-} in
    up)
        # Ensure data directories exist
        mkdir -p ./data/test
        echo -e "${GREEN}Starting services...${NC}"
        docker-compose up -d --build --remove-orphans
        start_leaky_server
        ;;
    down)
        echo -e "${GREEN}Stopping services...${NC}"
        docker-compose down
        stop_leaky_server
        ;;
    restart)
        echo -e "${GREEN}Restarting services...${NC}"
        docker-compose down
        stop_leaky_server
        docker-compose up -d --build
        start_leaky_server
        ;;
    reset)
        ./bin/dev.sh down
        docker volume rm leaky_ipfs_data || true
        rm -rf ./data/*db
        rm -rf ./data/*db*
        rm -rf ./data/test
        mkdir -p ./data/pems
        cp -r ./example ./data/test
        ./bin/dev.sh up
        sleep 3
        cd ./data/test
        cargo run --bin leaky-cli -- init --remote http://localhost:3001 --key-path ../pems \
            && cargo run --bin leaky-cli -- add \
            && cargo run --bin leaky-cli -- tag --path /writing/by_the_ocean.md --value '{"title": "by the ocean", "description": "i want to go back" }' \
            && cargo run --bin leaky-cli -- tag --path /writing/backpack-life.md --value '{"title": "backpack lyfe", "description": "oof" }' --backdate 2024-1-1 \
            && cargo run --bin leaky-cli -- push
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
            echo "Available services: ipfs, nginx"
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