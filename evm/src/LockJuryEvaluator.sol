// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import {ERC8183} from "erc-8183/ERC8183.sol";

/// @title An ERC-8183 evaluator that settles a job on a ruling of the Lock Before You Talk jury.
/// @notice The jury rules on NEAR. A NEAR contract asks the NEAR signer network
///         (Chain Signatures) to sign a digest of the ruling. Anyone can send that
///         signature to `settle`. This contract checks the signer, and then it calls
///         `complete` or `reject` on the escrow.
/// @dev    Name this contract as the evaluator when you create the job. The escrow
///         type is the reference contract of erc-8183/base-contracts at commit 142e669.
///         This contract has no way to send out tokens. Use it only with an escrow
///         that pays no evaluator fee.
contract LockJuryEvaluator {
    /// The verdict "pay the provider".
    uint8 public constant COMPLETE = 1;
    /// The verdict "refund the client".
    uint8 public constant REJECT = 2;

    bytes32 public constant RULING_TYPEHASH = keccak256(
        "LockJuryRuling(uint256 chainId,address adapter,address escrow,uint256 jobId,bytes32 descriptionHash,uint8 verdict,bytes32 reason)"
    );

    ERC8183 public immutable escrow;
    /// The address of the key that the NEAR contract controls through Chain Signatures.
    address public immutable nearSigner;

    error WrongSigner(address recovered);
    error UnknownVerdict(uint8 verdict);

    constructor(ERC8183 escrow_, address nearSigner_) {
        escrow = escrow_;
        nearSigner = nearSigner_;
    }

    /// The digest of a ruling. `scripts/ruling_digest.py` and the NEAR contract
    /// compute the same value, and the three share one test vector.
    function digestOf(
        uint256 chainId,
        address adapter,
        address escrow_,
        uint256 jobId,
        bytes32 descriptionHash,
        uint8 verdict,
        bytes32 reason
    ) public pure returns (bytes32) {
        return keccak256(abi.encode(RULING_TYPEHASH, chainId, adapter, escrow_, jobId, descriptionHash, verdict, reason));
    }

    /// The digest that the NEAR signer must sign for this job on this chain.
    /// It uses the description that the escrow stores for the job.
    function rulingDigest(uint256 jobId, uint8 verdict, bytes32 reason) public view returns (bytes32) {
        bytes32 descriptionHash = keccak256(bytes(escrow.getJob(jobId).description));
        return digestOf(block.chainid, address(this), address(escrow), jobId, descriptionHash, verdict, reason);
    }

    /// Settle a job with a signed ruling. Anyone can call this.
    /// @param reason A hash that names the jury case. The escrow emits it.
    function settle(uint256 jobId, uint8 verdict, bytes32 reason, uint8 v, bytes32 r, bytes32 s) external {
        address recovered = ECDSA.recover(rulingDigest(jobId, verdict, reason), v, r, s);
        if (recovered != nearSigner) revert WrongSigner(recovered);
        if (verdict == COMPLETE) {
            escrow.complete(jobId, reason, "");
        } else if (verdict == REJECT) {
            escrow.reject(jobId, reason, "");
        } else {
            revert UnknownVerdict(verdict);
        }
    }
}
