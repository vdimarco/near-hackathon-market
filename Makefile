.PHONY: test demo demo-quick cases

# Unit tests of the contract. No network after the first build.
test:
	cargo test -p lock-jury

# Replay all recorded panels on a local NEAR sandbox. No API key.
demo:
	cargo run -q -p demo

# Replay 3 panels. Use this for a fast check.
demo-quick:
	cargo run -q -p demo -- --limit 3

# Export the case files again from the Apart run logs.
# Usage: make cases APART=/path/to/apart-swarm-alignment
cases:
	python3 scripts/export_cases.py $(APART)
