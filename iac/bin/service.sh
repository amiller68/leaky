#!/usr/bin/env bash

source .env
source ../.env.prod

# Start the nginx server on the host 
export ANSIBLE_HOST_KEY_CHECKING=False
ansible-playbook -i "$INVENTORY_PATH" \
  -u "root" \
  ./ansible/service.yml