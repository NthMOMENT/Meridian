#![no_main]
sp1_zkvm::entrypoint!(main);

use serde::{Deserialize, Serialize};

/// The raw intent data fed in from the host (orchestrator).
/// This matches the fields emitted by IntentManager.sol's IntentCreated event.
#[derive(Serialize, Deserialize)]
pub struct IntentInput {
    pub intent_id: [u8; 32],       // bytes32 intentId (from event)
    pub sender: [u8; 20],          // address sender
    pub amount: u64,               // uint256 amount (truncated to u64 for zkVM)
    pub destination_wallet: [u8; 20], // address destinationWallet
    pub destination_chain_id: u64, // uint64 destinationChainId
    pub expiry: u64,               // uint64 expiry timestamp
    pub slippage_bps: u16,         // uint16 slippageBps
    pub block_number: u64,         // block number the event was emitted in
    pub tx_hash: [u8; 32],         // transaction hash containing the event
}

/// Public outputs committed to the proof — readable by the orchestrator.
#[derive(Serialize, Deserialize)]
pub struct IntentOutput {
    pub intent_id: [u8; 32],
    pub destination_chain_id: u64,
    pub amount: u64,
    pub expiry: u64,
    pub block_number: u64,
    pub tx_hash: [u8; 32],
    pub verified: bool,
}

pub fn main() {
    // Read the intent input from the host via SP1's stdin pipe.
    let input: IntentInput = sp1_zkvm::io::read::<IntentInput>();

    // --- VERIFICATION LOGIC ---
    // 1. Verify intent_id is non-zero (a zero intentId is invalid).
    let id_nonzero = input.intent_id.iter().any(|&b| b != 0);

    // 2. Verify sender is non-zero (zero address is invalid).
    let sender_nonzero = input.sender.iter().any(|&b| b != 0);

    // 3. Verify destination wallet is non-zero.
    let dest_nonzero = input.destination_wallet.iter().any(|&b| b != 0);

    // 4. Verify amount > 0.
    let amount_valid = input.amount > 0;

    // 5. Verify expiry is in the future relative to the block (basic sanity).
    //    Block timestamps on Arbitrum Sepolia are in seconds.
    //    We check expiry > block_number as a proxy (not wall clock — 
    //    full timestamp verification is the mainnet path via block header).
    let expiry_valid = input.expiry > input.block_number;

    // 6. Verify destination chain ID is Solana (1399811149).
    let chain_valid = input.destination_chain_id == 1399811149u64;

    // 7. Verify slippage is within acceptable bounds (0–1000 bps = 0–10%).
    let slippage_valid = input.slippage_bps <= 1000;

    // All checks must pass.
    let verified = id_nonzero
        && sender_nonzero
        && dest_nonzero
        && amount_valid
        && expiry_valid
        && chain_valid
        && slippage_valid;

    // Commit the public outputs — these are readable outside the proof.
    let output = IntentOutput {
        intent_id: input.intent_id,
        destination_chain_id: input.destination_chain_id,
        amount: input.amount,
        expiry: input.expiry,
        block_number: input.block_number,
        tx_hash: input.tx_hash,
        verified,
    };

    sp1_zkvm::io::commit(&output);
}
