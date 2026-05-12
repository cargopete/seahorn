// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {OwnableUpgradeable} from "@openzeppelin/contracts-upgradeable/access/OwnableUpgradeable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";

import {DataService} from "@graphprotocol/horizon/data-service/DataService.sol";
import {DataServiceFees} from "@graphprotocol/horizon/data-service/extensions/DataServiceFees.sol";
import {
    DataServicePausableUpgradeable
} from "@graphprotocol/horizon/data-service/extensions/DataServicePausableUpgradeable.sol";
import {IGraphPayments} from "@graphprotocol/interfaces/contracts/horizon/IGraphPayments.sol";
import {IGraphTallyCollector} from "@graphprotocol/interfaces/contracts/horizon/IGraphTallyCollector.sol";
import {IHorizonStaking} from "@graphprotocol/interfaces/contracts/horizon/IHorizonStaking.sol";

import {ISolanaDataService} from "./interfaces/ISolanaDataService.sol";

/// @title SolanaDataService
/// @notice Solana data indexing service built on The Graph Protocol's Horizon framework.
///
/// Providers (indexers) stake GRT via HorizonStaking provisions, register here,
/// then call startService for each Solana program ID they wish to index.
/// Gateways (consumers) pay per-query via GraphTally (TAP v2); providers collect
/// fees by submitting signed RAVs to collect().
///
/// @dev Inherits DataService (provision utilities, GraphDirectory), DataServiceFees
///      (stake-backed fee locking), DataServicePausableUpgradeable (emergency stop).
///      Deployed as a UUPS upgradeable proxy on Arbitrum One.
contract SolanaDataService is
    OwnableUpgradeable,
    UUPSUpgradeable,
    DataService,
    DataServiceFees,
    DataServicePausableUpgradeable,
    ISolanaDataService
{
    // -------------------------------------------------------------------------
    // Constants
    // -------------------------------------------------------------------------

    /// @notice Default minimum GRT provision per registered provider.
    uint256 public constant DEFAULT_MIN_PROVISION = 555e18;

    /// @notice Fraction of collected fees burned by the data service, in PPM (1% = 10_000).
    uint256 public constant BURN_CUT_PPM = 10_000;

    /// @notice Fraction of collected fees retained by the data service as revenue, in PPM.
    uint256 public constant DATA_SERVICE_CUT_PPM = 10_000;

    /// @notice Absolute lower bound on the thawing period.
    uint64 public constant MIN_THAWING_PERIOD = 14 days;

    /// @notice Stake locked per GRT of fees collected. Matches SubgraphService.
    uint256 public constant STAKE_TO_FEES_RATIO = 5;

    // -------------------------------------------------------------------------
    // Storage
    // -------------------------------------------------------------------------

    /// @notice Governance-controlled set of supported Solana program IDs.
    /// @dev Key is keccak256(bytes(programId)) — strings are not valid mapping keys.
    mapping(bytes32 => bool) internal _supportedPrograms;

    /// @notice Whether a provider has registered with this service.
    mapping(address => bool) public registeredProviders;

    /// @notice Address that receives collected GRT for each provider.
    mapping(address => address) public paymentsDestination;

    /// @notice Program registrations per provider (active and historical).
    mapping(address => ProgramRegistration[]) internal _providerPrograms;

    /// @notice GraphTallyCollector used to redeem TAP receipts on-chain.
    IGraphTallyCollector private immutable GRAPH_TALLY_COLLECTOR;

    /// @notice Governance-adjustable thawing period (lower-bounded by MIN_THAWING_PERIOD).
    uint64 public minThawingPeriod;

    /// @dev Reserved storage slots for future upgrades.
    uint256[50] private __gap;

    // -------------------------------------------------------------------------
    // Constructor
    // -------------------------------------------------------------------------

    constructor(address controller, address graphTallyCollector) DataService(controller) {
        GRAPH_TALLY_COLLECTOR = IGraphTallyCollector(graphTallyCollector);
        _disableInitializers();
    }

    // -------------------------------------------------------------------------
    // Initializer
    // -------------------------------------------------------------------------

    function initialize(address owner_, address pauseGuardian) external initializer {
        __Ownable_init(owner_);
        __DataService_init();
        __DataServicePausable_init();

        minThawingPeriod = MIN_THAWING_PERIOD;
        _setProvisionTokensRange(DEFAULT_MIN_PROVISION, type(uint256).max);
        _setThawingPeriodRange(MIN_THAWING_PERIOD, type(uint64).max);
        _setVerifierCutRange(0, uint32(1_000_000));
        _setPauseGuardian(pauseGuardian, true);
    }

    // -------------------------------------------------------------------------
    // UUPS
    // -------------------------------------------------------------------------

    function _authorizeUpgrade(address) internal override onlyOwner {}

    // -------------------------------------------------------------------------
    // Governance
    // -------------------------------------------------------------------------

    /// @inheritdoc ISolanaDataService
    function addProgram(string calldata programId) external onlyOwner {
        _supportedPrograms[keccak256(bytes(programId))] = true;
        emit ProgramAdded(programId);
    }

    /// @inheritdoc ISolanaDataService
    function removeProgram(string calldata programId) external onlyOwner {
        _supportedPrograms[keccak256(bytes(programId))] = false;
        emit ProgramRemoved(programId);
    }

    /// @inheritdoc ISolanaDataService
    function setMinThawingPeriod(uint64 period) external onlyOwner {
        if (period < MIN_THAWING_PERIOD) revert ThawingPeriodTooShort(MIN_THAWING_PERIOD, period);
        minThawingPeriod = period;
        emit MinThawingPeriodSet(period);
    }

    // -------------------------------------------------------------------------
    // IDataService — lifecycle
    // -------------------------------------------------------------------------

    /// @notice Register as a Solana data provider.
    /// @param data ABI-encoded (string endpoint, string geoHash, address paymentsDestination).
    function register(address serviceProvider, bytes calldata data) external override whenNotPaused {
        _requireAuthorizedForProvision(serviceProvider);
        if (registeredProviders[serviceProvider]) {
            revert ProviderAlreadyRegistered(serviceProvider);
        }

        _checkProvisionTokens(serviceProvider);
        _checkProvisionParameters(serviceProvider, false);

        (string memory endpoint, string memory geoHash, address dest) = abi.decode(data, (string, string, address));
        registeredProviders[serviceProvider] = true;
        paymentsDestination[serviceProvider] = dest == address(0) ? serviceProvider : dest;

        emit ProviderRegistered(serviceProvider, endpoint, geoHash);
    }

    /// @notice Deregister as a Solana data provider.
    /// @dev All program registrations must be stopped first.
    function deregister(address serviceProvider, bytes calldata) external {
        _requireAuthorizedForProvision(serviceProvider);
        if (!registeredProviders[serviceProvider]) revert ProviderNotRegistered(serviceProvider);
        if (activeRegistrationCount(serviceProvider) > 0) revert ActiveRegistrationsExist(serviceProvider);

        registeredProviders[serviceProvider] = false;
        emit ProviderDeregistered(serviceProvider);
    }

    /// @inheritdoc ISolanaDataService
    function setPaymentsDestination(address destination) external {
        if (!registeredProviders[msg.sender]) revert ProviderNotRegistered(msg.sender);
        address dest = destination == address(0) ? msg.sender : destination;
        paymentsDestination[msg.sender] = dest;
        emit PaymentsDestinationSet(msg.sender, dest);
    }

    /// @notice Activate indexing service for a specific Solana program.
    /// @param data ABI-encoded (string programId, string endpoint).
    function startService(address serviceProvider, bytes calldata data) external override whenNotPaused {
        _requireAuthorizedForProvision(serviceProvider);
        if (!registeredProviders[serviceProvider]) revert ProviderNotRegistered(serviceProvider);

        (string memory programId, string memory endpoint) = abi.decode(data, (string, string));

        if (!_supportedPrograms[keccak256(bytes(programId))]) {
            revert ProgramNotSupported(programId);
        }

        IHorizonStaking.Provision memory provision = _graphStaking().getProvision(serviceProvider, address(this));
        if (provision.tokens < DEFAULT_MIN_PROVISION) {
            revert InsufficientProvision(DEFAULT_MIN_PROVISION, provision.tokens);
        }

        // Reactivate an existing stopped entry rather than growing the array unboundedly.
        ProgramRegistration[] storage regs = _providerPrograms[serviceProvider];
        for (uint256 i = 0; i < regs.length; i++) {
            if (keccak256(bytes(regs[i].programId)) == keccak256(bytes(programId))) {
                regs[i].active = true;
                regs[i].endpoint = endpoint;
                emit ServiceStarted(serviceProvider, programId, endpoint);
                return;
            }
        }

        regs.push(ProgramRegistration({programId: programId, endpoint: endpoint, active: true}));
        emit ServiceStarted(serviceProvider, programId, endpoint);
    }

    /// @notice Deactivate indexing service for a specific Solana program.
    /// @param data ABI-encoded (string programId).
    function stopService(address serviceProvider, bytes calldata data) external override {
        _requireAuthorizedForProvision(serviceProvider);
        string memory programId = abi.decode(data, (string));

        ProgramRegistration[] storage regs = _providerPrograms[serviceProvider];
        for (uint256 i = 0; i < regs.length; i++) {
            if (keccak256(bytes(regs[i].programId)) == keccak256(bytes(programId)) && regs[i].active) {
                regs[i].active = false;
                emit ServiceStopped(serviceProvider, programId);
                return;
            }
        }
        revert RegistrationNotFound(serviceProvider, programId);
    }

    /// @notice Collect fees by submitting a signed Receipt Aggregate Voucher (RAV).
    ///
    /// Flow: SolanaDataService.collect() → GraphTallyCollector.collect()
    ///   → PaymentsEscrow.collect() → GraphPayments.collect()
    ///   → distributes: protocol tax → data service cut → delegator cut → provider
    ///
    /// @param data ABI-encoded (SignedRAV, tokensToCollect).
    function collect(address serviceProvider, IGraphPayments.PaymentTypes paymentType, bytes calldata data)
        external
        override
        whenNotPaused
        returns (uint256 fees)
    {
        if (paymentType != IGraphPayments.PaymentTypes.QueryFee) revert InvalidPaymentType();
        if (!registeredProviders[serviceProvider]) revert ProviderNotRegistered(serviceProvider);

        (IGraphTallyCollector.SignedRAV memory signedRav, uint256 tokensToCollect) =
            abi.decode(data, (IGraphTallyCollector.SignedRAV, uint256));

        if (signedRav.rav.serviceProvider != serviceProvider) {
            revert InvalidServiceProvider(serviceProvider, signedRav.rav.serviceProvider);
        }

        _releaseStake(serviceProvider, 0);

        uint256 balanceBefore = _graphToken().balanceOf(address(this));
        fees = GRAPH_TALLY_COLLECTOR.collect(
            paymentType,
            abi.encode(signedRav, BURN_CUT_PPM + DATA_SERVICE_CUT_PPM, paymentsDestination[serviceProvider]),
            tokensToCollect
        );

        uint256 received = _graphToken().balanceOf(address(this)) - balanceBefore;
        if (received > 0) {
            uint256 burned = received * BURN_CUT_PPM / (BURN_CUT_PPM + DATA_SERVICE_CUT_PPM);
            _graphToken().burn(burned);
            emit FeesBurned(serviceProvider, burned);
        }

        if (fees > 0) {
            _lockStake(serviceProvider, fees * STAKE_TO_FEES_RATIO, block.timestamp + minThawingPeriod);
        }
    }

    /// @notice Slashing is not supported.
    function slash(address, bytes calldata) external pure override {
        revert("slashing not supported");
    }

    /// @notice Accept pending changes to this provider's provision parameters.
    function acceptProvisionPendingParameters(address serviceProvider, bytes calldata) external override {
        _requireAuthorizedForProvision(serviceProvider);
        _acceptProvisionParameters(serviceProvider);
    }

    // -------------------------------------------------------------------------
    // Views
    // -------------------------------------------------------------------------

    /// @inheritdoc ISolanaDataService
    function isRegistered(address provider) external view override returns (bool) {
        return registeredProviders[provider];
    }

    /// @inheritdoc ISolanaDataService
    function getProgramRegistrations(address provider)
        external
        view
        override
        returns (ProgramRegistration[] memory)
    {
        return _providerPrograms[provider];
    }

    /// @inheritdoc ISolanaDataService
    function isProgramSupported(string calldata programId) external view override returns (bool) {
        return _supportedPrograms[keccak256(bytes(programId))];
    }

    /// @inheritdoc ISolanaDataService
    function activeRegistrationCount(address provider) public view override returns (uint256 count) {
        ProgramRegistration[] storage regs = _providerPrograms[provider];
        for (uint256 i = 0; i < regs.length; i++) {
            if (regs[i].active) count++;
        }
    }

    function setPauseGuardian(address guardian, bool allowed) external onlyOwner {
        _setPauseGuardian(guardian, allowed);
    }

    function withdrawFees(address to, uint256 amount) external onlyOwner {
        require(to != address(0), "zero address");
        _graphToken().transfer(to, amount);
        emit FeesWithdrawn(to, amount);
    }
}
