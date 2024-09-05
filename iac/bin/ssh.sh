#!/bin/bash

source .env

# File path
inventory_file="$INVENTORY_PATH"
user=$1

# Read the file and extract IP and key file
if [[ -f "$inventory_file" ]]; then
    read ip key_file <<< $(awk '{print $1, $2}' "$inventory_file" | sed 's/ansible_ssh_private_key_file=//')
    
    echo "IP Address: $ip"
    echo "SSH Key File: $key_file"
else
    echo "Inventory file not found: $inventory_file"
    exit 1
fi

# Construct the SSH command
command="ssh -i \"$key_file\" root@$ip"

# If a user is specified, modify the command to switch user after connecting
if [ -n "$user" ]; then
    command="$command -t \"sudo su - $user\""
fi

# Execute the command
echo "Executing: $command"
eval "$command"