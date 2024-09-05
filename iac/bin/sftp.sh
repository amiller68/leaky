#!/bin/bash

# Source environment variables
source .env

# File path
inventory_file="$INVENTORY_PATH"

# Read the file and extract IP and key file
if [[ -f "$inventory_file" ]]; then
    read ip key_file <<< $(awk '{print $1, $2}' "$inventory_file" | sed 's/ansible_ssh_private_key_file=//')
    
    echo "IP Address: $ip"
    echo "SSH Key File: $key_file"
else
    echo "Inventory file not found: $inventory_file"
    exit 1
fi

# Set the remote directory where the SQLite database is located
remote_data_dir="/home/service/service/data"  # Update this to the correct path

# Construct the SFTP command
sftp_command="sftp -i \"$key_file\" root@$ip:$remote_data_dir"

# Execute the SFTP command
echo "Connecting to remote server via SFTP..."
echo "Once connected, you can use SFTP commands to browse and transfer files."
echo "For example:"
echo "  ls             : List files"
echo "  get app.db     : Download the SQLite database"
echo "  exit           : Close the SFTP connection"
echo ""
echo "Executing: $sftp_command"
eval "$sftp_command"