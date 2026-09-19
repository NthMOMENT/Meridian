use anyhow::{anyhow, Result};
use dotenv::dotenv;
use serde::{Deserialize, Serialize};
use sp1_sdk::{include_elf, Elf, HashableKey, ProveRequest, Prover, ProvingKey, ProverClient, SP1Stdin};
use std::env;

#[derive(Serialize, Deserialize)]
pub struct IntentInput {
    pub intent_id: [u8; 32],
    pub sender: [u8; 20],
    pub amount: u64,
    pub destination_wallet: [u8; 20],
    pub destination_chain_id: u64,
    pub expiry: u64,
    pub slippage_bps: u16,
    pub block_number: u64,
    pub tx_hash: [u8; 32],
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IntentOutput {
    pub intent_id: [u8; 32],
    pub destination_chain_id: u64,
    pub amount: u64,
    pub expiry: u64,
    pub block_number: u64,
    pub tx_hash: [u8; 32],
    pub verified: bool,
}

const MAAT_ZK_ELF: Elf = include_elf!("maat-zk-program");

fn fetch_logs(rpc_url: &str, topic: &str) -> Result<serde_json::Value> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_getLogs",
        "params": [{
            "address": "0xab8682775cf43059BCEed90975D8ee8Ac152D505",
            "topics": [topic],
            "fromBlock": "0x12772c2c",
            "toBlock": "0x12772c2c"
        }]
    });
    let resp = reqwest::blocking::Client::new()
        .post(rpc_url).json(&body).send()?
        .json::<serde_json::Value>()?;
    resp.get("result").cloned()
        .ok_or_else(|| anyhow!("RPC error: {:?}", resp))
}

fn hex_to_fixed<const N: usize>(s: &str) -> Result<[u8; N]> {
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s)?;
    if bytes.len() < N {
        let mut arr = [0u8; N];
        arr[N - bytes.len()..].copy_from_slice(&bytes);
        Ok(arr)
    } else {
        let mut arr = [0u8; N];
        arr.copy_from_slice(&bytes[bytes.len() - N..]);
        Ok(arr)
    }
}

fn hex_to_u64(s: &str) -> Result<u64> {
    Ok(u64::from_str_radix(s.trim_start_matches("0x"), 16)?)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    sp1_sdk::utils::setup_logger();

    let mode = env::args().nth(1).unwrap_or_else(|| "execute".to_string());
    let rpc_url = env::var("ALCHEMY_RPC_URL_1")
        .map_err(|_| anyhow!("ALCHEMY_RPC_URL_1 not set in .env"))?;

    println!("[maat-zk] Fetching IntentCreated events from Arbitrum Sepolia...");

    let logs = fetch_logs(
        &rpc_url,
        "0x8ccdcf0586d2287281cf80bf9204a6bc49bebf6105d31015eb8fc423f881f0d8",
    )?;

    let logs_arr = logs.as_array()
        .ok_or_else(|| anyhow!("Expected array of logs"))?;

    if logs_arr.is_empty() {
        return Err(anyhow!("No IntentCreated events found. Run fire_intent.sh first."));
    }

    let log = &logs_arr[logs_arr.len() - 1];
    println!("[maat-zk] Found {} event(s). Using most recent.", logs_arr.len());

    let topics = log["topics"].as_array()
        .ok_or_else(|| anyhow!("No topics in log"))?;

    let intent_id = hex_to_fixed::<32>(
        topics.get(1).and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("Missing topic[1]"))?,
    )?;
    let sender = hex_to_fixed::<20>(
        topics.get(2).and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("Missing topic[2]"))?,
    )?;

    let data_bytes = hex::decode(
        log["data"].as_str().ok_or_else(|| anyhow!("Missing data"))?
            .trim_start_matches("0x")
    )?;

    if data_bytes.len() < 160 {
        return Err(anyhow!("Event data too short: {} bytes", data_bytes.len()));
    }

    let amount               = u64::from_be_bytes(data_bytes[24..32].try_into()?);
    let destination_wallet: [u8; 20] = data_bytes[44..64].try_into()?;
    let destination_chain_id = u64::from_be_bytes(data_bytes[88..96].try_into()?);
    let expiry               = u64::from_be_bytes(data_bytes[120..128].try_into()?);
    let slippage_bps         = u16::from_be_bytes(data_bytes[158..160].try_into()?);
    let block_number = hex_to_u64(log["blockNumber"].as_str()
        .ok_or_else(|| anyhow!("Missing blockNumber"))?)?;
    let tx_hash = hex_to_fixed::<32>(log["transactionHash"].as_str()
        .ok_or_else(|| anyhow!("Missing transactionHash"))?)?;

    println!("[maat-zk] Intent parsed:");
    println!("  intentId:           0x{}", hex::encode(intent_id));
    println!("  sender:             0x{}", hex::encode(sender));
    println!("  amount:             {}", amount);
    println!("  destinationWallet:  0x{}", hex::encode(destination_wallet));
    println!("  destinationChainId: {}", destination_chain_id);
    println!("  expiry:             {}", expiry);
    println!("  slippageBps:        {}", slippage_bps);
    println!("  blockNumber:        {}", block_number);
    println!("  txHash:             0x{}", hex::encode(tx_hash));

    let input = IntentInput {
        intent_id, sender, amount, destination_wallet,
        destination_chain_id, expiry, slippage_bps, block_number, tx_hash,
    };

    let mut stdin = SP1Stdin::new();
    stdin.write(&input);

    let client = ProverClient::builder().cpu().build().await;
    let pk = client.setup(MAAT_ZK_ELF).await
        .map_err(|e| anyhow!("Setup failed: {:?}", e))?;

    println!("[maat-zk] Verification key: {}", pk.verifying_key().bytes32());

    if mode == "execute" {
        println!("[maat-zk] MODE: execute (no proof)");
        let (mut public_values, report) = client
            .execute(MAAT_ZK_ELF, stdin)
            .await
            .map_err(|e| anyhow!("Execution failed: {:?}", e))?;
        let output: IntentOutput = public_values.read::<IntentOutput>();
        println!("[maat-zk] Execution complete.");
        println!("  verified: {}", output.verified);
        println!("  cycles:   {}", report.total_instruction_count());
        if output.verified == false {
            return Err(anyhow!("Intent failed ZK verification checks."));
        }
        println!("[maat-zk] Intent is valid. Ready to prove on VPS.");
    } else {
        println!("[maat-zk] MODE: prove (10-30min on CPU)");
        let proof = client
            .prove(&pk, stdin)
            .mode(sp1_sdk::SP1ProofMode::Compressed)
            .await
            .map_err(|e| anyhow!("Proving failed: {:?}", e))?;

        let mut pv = proof.public_values.clone();
        let output: IntentOutput = pv.read::<IntentOutput>();

        println!("[maat-zk] PROOF GENERATED.");
        println!("  verified: {}", output.verified);
        println!("  intentId: 0x{}", hex::encode(output.intent_id));
        println!("  vkey:     {}", pk.verifying_key().bytes32());

        proof.save("proof_output.bin")?;
        println!("[maat-zk] Proof saved to: proof_output.bin");
    }

    Ok(())
}
