// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {ERC8183} from "erc-8183/ERC8183.sol";
import {MockUSDC} from "erc-8183/mocks/MockUSDC.sol";
import {LockJuryEvaluator} from "../src/LockJuryEvaluator.sol";

/// The recorded check. It needs no key and no network.
///
/// `fixtures/evaluator/testnet-rulings.json` holds two signatures that the
/// NEAR testnet signer network made for two rulings of the jury contract.
/// This test replays them against the adapter and the ERC-8183 reference
/// contract on a local EVM. Each digest binds a chain id, an escrow address,
/// and an adapter address, so the test sets the local EVM to those values.
/// The signer address in the file comes from the `derived_public_key` view of
/// the signer contract. It does not come from the signatures.
contract RecordedRulingTest is Test {
    string constant FIXTURE = "../fixtures/evaluator/testnet-rulings.json";
    uint256 constant BUDGET = 5_000_000; // 5 USDC, 6 decimals
    uint8 constant COMPLETE = 1;
    uint8 constant REJECT = 2;

    struct Recorded {
        uint256 jobId;
        string description;
        uint8 verdict;
        bytes32 reason;
        bytes32 digest;
        uint8 v;
        bytes32 r;
        bytes32 s;
    }

    string json;
    ERC8183 escrow;
    MockUSDC usdc;
    LockJuryEvaluator adapter;
    address nearSigner;

    address admin = makeAddr("admin");
    address treasury = makeAddr("treasury");
    address client = makeAddr("client");
    address provider = makeAddr("provider");

    function setUp() public {
        json = vm.readFile(FIXTURE);
        nearSigner = vm.parseJsonAddress(json, ".signer_address");
        vm.chainId(vm.parseJsonUint(json, ".chain_id"));

        address escrowAt = vm.parseJsonAddress(json, ".escrow");
        ERC8183 implementation = new ERC8183();
        bytes memory init = abi.encodeCall(ERC8183.initialize, (treasury, admin));
        deployCodeTo("ERC1967Proxy.sol:ERC1967Proxy", abi.encode(address(implementation), init), escrowAt);
        escrow = ERC8183(escrowAt);

        address adapterAt = vm.parseJsonAddress(json, ".adapter");
        deployCodeTo("LockJuryEvaluator.sol:LockJuryEvaluator", abi.encode(escrow, nearSigner), adapterAt);
        adapter = LockJuryEvaluator(adapterAt);

        usdc = new MockUSDC();
        vm.prank(admin);
        escrow.setPaymentTokenAllowed(address(usdc), true);
        usdc.mint(client, 100 * BUDGET);

        // The jobs of the fixture, in its order. The escrow gives them the ids 1 and 2.
        for (uint256 i = 0; i < 2; i++) {
            Recorded memory ruling = _recorded(i);
            assertEq(_submittedJob(ruling.description), ruling.jobId, "job id");
        }
    }

    function _recorded(uint256 index) internal view returns (Recorded memory ruling) {
        string memory at = string.concat(".rulings[", vm.toString(index), "]");
        ruling.jobId = vm.parseJsonUint(json, string.concat(at, ".job_id"));
        ruling.description = vm.parseJsonString(json, string.concat(at, ".description"));
        ruling.verdict = uint8(vm.parseJsonUint(json, string.concat(at, ".verdict")));
        ruling.reason = vm.parseJsonBytes32(json, string.concat(at, ".reason"));
        ruling.digest = vm.parseJsonBytes32(json, string.concat(at, ".digest"));
        ruling.v = uint8(vm.parseJsonUint(json, string.concat(at, ".v")));
        ruling.r = vm.parseJsonBytes32(json, string.concat(at, ".r"));
        ruling.s = vm.parseJsonBytes32(json, string.concat(at, ".s"));
    }

    function _submittedJob(string memory description) internal returns (uint256 jobId) {
        vm.prank(client);
        jobId =
            escrow.createJob(provider, address(adapter), uint48(block.timestamp + 1 days), description, address(0), 0);
        vm.prank(provider);
        escrow.setBudget(jobId, address(usdc), BUDGET, "");
        vm.startPrank(client);
        usdc.approve(address(escrow), BUDGET);
        escrow.fund(jobId, address(usdc), BUDGET, "");
        vm.stopPrank();
        vm.prank(provider);
        escrow.submit(jobId, keccak256("deliverable"), "");
    }

    function _settle(Recorded memory ruling) internal {
        adapter.settle(ruling.jobId, ruling.verdict, ruling.reason, ruling.v, ruling.r, ruling.s);
    }

    function test_recorded_signature_for_the_worker_pays_the_provider() public {
        Recorded memory ruling = _recorded(0);
        assertEq(ruling.verdict, COMPLETE, "the first recorded ruling is for the worker");

        _settle(ruling);

        assertEq(usdc.balanceOf(provider), BUDGET, "provider balance");
        assertEq(uint8(escrow.getJob(ruling.jobId).status), uint8(ERC8183.JobStatus.Completed), "job status");
    }

    function test_recorded_signature_for_the_requester_refunds_the_client() public {
        Recorded memory ruling = _recorded(1);
        assertEq(ruling.verdict, REJECT, "the second recorded ruling is for the requester");
        uint256 clientBefore = usdc.balanceOf(client);

        _settle(ruling);

        assertEq(usdc.balanceOf(client), clientBefore + BUDGET, "client balance");
        assertEq(usdc.balanceOf(provider), 0, "provider balance");
        assertEq(uint8(escrow.getJob(ruling.jobId).status), uint8(ERC8183.JobStatus.Rejected), "job status");
    }

    function test_the_near_contract_and_the_adapter_compute_the_same_digest() public view {
        for (uint256 i = 0; i < 2; i++) {
            Recorded memory ruling = _recorded(i);
            assertEq(adapter.rulingDigest(ruling.jobId, ruling.verdict, ruling.reason), ruling.digest);
        }
    }

    function test_each_recorded_signature_recovers_to_the_derived_signer_address() public view {
        for (uint256 i = 0; i < 2; i++) {
            Recorded memory ruling = _recorded(i);
            assertEq(ecrecover(ruling.digest, ruling.v, ruling.r, ruling.s), nearSigner);
        }
    }

    function test_recorded_signature_with_a_changed_verdict_is_refused() public {
        Recorded memory ruling = _recorded(0);

        vm.expectPartialRevert(LockJuryEvaluator.WrongSigner.selector);
        adapter.settle(ruling.jobId, REJECT, ruling.reason, ruling.v, ruling.r, ruling.s);
    }

    function test_recorded_signature_for_another_job_is_refused() public {
        Recorded memory ruling = _recorded(0);
        uint256 otherJob = _recorded(1).jobId;

        vm.expectPartialRevert(LockJuryEvaluator.WrongSigner.selector);
        adapter.settle(otherJob, ruling.verdict, ruling.reason, ruling.v, ruling.r, ruling.s);
    }

    function test_adapter_with_another_signer_refuses_the_recorded_signature() public {
        Recorded memory ruling = _recorded(0);
        // Put an adapter that trusts another key at the address of the evaluator.
        deployCodeTo(
            "LockJuryEvaluator.sol:LockJuryEvaluator", abi.encode(escrow, makeAddr("another signer")), address(adapter)
        );

        vm.expectRevert(abi.encodeWithSelector(LockJuryEvaluator.WrongSigner.selector, nearSigner));
        _settle(ruling);

        assertEq(usdc.balanceOf(provider), 0, "provider balance");
    }

    function test_second_call_with_a_recorded_signature_is_refused() public {
        Recorded memory ruling = _recorded(0);
        _settle(ruling);

        vm.expectRevert(ERC8183.WrongStatus.selector);
        _settle(ruling);

        assertEq(usdc.balanceOf(provider), BUDGET, "provider is paid one time");
    }
}
