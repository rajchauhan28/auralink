#!/bin/bash
# Rebuild binary first
cargo build --bin auralink-bt

# Test 'status'
echo "Testing 'status'..."
./target/debug/auralink-bt status

# Test 'fullstatus'
echo "Testing 'fullstatus'..."
./target/debug/auralink-bt fullstatus

# Test '--help'
echo "Testing '--help'..."
./target/debug/auralink-bt --help
