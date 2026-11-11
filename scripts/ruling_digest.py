"""Compute the digest of a jury ruling for an ERC-8183 job on an EVM chain.

Three programs compute this digest and share one test vector: this script,
the NEAR contract in `evaluator/`, and the Solidity adapter in `evm/`.

The digest is Keccak-256 of eight 32-byte words, in this order: the hash of
the type text, the chain id, the adapter address, the escrow address, the job
id, the Keccak-256 hash of the job description, the verdict, and the reason.
The verdict is 1 for "pay the provider" and 2 for "refund the client".

Python has no Keccak-256 in its standard library. `hashlib.sha3_256` is a
different function. This file has its own Keccak-256.
"""
import hashlib
import sys

TYPE_TEXT = (
    b"LockJuryRuling(uint256 chainId,address adapter,address escrow,uint256 jobId,"
    b"bytes32 descriptionHash,uint8 verdict,bytes32 reason)"
)
COMPLETE = 1
REJECT = 2
# Order of the secp256k1 group.
N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141

_MASK = (1 << 64) - 1
_ROUND_CONSTANTS = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808A, 0x8000000080008000,
    0x000000000000808B, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
    0x000000000000008A, 0x0000000000000088, 0x0000000080008009, 0x000000008000000A,
    0x000000008000808B, 0x800000000000008B, 0x8000000000008089, 0x8000000000008003,
    0x8000000000008002, 0x8000000000000080, 0x000000000000800A, 0x800000008000000A,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
]
_ROTATIONS = [
    [0, 36, 3, 41, 18],
    [1, 44, 10, 45, 2],
    [62, 6, 43, 15, 61],
    [28, 55, 25, 21, 56],
    [27, 20, 39, 8, 14],
]


def _rol(value, count):
    return ((value << count) | (value >> (64 - count))) & _MASK if count else value


def _permute(a):
    for constant in _ROUND_CONSTANTS:
        c = [a[x][0] ^ a[x][1] ^ a[x][2] ^ a[x][3] ^ a[x][4] for x in range(5)]
        d = [c[(x - 1) % 5] ^ _rol(c[(x + 1) % 5], 1) for x in range(5)]
        a = [[a[x][y] ^ d[x] for y in range(5)] for x in range(5)]
        b = [[0] * 5 for _ in range(5)]
        for x in range(5):
            for y in range(5):
                b[y][(2 * x + 3 * y) % 5] = _rol(a[x][y], _ROTATIONS[x][y])
        a = [[b[x][y] ^ (~b[(x + 1) % 5][y] & _MASK & b[(x + 2) % 5][y]) for y in range(5)] for x in range(5)]
        a[0][0] ^= constant
    return a


def keccak256(data):
    """Keccak-256 with the original padding, as Ethereum uses it."""
    rate = 136
    padded = bytearray(data)
    padded.append(0x01)
    while len(padded) % rate:
        padded.append(0)
    padded[-1] |= 0x80
    state = [[0] * 5 for _ in range(5)]
    for offset in range(0, len(padded), rate):
        block = padded[offset:offset + rate]
        for i in range(rate // 8):
            state[i % 5][i // 5] ^= int.from_bytes(block[8 * i:8 * i + 8], "little")
        state = _permute(state)
    return b"".join(state[i % 5][i // 5].to_bytes(8, "little") for i in range(4))


def _word(number):
    return number.to_bytes(32, "big")


def _address(text):
    raw = bytes.fromhex(text[2:])
    if not text.startswith("0x") or len(raw) != 20:
        raise ValueError("an address is 0x and 40 hex characters")
    return bytes(12) + raw


def _hash32(text):
    raw = bytes.fromhex(text)
    if len(raw) != 32:
        raise ValueError("a hash is 64 hex characters")
    return raw


def description_hash(description):
    """Keccak-256 of the job description text, in hex."""
    return keccak256(description.encode()).hex()


def ruling_digest(chain_id, adapter, escrow, job_id, description_hash, verdict, reason):
    """The 32-byte digest that the NEAR signer signs, in hex."""
    if verdict not in (COMPLETE, REJECT):
        raise ValueError("the verdict is 1 or 2")
    data = keccak256(TYPE_TEXT) + _word(chain_id) + _address(adapter) + _address(escrow)
    data += _word(job_id) + _hash32(description_hash) + _word(verdict) + _hash32(reason)
    return keccak256(data).hex()


def case_key(chain_id, escrow, job_id, adapter, description_hash):
    """The jury case key for an ERC-8183 job. All hex is lower case."""
    _address(escrow), _address(adapter), _hash32(description_hash)
    return f"eip155:{chain_id}:{escrow.lower()}:{job_id}:{adapter.lower()}:{description_hash.lower()}"


def reason_hash(jury_account, case_id, evidence_hash, policy_hash, ruling):
    """SHA-256 of one line of text that names the jury case and its ruling, in hex."""
    line = f"lock-jury-ruling/v1|{jury_account}|{case_id}|{evidence_hash}|{policy_hash}|{ruling}"
    return hashlib.sha256(line.encode()).hexdigest()


def eth_signature(big_r, s, recovery_id):
    """Convert the answer of the NEAR signer to the Ethereum form (v, r, s).

    `big_r` is a compressed point: one prefix byte and the 32-byte x value.
    Ethereum libraries refuse an `s` in the upper half of the group order, so
    this function moves such an `s` to the lower half and changes `v` with it.
    """
    point = bytes.fromhex(big_r)
    if len(point) != 33 or point[0] not in (2, 3):
        raise ValueError("big_r is a compressed point of 33 bytes")
    s_number = int(s, 16)
    v = 27 + recovery_id
    if s_number > N // 2:
        s_number = N - s_number
        v = 55 - v
    return v, point[1:].hex(), f"{s_number:064x}"


KECCAK_VECTORS = {
    b"": "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
    b"abc": "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45",
}
TEST_VECTOR = dict(
    chain_id=5042002,
    adapter="0x1111111111111111111111111111111111111111",
    escrow="0x0747eef0706327138c69792bf28cd525089e4583",
    job_id=7,
    description_hash=description_hash("Write slugify(title)."),
    verdict=COMPLETE,
    reason="22" * 32,
)
TEST_VECTOR_DIGEST = "5737b69b4a81013128480187cd99686dc83320a88040ba11cdd3ea0389392f41"

if __name__ == "__main__":
    for message, expected in KECCAK_VECTORS.items():
        if keccak256(message).hex() != expected:
            sys.exit("Keccak-256 is wrong for a published test vector")
    got = ruling_digest(**TEST_VECTOR)
    print(got)
    if got != TEST_VECTOR_DIGEST:
        sys.exit("test vector mismatch: this script, the contract, and the adapter disagree")
    print("test vector ok: this script, the contract, and the adapter agree")
