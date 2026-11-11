.PHONY: check test demo demo-quick fresh fresh-one viewer cases market-cases evaluator-check evaluator-live sandbox-limit-check

# Every check that needs no key and no account, in one command. About 10 minutes on a first build.
check: test demo-quick evaluator-check viewer
	@echo "ALL CHECKS PASSED"


# Unit tests of both contracts, and the shared test vector of the commitment hash.
test:
	cargo test -p lock-jury -p lock-escrow
	python3 scripts/commit_hash.py

# Each target that starts the NEAR sandbox runs through this script. The sandbox
# asks for 65,535 open files. On a machine that allows fewer, the script preloads
# a small library that lowers the request. On other machines it changes nothing.
SANDBOX = sh scripts/with_sandbox.sh

# Recorded replay of all panels on a local NEAR sandbox. No API key.
demo:
	$(SANDBOX) cargo run -q -p demo

# Attack tests and 3 panels. Use this for a fast check.
demo-quick:
	$(SANDBOX) cargo run -q -p demo -- --limit 3

# Fresh run: six jurors read the ten synthetic disputes with new model calls.
# Needs the keys in .env. Writes records to runs/.
fresh:
	$(SANDBOX) cargo run -q -p demo --bin fresh

# One dispute. Use this to test the keys and the harness.
fresh-one:
	$(SANDBOX) cargo run -q -p demo --bin fresh -- --limit 1

# Test of scripts/with_sandbox.sh and scripts/nofile_clamp.c. Linux only.
sandbox-limit-check:
	sh scripts/test_nofile_clamp.sh

# Export the case files again from the Apart run logs.
# Usage: make cases APART=/path/to/apart-swarm-alignment
cases:
	python3 scripts/export_cases.py $(APART)

# Read public dispute records from the legacy market again. Uses the network.
market-cases:
	python3 scripts/fetch_market_cases.py

# Build the one-page viewer from the newest run in runs/. No network.
# Then open viewer/index.html in a browser.
viewer:
	python3 scripts/build_viewer.py

# Recorded check of the evaluator for ERC-8183 jobs. No key and no NEAR account.
# It replays two signatures of the NEAR testnet signer network against the EVM
# adapter on a local EVM. Needs Node.js: npm installs Foundry into evm/node_modules.
FORGE = node node_modules/@foundry-rs/forge/bin.mjs
evaluator-check:
	cargo test -p lock-evaluator
	cargo test -p demo --bin evaluator_testnet
	python3 scripts/ruling_digest.py
	cd evm && npm ci --no-audit --no-fund && $(FORGE) test

# Live run on NEAR testnet. The jury rules on two recorded disputes, and the
# signer network signs each ruling. Writes fixtures/evaluator/testnet-rulings.json.
# The testnet faucet funds the accounts. Uses the network. About 3 minutes.
evaluator-live:
	cargo run -q -p demo --bin evaluator_testnet
