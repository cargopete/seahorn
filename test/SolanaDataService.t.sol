// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

import {SolanaDataService} from "../contracts/SolanaDataService.sol";
import {ISolanaDataService} from "../contracts/interfaces/ISolanaDataService.sol";
import {IGraphPayments} from "@graphprotocol/interfaces/contracts/horizon/IGraphPayments.sol";
import {IGraphTallyCollector} from "@graphprotocol/interfaces/contracts/horizon/IGraphTallyCollector.sol";
import {IHorizonStaking} from "@graphprotocol/interfaces/contracts/horizon/IHorizonStaking.sol";
import {ControllerMock} from "@graphprotocol/horizon/mocks/ControllerMock.sol";

contract SolanaDataServiceTest is Test {
    // ---------- deployment handles ----------
    SolanaDataService impl;
    SolanaDataService ds; // proxy

    ControllerMock controller;

    // ---------- actors ----------
    address owner = makeAddr("owner");
    address pauseGuardian = makeAddr("pauseGuardian");
    address provider = makeAddr("provider");
    address graphTallyCollector = makeAddr("graphTallyCollector");

    // ---------- mocked Horizon contract addresses ----------
    address grtToken;
    address staking;
    address graphPayments;
    address paymentsEscrow;
    address epochManager;
    address rewardsManager;
    address tokenGateway;
    address proxyAdmin;

    // ---------- Solana program IDs ----------
    string constant PUMPFUN = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
    string constant RAYDIUM = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
    string constant JUPITER = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
    string constant UNKNOWN = "unknownProgram111111111111111111111111111111";

    // ---------- setUp ----------

    function setUp() public {
        grtToken = makeAddr("grtToken");
        staking = makeAddr("staking");
        graphPayments = makeAddr("graphPayments");
        paymentsEscrow = makeAddr("paymentsEscrow");
        epochManager = makeAddr("epochManager");
        rewardsManager = makeAddr("rewardsManager");
        tokenGateway = makeAddr("tokenGateway");
        proxyAdmin = makeAddr("proxyAdmin");

        controller = new ControllerMock(owner);
        controller.setContractProxy(keccak256("GraphToken"), grtToken);
        controller.setContractProxy(keccak256("Staking"), staking);
        controller.setContractProxy(keccak256("GraphPayments"), graphPayments);
        controller.setContractProxy(keccak256("PaymentsEscrow"), paymentsEscrow);
        controller.setContractProxy(keccak256("EpochManager"), epochManager);
        controller.setContractProxy(keccak256("RewardsManager"), rewardsManager);
        controller.setContractProxy(keccak256("GraphTokenGateway"), tokenGateway);
        controller.setContractProxy(keccak256("GraphProxyAdmin"), proxyAdmin);

        impl = new SolanaDataService(address(controller), graphTallyCollector);
        bytes memory initData = abi.encodeCall(SolanaDataService.initialize, (owner, pauseGuardian));
        ds = SolanaDataService(address(new ERC1967Proxy(address(impl), initData)));

        vm.startPrank(owner);
        ds.addProgram(PUMPFUN);
        ds.addProgram(RAYDIUM);
        ds.addProgram(JUPITER);
        vm.stopPrank();
    }

    // ---------- helpers ----------

    /// Mock staking.isAuthorized so `caller` is authorized for `sp`.
    function _mockAuthorized(address sp, address caller) internal {
        vm.mockCall(
            staking,
            abi.encodeWithSignature("isAuthorized(address,address,address)", sp, address(ds), caller),
            abi.encode(true)
        );
    }

    /// Mock staking.getProvision to return a valid provision with `tokens`.
    function _mockProvision(address sp, uint256 tokens) internal {
        IHorizonStaking.Provision memory p;
        p.tokens = tokens;
        p.thawingPeriod = uint64(14 days);
        p.maxVerifierCut = uint32(1_000_000);
        p.createdAt = uint64(block.timestamp); // must be non-zero or ProvisionManager reverts
        vm.mockCall(
            staking,
            abi.encodeWithSignature("getProvision(address,address)", sp, address(ds)),
            abi.encode(p)
        );
    }

    /// Register provider with mocked staking.
    function _register(address sp) internal {
        _mockAuthorized(sp, sp);
        _mockProvision(sp, 555e18);
        vm.prank(sp);
        ds.register(sp, abi.encode("https://seahorn.example.com", "u4pruydqqvs", sp));
    }

    /// Start service for `programId` on behalf of `sp`.
    function _startService(address sp, string memory programId) internal {
        _mockAuthorized(sp, sp);
        _mockProvision(sp, 555e18);
        vm.prank(sp);
        ds.startService(sp, abi.encode(programId, "https://seahorn.example.com"));
    }

    // ==========================================================================
    // Governance — addProgram / removeProgram
    // ==========================================================================

    function test_addProgram() public view {
        assertTrue(ds.isProgramSupported(PUMPFUN));
        assertTrue(ds.isProgramSupported(RAYDIUM));
        assertTrue(ds.isProgramSupported(JUPITER));
        assertFalse(ds.isProgramSupported(UNKNOWN));
    }

    function test_addProgram_emitsEvent() public {
        vm.expectEmit(false, false, false, true);
        emit ISolanaDataService.ProgramAdded(UNKNOWN);
        vm.prank(owner);
        ds.addProgram(UNKNOWN);
    }

    function test_addProgram_notOwner_reverts() public {
        vm.expectRevert();
        ds.addProgram(UNKNOWN);
    }

    function test_removeProgram() public {
        vm.prank(owner);
        ds.removeProgram(PUMPFUN);
        assertFalse(ds.isProgramSupported(PUMPFUN));
    }

    function test_removeProgram_emitsEvent() public {
        vm.expectEmit(false, false, false, true);
        emit ISolanaDataService.ProgramRemoved(PUMPFUN);
        vm.prank(owner);
        ds.removeProgram(PUMPFUN);
    }

    function test_removeProgram_notOwner_reverts() public {
        vm.expectRevert();
        ds.removeProgram(PUMPFUN);
    }

    // ==========================================================================
    // Governance — setMinThawingPeriod
    // ==========================================================================

    function test_setMinThawingPeriod() public {
        uint64 newPeriod = 30 days;
        vm.expectEmit(false, false, false, true);
        emit ISolanaDataService.MinThawingPeriodSet(newPeriod);
        vm.prank(owner);
        ds.setMinThawingPeriod(newPeriod);
        assertEq(ds.minThawingPeriod(), newPeriod);
    }

    function test_setMinThawingPeriod_tooShort_reverts() public {
        vm.prank(owner);
        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.ThawingPeriodTooShort.selector, 14 days, 1 days)
        );
        ds.setMinThawingPeriod(1 days);
    }

    // ==========================================================================
    // register
    // ==========================================================================

    function test_register() public {
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);

        vm.expectEmit(true, false, false, true);
        emit ISolanaDataService.ProviderRegistered(provider, "https://seahorn.example.com", "u4pruydqqvs");
        vm.prank(provider);
        ds.register(provider, abi.encode("https://seahorn.example.com", "u4pruydqqvs", provider));

        assertTrue(ds.isRegistered(provider));
        assertEq(ds.paymentsDestination(provider), provider);
    }

    function test_register_zeroDestination_defaultsToSelf() public {
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        ds.register(provider, abi.encode("https://seahorn.example.com", "u4pruydqqvs", address(0)));
        assertEq(ds.paymentsDestination(provider), provider);
    }

    function test_register_alreadyRegistered_reverts() public {
        _register(provider);

        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.ProviderAlreadyRegistered.selector, provider)
        );
        ds.register(provider, abi.encode("https://seahorn.example.com", "u4pruydqqvs", provider));
    }

    function test_register_insufficientProvision_reverts() public {
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 100e18); // below 555 GRT
        vm.prank(provider);
        // _checkProvisionTokens will revert with a ProvisionManager error
        vm.expectRevert();
        ds.register(provider, abi.encode("https://seahorn.example.com", "u4pruydqqvs", provider));
    }

    // ==========================================================================
    // deregister
    // ==========================================================================

    function test_deregister() public {
        _register(provider);
        _mockAuthorized(provider, provider);

        vm.expectEmit(true, false, false, false);
        emit ISolanaDataService.ProviderDeregistered(provider);
        vm.prank(provider);
        ds.deregister(provider, "");

        assertFalse(ds.isRegistered(provider));
    }

    function test_deregister_notRegistered_reverts() public {
        _mockAuthorized(provider, provider);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.ProviderNotRegistered.selector, provider)
        );
        ds.deregister(provider, "");
    }

    function test_deregister_withActiveRegistrations_reverts() public {
        _register(provider);
        _startService(provider, PUMPFUN);
        _mockAuthorized(provider, provider);

        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.ActiveRegistrationsExist.selector, provider)
        );
        ds.deregister(provider, "");
    }

    // ==========================================================================
    // startService / stopService
    // ==========================================================================

    function test_startService() public {
        _register(provider);

        vm.expectEmit(true, false, false, true);
        emit ISolanaDataService.ServiceStarted(provider, PUMPFUN, "https://seahorn.example.com");
        _startService(provider, PUMPFUN);

        ISolanaDataService.ProgramRegistration[] memory regs = ds.getProgramRegistrations(provider);
        assertEq(regs.length, 1);
        assertEq(regs[0].programId, PUMPFUN);
        assertTrue(regs[0].active);
        assertEq(ds.activeRegistrationCount(provider), 1);
    }

    function test_startService_notRegistered_reverts() public {
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.ProviderNotRegistered.selector, provider)
        );
        ds.startService(provider, abi.encode(PUMPFUN, "https://seahorn.example.com"));
    }

    function test_startService_unsupportedProgram_reverts() public {
        _register(provider);
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.ProgramNotSupported.selector, UNKNOWN)
        );
        ds.startService(provider, abi.encode(UNKNOWN, "https://seahorn.example.com"));
    }

    function test_startService_reactivatesExisting() public {
        _register(provider);
        _startService(provider, PUMPFUN);

        // Stop then re-start — should reuse the existing array slot.
        _mockAuthorized(provider, provider);
        vm.prank(provider);
        ds.stopService(provider, abi.encode(PUMPFUN));

        assertEq(ds.activeRegistrationCount(provider), 0);

        _startService(provider, PUMPFUN);

        ISolanaDataService.ProgramRegistration[] memory regs = ds.getProgramRegistrations(provider);
        assertEq(regs.length, 1); // not grown to 2
        assertTrue(regs[0].active);
    }

    function test_startService_multiplePrograms() public {
        _register(provider);
        _startService(provider, PUMPFUN);
        _startService(provider, RAYDIUM);
        _startService(provider, JUPITER);

        assertEq(ds.activeRegistrationCount(provider), 3);
    }

    function test_stopService() public {
        _register(provider);
        _startService(provider, PUMPFUN);

        _mockAuthorized(provider, provider);
        vm.expectEmit(true, false, false, true);
        emit ISolanaDataService.ServiceStopped(provider, PUMPFUN);
        vm.prank(provider);
        ds.stopService(provider, abi.encode(PUMPFUN));

        assertEq(ds.activeRegistrationCount(provider), 0);
        ISolanaDataService.ProgramRegistration[] memory regs = ds.getProgramRegistrations(provider);
        assertFalse(regs[0].active);
    }

    function test_stopService_notFound_reverts() public {
        _register(provider);
        _mockAuthorized(provider, provider);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.RegistrationNotFound.selector, provider, PUMPFUN)
        );
        ds.stopService(provider, abi.encode(PUMPFUN));
    }

    // ==========================================================================
    // setPaymentsDestination
    // ==========================================================================

    function test_setPaymentsDestination() public {
        _register(provider);
        address dest = makeAddr("treasury");

        vm.expectEmit(true, true, false, false);
        emit ISolanaDataService.PaymentsDestinationSet(provider, dest);
        vm.prank(provider);
        ds.setPaymentsDestination(dest);

        assertEq(ds.paymentsDestination(provider), dest);
    }

    function test_setPaymentsDestination_zero_defaultsToSelf() public {
        _register(provider);
        vm.prank(provider);
        ds.setPaymentsDestination(address(0));
        assertEq(ds.paymentsDestination(provider), provider);
    }

    function test_setPaymentsDestination_notRegistered_reverts() public {
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.ProviderNotRegistered.selector, provider)
        );
        ds.setPaymentsDestination(makeAddr("dest"));
    }

    // ==========================================================================
    // collect
    // ==========================================================================

    function _buildSignedRAV(address sp, uint128 valueAggregate)
        internal
        returns (IGraphTallyCollector.SignedRAV memory)
    {
        IGraphTallyCollector.ReceiptAggregateVoucher memory rav = IGraphTallyCollector.ReceiptAggregateVoucher({
            collectionId: bytes32(0),
            payer: makeAddr("payer"),
            serviceProvider: sp,
            dataService: address(ds),
            timestampNs: uint64(block.timestamp * 1e9),
            valueAggregate: valueAggregate,
            metadata: ""
        });
        return IGraphTallyCollector.SignedRAV({rav: rav, signature: new bytes(65)});
    }

    function test_collect() public {
        _register(provider);

        uint128 valueAggregate = 100e18;
        uint256 tokensToCollect = 100e18;
        uint256 fees = tokensToCollect;
        IGraphTallyCollector.SignedRAV memory signedRav = _buildSignedRAV(provider, valueAggregate);

        // balanceOf returns 0 before and after → received = 0, burn branch skipped.
        vm.mockCall(grtToken, abi.encodeWithSignature("balanceOf(address)", address(ds)), abi.encode(uint256(0)));

        // graphTallyCollector.collect — match on selector only (calldata is complex dynamic data).
        vm.mockCall(graphTallyCollector, abi.encodeWithSignature("collect(uint8,bytes,uint256)"), abi.encode(fees));

        // _lockStake → ProvisionTracker.lock → staking.getTokensAvailable
        // delegationRatio defaults to type(uint32).max in ProvisionManager
        vm.mockCall(
            staking,
            abi.encodeWithSignature(
                "getTokensAvailable(address,address,uint32)",
                provider, address(ds), type(uint32).max
            ),
            abi.encode(uint256(1_000_000e18))
        );

        bytes memory data = abi.encode(signedRav, tokensToCollect);
        uint256 returned = ds.collect(provider, IGraphPayments.PaymentTypes.QueryFee, data);
        assertEq(returned, fees);
    }

    function test_collect_invalidPaymentType_reverts() public {
        _register(provider);
        IGraphTallyCollector.SignedRAV memory signedRav = _buildSignedRAV(provider, 100e18);
        bytes memory data = abi.encode(signedRav, uint256(100e18));

        vm.expectRevert(ISolanaDataService.InvalidPaymentType.selector);
        ds.collect(provider, IGraphPayments.PaymentTypes.IndexingFee, data);
    }

    function test_collect_notRegistered_reverts() public {
        IGraphTallyCollector.SignedRAV memory signedRav = _buildSignedRAV(provider, 100e18);
        bytes memory data = abi.encode(signedRav, uint256(100e18));

        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.ProviderNotRegistered.selector, provider)
        );
        ds.collect(provider, IGraphPayments.PaymentTypes.QueryFee, data);
    }

    function test_collect_wrongServiceProvider_reverts() public {
        _register(provider);
        address other = makeAddr("other");
        IGraphTallyCollector.SignedRAV memory signedRav = _buildSignedRAV(other, 100e18); // RAV for `other`
        bytes memory data = abi.encode(signedRav, uint256(100e18));

        vm.expectRevert(
            abi.encodeWithSelector(ISolanaDataService.InvalidServiceProvider.selector, provider, other)
        );
        ds.collect(provider, IGraphPayments.PaymentTypes.QueryFee, data);
    }

    // ==========================================================================
    // slash — always reverts
    // ==========================================================================

    function test_slash_reverts() public {
        vm.expectRevert("slashing not supported");
        ds.slash(provider, "");
    }

    // ==========================================================================
    // Pause / unpause
    // ==========================================================================

    function test_pause() public {
        vm.prank(pauseGuardian);
        ds.pause();

        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        vm.expectRevert();
        ds.register(provider, abi.encode("https://seahorn.example.com", "u4pruydqqvs", provider));
    }

    function test_unpause() public {
        vm.prank(pauseGuardian);
        ds.pause();

        vm.prank(pauseGuardian);
        ds.unpause();

        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        ds.register(provider, abi.encode("https://seahorn.example.com", "u4pruydqqvs", provider));
        assertTrue(ds.isRegistered(provider));
    }

    // ==========================================================================
    // withdrawFees
    // ==========================================================================

    function test_withdrawFees() public {
        address to = makeAddr("treasury");
        uint256 amount = 1000e18;
        vm.mockCall(
            grtToken,
            abi.encodeWithSignature("transfer(address,uint256)", to, amount),
            abi.encode(true)
        );
        vm.expectEmit(false, false, false, true);
        emit ISolanaDataService.FeesWithdrawn(to, amount);
        vm.prank(owner);
        ds.withdrawFees(to, amount);
    }

    function test_withdrawFees_notOwner_reverts() public {
        vm.expectRevert();
        ds.withdrawFees(makeAddr("treasury"), 1000e18);
    }

    function test_withdrawFees_zeroAddress_reverts() public {
        vm.prank(owner);
        vm.expectRevert("zero address");
        ds.withdrawFees(address(0), 1000e18);
    }

    // ==========================================================================
    // UUPS upgradeability — only owner can upgrade
    // ==========================================================================

    function test_upgrade_notOwner_reverts() public {
        SolanaDataService newImpl = new SolanaDataService(address(controller), graphTallyCollector);
        vm.expectRevert();
        ds.upgradeToAndCall(address(newImpl), "");
    }

    function test_upgrade_owner() public {
        SolanaDataService newImpl = new SolanaDataService(address(controller), graphTallyCollector);
        vm.prank(owner);
        ds.upgradeToAndCall(address(newImpl), "");
    }
}
