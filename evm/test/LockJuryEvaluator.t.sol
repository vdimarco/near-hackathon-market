// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {ERC8183} from "erc-8183/ERC8183.sol";
import {MockUSDC} from "erc-8183/mocks/MockUSDC.sol";
import {LockJuryEvaluator} from "../src/LockJuryEvaluator.sol";

/// Tests of the adapter against the ERC-8183 reference contract.
/// A local test key signs the rulings here. The recorded check in
/// `RecordedRuling.t.sol` uses a signature from the NEAR testnet signer.
contract LockJuryEvaluatorTest is Test {
    uint256 constant BUDGET = 5_000_000; // 5 USDC, 6 decimals
    uint8 constant COMPLETE = 1;
    uint8 constant REJECT = 2;

    ERC8183 escrow;
    MockUSDC usdc;
    LockJuryEvaluator adapter;

    address admin = makeAddr("admin");
    address treasury = makeAddr("treasury");
    address client = makeAddr("client");
    address provider = makeAddr("provider");
    uint256 signerKey = 0xA11CE;
    uint256 otherKey = 0xB0B;
    address nearSigner;

    function setUp() public {
        escrow = _newEscrow();
        usdc = new MockUSDC();
        vm.prank(admin);
        escrow.setPaymentTokenAllowed(address(usdc), true);
        nearSigner = vm.addr(signerKey);
        adapter = new LockJuryEvaluator(escrow, nearSigner);
        usdc.mint(client, 100 * BUDGET);
    }

    function _newEscrow() internal returns (ERC8183) {
        ERC8183 implementation = new ERC8183();
        bytes memory init = abi.encodeCall(ERC8183.initialize, (treasury, admin));
        return ERC8183(address(new ERC1967Proxy(address(implementation), init)));
    }

    /// A job in the Submitted state, with `evaluator` as its evaluator.
    function _submittedJob(address evaluator, string memory description) internal returns (uint256 jobId) {
        vm.prank(client);
        jobId = escrow.createJob(provider, evaluator, uint48(block.timestamp + 1 days), description, address(0), 0);
        vm.prank(provider);
        escrow.setBudget(jobId, address(usdc), BUDGET, "");
        vm.startPrank(client);
        usdc.approve(address(escrow), BUDGET);
        escrow.fund(jobId, address(usdc), BUDGET, "");
        vm.stopPrank();
        vm.prank(provider);
        escrow.submit(jobId, keccak256("deliverable"), "");
    }

    function _sign(uint256 key, uint256 jobId, uint8 verdict, bytes32 reason)
        internal
        view
        returns (uint8 v, bytes32 r, bytes32 s)
    {
        (v, r, s) = vm.sign(key, adapter.rulingDigest(jobId, verdict, reason));
    }

    function test_valid_complete_ruling_pays_the_provider() public {
        uint256 jobId = _submittedJob(address(adapter), "Write slugify(title).");
        bytes32 reason = keccak256("case 1");
        (uint8 v, bytes32 r, bytes32 s) = _sign(signerKey, jobId, COMPLETE, reason);

        adapter.settle(jobId, COMPLETE, reason, v, r, s);

        assertEq(usdc.balanceOf(provider), BUDGET, "provider balance");
        assertEq(uint8(escrow.getJob(jobId).status), uint8(ERC8183.JobStatus.Completed), "job status");
    }

    function test_valid_reject_ruling_refunds_the_client() public {
        uint256 jobId = _submittedJob(address(adapter), "Write slugify(title).");
        uint256 clientBefore = usdc.balanceOf(client);
        bytes32 reason = keccak256("case 2");
        (uint8 v, bytes32 r, bytes32 s) = _sign(signerKey, jobId, REJECT, reason);

        adapter.settle(jobId, REJECT, reason, v, r, s);

        assertEq(usdc.balanceOf(client), clientBefore + BUDGET, "client balance");
        assertEq(usdc.balanceOf(provider), 0, "provider balance");
        assertEq(uint8(escrow.getJob(jobId).status), uint8(ERC8183.JobStatus.Rejected), "job status");
    }

    function test_ruling_signed_by_another_key_is_refused() public {
        uint256 jobId = _submittedJob(address(adapter), "Write slugify(title).");
        bytes32 reason = keccak256("case 3");
        (uint8 v, bytes32 r, bytes32 s) = _sign(otherKey, jobId, COMPLETE, reason);

        vm.expectPartialRevert(LockJuryEvaluator.WrongSigner.selector);
        adapter.settle(jobId, COMPLETE, reason, v, r, s);

        assertEq(usdc.balanceOf(provider), 0, "provider balance");
    }

    function test_signature_for_reject_cannot_complete_the_job() public {
        uint256 jobId = _submittedJob(address(adapter), "Write slugify(title).");
        bytes32 reason = keccak256("case 4");
        (uint8 v, bytes32 r, bytes32 s) = _sign(signerKey, jobId, REJECT, reason);

        vm.expectPartialRevert(LockJuryEvaluator.WrongSigner.selector);
        adapter.settle(jobId, COMPLETE, reason, v, r, s);
    }

    function test_signature_for_one_job_cannot_settle_another_job() public {
        uint256 first = _submittedJob(address(adapter), "Write slugify(title).");
        uint256 second = _submittedJob(address(adapter), "Write slugify(title).");
        bytes32 reason = keccak256("case 5");
        (uint8 v, bytes32 r, bytes32 s) = _sign(signerKey, first, COMPLETE, reason);

        vm.expectPartialRevert(LockJuryEvaluator.WrongSigner.selector);
        adapter.settle(second, COMPLETE, reason, v, r, s);
    }

    function test_unknown_verdict_is_refused() public {
        uint256 jobId = _submittedJob(address(adapter), "Write slugify(title).");
        bytes32 reason = keccak256("case 6");
        (uint8 v, bytes32 r, bytes32 s) = _sign(signerKey, jobId, 3, reason);

        vm.expectRevert(abi.encodeWithSelector(LockJuryEvaluator.UnknownVerdict.selector, uint8(3)));
        adapter.settle(jobId, 3, reason, v, r, s);
    }

    /// The same vector is in `scripts/ruling_digest.py` and in `evaluator/src/lib.rs`.
    function test_digest_matches_the_shared_test_vector() public view {
        bytes32 digest = adapter.digestOf(
            5042002,
            0x1111111111111111111111111111111111111111,
            0x0747EEf0706327138c69792bF28Cd525089e4583,
            7,
            keccak256("Write slugify(title)."),
            COMPLETE,
            0x2222222222222222222222222222222222222222222222222222222222222222
        );
        assertEq(digest, 0x5737b69b4a81013128480187cd99686dc83320a88040ba11cdd3ea0389392f41);
    }

    function test_ruling_digest_uses_the_description_that_the_escrow_stores() public {
        string memory description = "Write slugify(title).";
        uint256 jobId = _submittedJob(address(adapter), description);
        bytes32 reason = keccak256("case 7");

        bytes32 expected = adapter.digestOf(
            block.chainid, address(adapter), address(escrow), jobId, keccak256(bytes(description)), COMPLETE, reason
        );

        bytes32 forOtherText = adapter.digestOf(
            block.chainid, address(adapter), address(escrow), jobId, keccak256("Another description."), COMPLETE, reason
        );

        assertEq(adapter.rulingDigest(jobId, COMPLETE, reason), expected);
        assertNotEq(adapter.rulingDigest(jobId, COMPLETE, reason), forOtherText);
    }

    function test_signature_made_for_another_chain_is_refused() public {
        uint256 jobId = _submittedJob(address(adapter), "Write slugify(title).");
        bytes32 reason = keccak256("case 8");
        (uint8 v, bytes32 r, bytes32 s) = _sign(signerKey, jobId, COMPLETE, reason);

        vm.chainId(block.chainid + 1);

        vm.expectPartialRevert(LockJuryEvaluator.WrongSigner.selector);
        adapter.settle(jobId, COMPLETE, reason, v, r, s);
    }

    function test_signature_made_for_another_adapter_is_refused() public {
        LockJuryEvaluator other = new LockJuryEvaluator(escrow, nearSigner);
        uint256 jobId = _submittedJob(address(adapter), "Write slugify(title).");
        bytes32 reason = keccak256("case 9");
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerKey, other.rulingDigest(jobId, COMPLETE, reason));

        vm.expectPartialRevert(LockJuryEvaluator.WrongSigner.selector);
        adapter.settle(jobId, COMPLETE, reason, v, r, s);
    }

    /// The escrow gives this refusal: a settled job is not in the Submitted state.
    function test_second_settle_call_is_refused() public {
        uint256 jobId = _submittedJob(address(adapter), "Write slugify(title).");
        bytes32 reason = keccak256("case 10");
        (uint8 v, bytes32 r, bytes32 s) = _sign(signerKey, jobId, COMPLETE, reason);
        adapter.settle(jobId, COMPLETE, reason, v, r, s);

        vm.expectRevert(ERC8183.WrongStatus.selector);
        adapter.settle(jobId, COMPLETE, reason, v, r, s);

        assertEq(usdc.balanceOf(provider), BUDGET, "provider is paid one time");
    }
}
