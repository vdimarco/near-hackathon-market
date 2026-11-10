.PHONY: test demo demo-quick cases market-cases

# Unit tests of both contracts, and the shared test vector of the commitment hash.
test:
	cargo test -p lock-jury -p lock-escrow
	python3 scripts/commit_hash.py

# Recorded replay of all panels on a local NEAR sandbox. No API key.
demo:
	cargo run -q -p demo

# Attack tests and 3 panels. Use this for a fast check.
demo-quick:
	cargo run -q -p demo -- --limit 3

# Export the case files again from the Apart run logs.
# Usage: make cases APART=/path/to/apart-swarm-alignment
cases:
	python3 scripts/export_cases.py $(APART)

# Read public dispute records from the legacy market again. Uses the network.
market-cases:
	python3 scripts/fetch_market_cases.py
