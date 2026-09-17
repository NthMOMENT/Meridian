// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";

/// @title IntentManager
/// @notice Source-chain (Arbitrum Sepolia / Robinhood Chain testnet) contract for Maat.
/// Users escrow ETH here to express a cross-chain intent; solvers overcollateralize
/// and front the destination-chain payout; the off-chain orchestrator confirms
/// settlement (or slashes a non-performing solver) once a ZK proof of the
/// destination-chain event is verified off-chain.
contract IntentManager is ReentrancyGuard, Pausable {
    enum IntentStatus {
        Pending,
        Settled,
        Expired,
        Slashed
    }

    struct Intent {
        address owner;
        uint256 amount;
        address destinationWallet;
        uint64 destinationChainId;
        uint64 expiry;
        uint16 slippageBps;
        IntentStatus status;
        address solver;
        uint256 collateralPosted;
        bytes32 zkProofHash;
        uint256 createdAt;
        uint256 settledAt;
    }

    mapping(bytes32 => Intent) public intents;
    uint256 public intentNonce;

    address public immutable orchestrator;
    address public immutable treasury;
    address public immutable owner;

    uint256 public constant COLLATERAL_RATIO = 150;

    uint256 public dailyVolume;
    uint256 public dailyVolumeLimit;
    uint256 public lastVolumeReset;
    uint256 public constant VOLUME_WINDOW = 24 hours;

    event IntentCreated(
        bytes32 indexed intentId,
        address indexed sender,
        uint256 amount,
        address destinationWallet,
        uint64 destinationChainId,
        uint64 expiry,
        uint16 slippageBps
    );
    event CollateralPosted(bytes32 indexed intentId, address indexed solver, uint256 collateralAmount);
    event IntentSettled(bytes32 indexed intentId, address indexed solver, uint256 amount, bytes32 zkProofHash);
    event SolverSlashed(bytes32 indexed intentId, address indexed solver, uint256 collateralSlashed, address treasury);
    event IntentCancelled(bytes32 indexed intentId, address indexed user, uint256 amount);
    event CircuitBreakerTriggered(uint256 amount, uint256 dailyVolume, uint256 dailyVolumeLimit);
    event DailyVolumeLimitUpdated(uint256 newLimit);

    error InvalidAmount();
    error ExpiryInPast();
    error IncorrectValue();
    error DailyVolumeLimitExceeded();
    error IntentDoesNotExist();
    error SolverAlreadyPosted();
    error IntentExpired();
    error IntentNotExpired();
    error InsufficientCollateral();
    error NotOrchestrator();
    error NotAdmin();
    error NotIntentOwner();
    error IntentNotPending();
    error NoSolver();
    error EthTransferFailed();
    error ZeroAddress();

    modifier onlyOrchestrator() {
        if (msg.sender != orchestrator) revert NotOrchestrator();
        _;
    }

    modifier onlyAdmin() {
        if (msg.sender != owner) revert NotAdmin();
        _;
    }

    constructor(address _orchestrator, address _treasury, address _owner) {
        if (_orchestrator == address(0) || _treasury == address(0) || _owner == address(0)) {
            revert ZeroAddress();
        }
        orchestrator = _orchestrator;
        treasury = _treasury;
        owner = _owner;
        dailyVolumeLimit = type(uint256).max;
        lastVolumeReset = block.timestamp;
    }

    /// @notice User escrows ETH and creates a cross-chain intent.
    function submitIntent(
        uint256 amount,
        address destinationWallet,
        uint64 destinationChainId,
        uint64 expiry,
        uint16 slippageBps
    ) external payable whenNotPaused nonReentrant returns (bytes32 intentId) {
        if (amount == 0) revert InvalidAmount();
        if (expiry <= block.timestamp) revert ExpiryInPast();
        if (msg.value != amount) revert IncorrectValue();

        if (block.timestamp > lastVolumeReset + VOLUME_WINDOW) {
            dailyVolume = 0;
            lastVolumeReset = block.timestamp;
        }
        if (dailyVolume + amount > dailyVolumeLimit) {
            emit CircuitBreakerTriggered(amount, dailyVolume, dailyVolumeLimit);
            revert DailyVolumeLimitExceeded();
        }
        dailyVolume += amount;

        intentId = keccak256(abi.encodePacked(msg.sender, intentNonce, block.timestamp));
        intentNonce += 1;

        intents[intentId] = Intent({
            owner: msg.sender,
            amount: amount,
            destinationWallet: destinationWallet,
            destinationChainId: destinationChainId,
            expiry: expiry,
            slippageBps: slippageBps,
            status: IntentStatus.Pending,
            solver: address(0),
            collateralPosted: 0,
            zkProofHash: bytes32(0),
            createdAt: block.timestamp,
            settledAt: 0
        });

        emit IntentCreated(intentId, msg.sender, amount, destinationWallet, destinationChainId, expiry, slippageBps);
    }

    /// @notice Solver overcollateralizes against a pending intent.
    function postCollateral(bytes32 intentId) external payable whenNotPaused nonReentrant {
        Intent storage intent = intents[intentId];
        if (intent.owner == address(0)) revert IntentDoesNotExist();
        if (intent.solver != address(0)) revert SolverAlreadyPosted();
        if (block.timestamp >= intent.expiry) revert IntentExpired();

        uint256 required = (intent.amount * COLLATERAL_RATIO) / 100;
        if (msg.value != required) revert InsufficientCollateral();

        intent.solver = msg.sender;
        intent.collateralPosted = msg.value;

        emit CollateralPosted(intentId, msg.sender, msg.value);
    }

    /// @notice Orchestrator confirms an off-chain-verified ZK proof and releases the
    /// escrowed user funds to the solver as reimbursement/reward.
    function confirmSettlement(bytes32 intentId, bytes32 zkProofHash) external onlyOrchestrator nonReentrant {
        Intent storage intent = intents[intentId];
        if (intent.status != IntentStatus.Pending) revert IntentNotPending();
        if (block.timestamp >= intent.expiry) revert IntentExpired();
        if (intent.solver == address(0)) revert NoSolver();

        intent.status = IntentStatus.Settled;
        intent.zkProofHash = zkProofHash;
        intent.settledAt = block.timestamp;

        uint256 amount = intent.amount;
        address solver = intent.solver;

        (bool ok,) = payable(solver).call{value: amount}("");
        if (!ok) revert EthTransferFailed();

        emit IntentSettled(intentId, solver, amount, zkProofHash);
    }

    /// @notice Orchestrator slashes a solver that failed to deliver before expiry.
    function slashSolver(bytes32 intentId) external onlyOrchestrator nonReentrant {
        Intent storage intent = intents[intentId];
        if (intent.status != IntentStatus.Pending) revert IntentNotPending();
        if (block.timestamp < intent.expiry) revert IntentNotExpired();
        if (intent.solver == address(0)) revert NoSolver();

        intent.status = IntentStatus.Slashed;

        address solver = intent.solver;
        address user = intent.owner;
        uint256 amount = intent.amount;
        uint256 collateral = intent.collateralPosted;

        (bool okUser,) = payable(user).call{value: amount}("");
        if (!okUser) revert EthTransferFailed();

        (bool okTreasury,) = payable(treasury).call{value: collateral}("");
        if (!okTreasury) revert EthTransferFailed();

        emit SolverSlashed(intentId, solver, collateral, treasury);
    }

    /// @notice User reclaims escrow for an intent that expired with no solver.
    function cancelIntent(bytes32 intentId) external nonReentrant {
        Intent storage intent = intents[intentId];
        if (intent.owner != msg.sender) revert NotIntentOwner();
        if (intent.status != IntentStatus.Pending) revert IntentNotPending();
        if (block.timestamp < intent.expiry) revert IntentNotExpired();
        if (intent.solver != address(0)) revert SolverAlreadyPosted();

        intent.status = IntentStatus.Expired;

        uint256 amount = intent.amount;
        (bool ok,) = payable(msg.sender).call{value: amount}("");
        if (!ok) revert EthTransferFailed();

        emit IntentCancelled(intentId, msg.sender, amount);
    }

    // ------------------- Admin -------------------

    function pause() external onlyAdmin {
        _pause();
    }

    function unpause() external onlyAdmin {
        _unpause();
    }

    function setDailyVolumeLimit(uint256 newLimit) external onlyAdmin {
        dailyVolumeLimit = newLimit;
        emit DailyVolumeLimitUpdated(newLimit);
    }
}
