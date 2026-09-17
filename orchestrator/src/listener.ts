import { createPublicClient, http, fallback, parseAbiItem, type Log, type Chain } from "viem";
import { arbitrumSepolia } from "viem/chains";
import * as dotenv from "dotenv";

dotenv.config();

// ─── Config ──────────────────────────────────────────────────────────────────

const INTENT_MANAGER_ADDRESS = "0xab8682775cf43059BCEed90975D8ee8Ac152D505" as const;

const ALCHEMY_RPC_URL_1 = process.env.ALCHEMY_RPC_URL_1;
const ALCHEMY_RPC_URL_2 = process.env.ALCHEMY_RPC_URL_2;
const ROBINHOOD_RPC_URL = process.env.ROBINHOOD_RPC_URL ?? "https://rpc.testnet.chain.robinhood.com";

if (!ALCHEMY_RPC_URL_1 || !ALCHEMY_RPC_URL_2) {
  console.error("[FATAL] ALCHEMY_RPC_URL_1 and ALCHEMY_RPC_URL_2 must both be set in .env");
  process.exit(1);
}

// ─── Chain definitions ───────────────────────────────────────────────────────

const robinhoodTestnet: Chain = {
  id: 46630,
  name: "Robinhood Chain Testnet",
  nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
  rpcUrls: {
    default: { http: [ROBINHOOD_RPC_URL] },
  },
};

// ─── ABI ─────────────────────────────────────────────────────────────────────

const INTENT_CREATED_EVENT = parseAbiItem(
  "event IntentCreated(bytes32 indexed intentId, address indexed sender, uint256 amount, address destinationWallet, uint64 destinationChainId, uint64 expiry, uint16 slippageBps)"
);

const COLLATERAL_POSTED_EVENT = parseAbiItem(
  "event CollateralPosted(bytes32 indexed intentId, address indexed solver, uint256 collateralAmount)"
);

const INTENT_SETTLED_EVENT = parseAbiItem(
  "event IntentSettled(bytes32 indexed intentId, address indexed solver, uint256 amount, bytes32 zkProofHash)"
);

const INTENT_MANAGER_ABI = [INTENT_CREATED_EVENT, COLLATERAL_POSTED_EVENT, INTENT_SETTLED_EVENT] as const;

// ─── Clients ─────────────────────────────────────────────────────────────────
// Arbitrum Sepolia uses viem's fallback() transport across two Alchemy URLs
// so a single provider's rate limit doesn't take the listener down.

const arbClient = createPublicClient({
  chain: arbitrumSepolia,
  transport: fallback([http(ALCHEMY_RPC_URL_1), http(ALCHEMY_RPC_URL_2)]),
});

const rhClient = createPublicClient({
  chain: robinhoodTestnet,
  transport: http(ROBINHOOD_RPC_URL),
});

// ─── Formatting helpers ───────────────────────────────────────────────────────

function formatEther(wei: bigint): string {
  return (Number(wei) / 1e18).toFixed(6);
}

function divider(): void {
  console.log("─".repeat(64));
}

// ─── Event handlers ────────────────────────────────────────────────────────────
// Each handler is wrapped so a single malformed log can never crash the process.

function logIntentCreated(log: Log, chainLabel: string): void {
  try {
    const args = (log as unknown as {
      args: {
        intentId: `0x${string}`;
        sender: `0x${string}`;
        amount: bigint;
        destinationWallet: `0x${string}`;
        destinationChainId: bigint;
        expiry: bigint;
        slippageBps: number;
      };
    }).args;

    const expiryDate = new Date(Number(args.expiry) * 1000).toISOString();

    divider();
    console.log(`[IntentCreated] Chain: ${chainLabel}`);
    console.log(`  intentId:           ${args.intentId}`);
    console.log(`  sender:             ${args.sender}`);
    console.log(`  amount:             ${formatEther(args.amount)} ETH (${args.amount.toString()} wei)`);
    console.log(`  destinationWallet:  ${args.destinationWallet}`);
    console.log(`  destinationChainId: ${args.destinationChainId.toString()}`);
    console.log(`  expiry:             ${expiryDate} (${args.expiry.toString()})`);
    console.log(`  slippageBps:        ${args.slippageBps} (${(args.slippageBps / 100).toFixed(2)}%)`);
    console.log(`  txHash:             ${log.transactionHash}`);
    console.log(`  blockNumber:        ${log.blockNumber?.toString()}`);
    divider();
  } catch (err) {
    console.error(`[ERROR] Failed to process IntentCreated log on ${chainLabel}:`, err);
  }
}

function logCollateralPosted(log: Log, chainLabel: string): void {
  try {
    const args = (log as unknown as {
      args: {
        intentId: `0x${string}`;
        solver: `0x${string}`;
        collateralAmount: bigint;
      };
    }).args;

    divider();
    console.log(`[CollateralPosted] Chain: ${chainLabel}`);
    console.log(`  intentId:         ${args.intentId}`);
    console.log(`  solver:           ${args.solver}`);
    console.log(`  collateralAmount: ${formatEther(args.collateralAmount)} ETH (${args.collateralAmount.toString()} wei)`);
    console.log(`  txHash:           ${log.transactionHash}`);
    console.log(`  blockNumber:      ${log.blockNumber?.toString()}`);
    divider();
  } catch (err) {
    console.error(`[ERROR] Failed to process CollateralPosted log on ${chainLabel}:`, err);
  }
}

function logIntentSettled(log: Log, chainLabel: string): void {
  try {
    const args = (log as unknown as {
      args: {
        intentId: `0x${string}`;
        solver: `0x${string}`;
        amount: bigint;
        zkProofHash: `0x${string}`;
      };
    }).args;

    divider();
    console.log(`[IntentSettled] Chain: ${chainLabel}`);
    console.log(`  intentId:    ${args.intentId}`);
    console.log(`  solver:      ${args.solver}`);
    console.log(`  amount:      ${formatEther(args.amount)} ETH (${args.amount.toString()} wei)`);
    console.log(`  zkProofHash: ${args.zkProofHash}`);
    console.log(`  txHash:      ${log.transactionHash}`);
    console.log(`  blockNumber: ${log.blockNumber?.toString()}`);
    divider();
  } catch (err) {
    console.error(`[ERROR] Failed to process IntentSettled log on ${chainLabel}:`, err);
  }
}

// ─── Watchers ────────────────────────────────────────────────────────────────

function watchChain(client: typeof arbClient | typeof rhClient, chainLabel: string): void {
  console.log(`[${chainLabel}] Watching IntentManager at ${INTENT_MANAGER_ADDRESS}`);

  client.watchContractEvent({
    address: INTENT_MANAGER_ADDRESS,
    abi: INTENT_MANAGER_ABI,
    eventName: "IntentCreated",
    onLogs: (logs) => logs.forEach((log) => logIntentCreated(log, chainLabel)),
    onError: (err) => console.error(`[${chainLabel}] IntentCreated watcher error:`, err.message),
  });

  client.watchContractEvent({
    address: INTENT_MANAGER_ADDRESS,
    abi: INTENT_MANAGER_ABI,
    eventName: "CollateralPosted",
    onLogs: (logs) => logs.forEach((log) => logCollateralPosted(log, chainLabel)),
    onError: (err) => console.error(`[${chainLabel}] CollateralPosted watcher error:`, err.message),
  });

  client.watchContractEvent({
    address: INTENT_MANAGER_ADDRESS,
    abi: INTENT_MANAGER_ABI,
    eventName: "IntentSettled",
    onLogs: (logs) => logs.forEach((log) => logIntentSettled(log, chainLabel)),
    onError: (err) => console.error(`[${chainLabel}] IntentSettled watcher error:`, err.message),
  });
}

// ─── Boot ─────────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  console.log("═".repeat(64));
  console.log("  Maat Orchestrator — Intent Listener");
  console.log(`  Started: ${new Date().toISOString()}`);
  console.log("═".repeat(64) + "\n");

  // Arbitrum Sepolia is required — exit if unreachable on both Alchemy URLs.
  try {
    const arbBlock = await arbClient.getBlockNumber();
    console.log(`[Arbitrum Sepolia] Connected. Latest block: ${arbBlock}`);
  } catch (err) {
    console.error("[FATAL] Cannot connect to Arbitrum Sepolia RPC (both Alchemy URLs failed):", (err as Error).message);
    process.exit(1);
  }

  // Robinhood Chain Testnet is best-effort — warn and continue if unreachable.
  let robinhoodAvailable = true;
  try {
    const rhBlock = await rhClient.getBlockNumber();
    console.log(`[Robinhood Testnet] Connected. Latest block: ${rhBlock}`);
  } catch (err) {
    robinhoodAvailable = false;
    console.warn("[WARN] Cannot connect to Robinhood Testnet RPC. Skipping its watcher.");
    console.warn(`  Error: ${(err as Error).message}`);
  }

  console.log("");
  watchChain(arbClient, "Arbitrum Sepolia");
  if (robinhoodAvailable) {
    watchChain(rhClient, "Robinhood Testnet");
  }

  console.log("\n[Orchestrator] Listening. Waiting for intents...\n");
}

process.on("unhandledRejection", (reason) => {
  console.error("[ERROR] Unhandled rejection (continuing):", reason);
});

process.on("uncaughtException", (err) => {
  console.error("[ERROR] Uncaught exception (continuing):", err);
});

main().catch((err) => {
  console.error("[FATAL] Unhandled error during boot:", err);
  process.exit(1);
});
