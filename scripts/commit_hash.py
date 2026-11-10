"""Compute a juror commitment. The harness uses this function.

The contract hashes the Borsh bytes of these fields, in this order:
protocol_version u32, contract_id string, case_id u64, evidence_hash string,
policy_hash string, juror string, probs list of u32, rationale_hash string,
salt string. Borsh writes integers in little-endian order and puts a u32
length before each string and each list.
"""
import hashlib
import secrets
import struct
import sys

PROTOCOL_VERSION = 2


def _s(text):
    raw = text.encode()
    return struct.pack("<I", len(raw)) + raw


def commitment_hash(contract_id, case_id, evidence_hash, policy_hash, juror, probs, rationale_hash, salt):
    if sum(probs) != 10_000:
        raise ValueError("probabilities must sum to 10000")
    data = struct.pack("<I", PROTOCOL_VERSION) + _s(contract_id) + struct.pack("<Q", case_id)
    data += _s(evidence_hash) + _s(policy_hash) + _s(juror)
    data += struct.pack("<I", len(probs)) + b"".join(struct.pack("<I", p) for p in probs)
    data += _s(rationale_hash) + _s(salt)
    return hashlib.sha256(data).hexdigest()


def new_salt():
    """32 random bytes from the system random-number generator, in hex."""
    return secrets.token_hex(32)


TEST_VECTOR = dict(
    contract_id="jury.near",
    case_id=7,
    evidence_hash="a" * 64,
    policy_hash="b" * 64,
    juror="j1.jury.near",
    probs=[7000, 2000, 1000],
    rationale_hash="c" * 64,
    salt="00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
)
TEST_VECTOR_HASH = "49cb452949724da1a78ea133ddb650af1b8cfd4ef3db6b648b565afd0214fd87"

if __name__ == "__main__":
    got = commitment_hash(**TEST_VECTOR)
    print(got)
    if got != TEST_VECTOR_HASH:
        sys.exit("test vector mismatch: this script and the contract disagree")
    print("test vector ok: this script and the contract agree")
