// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

/// @title ISolanaDataService
/// @notice Interface for Seahorn — a Solana data service on The Graph Protocol's Horizon framework.
///
/// Provider lifecycle:
///   register → startService (per Solana programId) → [collect]* → stopService → deregister
///
/// Provisions are managed via HorizonStaking: the provider calls
/// HorizonStaking.provision(provider, SolanaDataService, tokens, maxVerifierCut, thawingPeriod)
/// before registering here.
interface ISolanaDataService {
    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    struct ProgramRegistration {
        string programId; // base58-encoded Solana program address
        string endpoint;  // e.g. "https://seahorn.example.com/api"
        bool active;
    }

    // -------------------------------------------------------------------------
    // Events
    // -------------------------------------------------------------------------

    event ProgramAdded(string programId);
    event ProgramRemoved(string programId);
    event MinThawingPeriodSet(uint64 period);
    event ProviderRegistered(address indexed provider, string endpoint, string geoHash);
    event ProviderDeregistered(address indexed provider);
    event PaymentsDestinationSet(address indexed provider, address indexed destination);
    event ServiceStarted(address indexed provider, string programId, string endpoint);
    event ServiceStopped(address indexed provider, string programId);
    event FeesBurned(address indexed provider, uint256 amount);
    event FeesWithdrawn(address indexed to, uint256 amount);

    // -------------------------------------------------------------------------
    // Errors
    // -------------------------------------------------------------------------

    error ProgramNotSupported(string programId);
    error ProviderAlreadyRegistered(address provider);
    error ProviderNotRegistered(address provider);
    error ActiveRegistrationsExist(address provider);
    error InsufficientProvision(uint256 required, uint256 actual);
    error ThawingPeriodTooShort(uint64 required, uint64 actual);
    error RegistrationNotFound(address provider, string programId);
    error InvalidServiceProvider(address expected, address actual);
    error InvalidPaymentType();

    // -------------------------------------------------------------------------
    // Governance (owner-only)
    // -------------------------------------------------------------------------

    /// @notice Add a Solana program ID to the supported set.
    /// @param programId Base58-encoded Solana program address.
    function addProgram(string calldata programId) external;

    /// @notice Remove a program from the supported set.
    function removeProgram(string calldata programId) external;

    /// @notice Update the minimum thawing period.
    function setMinThawingPeriod(uint64 period) external;

    // -------------------------------------------------------------------------
    // Provider operations
    // -------------------------------------------------------------------------

    /// @notice Update the address that receives collected GRT fees.
    function setPaymentsDestination(address destination) external;

    // -------------------------------------------------------------------------
    // Views
    // -------------------------------------------------------------------------

    function isRegistered(address provider) external view returns (bool);

    function getProgramRegistrations(address provider) external view returns (ProgramRegistration[] memory);

    function isProgramSupported(string calldata programId) external view returns (bool);

    function activeRegistrationCount(address provider) external view returns (uint256);

    function paymentsDestination(address provider) external view returns (address);

    function minThawingPeriod() external view returns (uint64);
}
