# Meridian by NTH MOMENT

> Cross-chain solvers are unsecured credit facilities operating without underwriting. Meridian is the first protocol to treat them like it.

Meridian is a decentralized, ZK verified cross chain intent network. Sign one transaction on any chain, SP1 zero-knowledge proofs verify source chain finality and settle the exact amount to your destination wallet. No bridges. No wrapped tokens. No gas management. One signature and in under 45 seconds.

---

## Architecture

| Layer | Stack | Role |
|---|---|---|
| Orchestration | Cosmos SDK (CometBFT) | Records intents & settlement confirmations. Source of truth. Holds zero user funds. |
| Settlement | Solana Anchor (Rust) | Listens for ZK-verified commands. Releases funds to destination wallet. |
| ZK Verification | SP1 by Succinct | Cryptographically proves source-chain finality before destination payout triggers. |
| Relayers / Solvers | TypeScript + Rust | Monitor chains, fetch proofs, submit to orchestration chain. Collateralized & credit-scored. |
| Frontend | Next.js + Wagmi + Viem | Mobile-first UI. Framer Motion progress tracker. Zero jargon. |
| Node Hardware | Orange Pi 5 (16GB RAM) | Globally distributed $100 micro-computers. No central point of control. |
| Security | [glassofbeer.ai](https://glassofbeer.ai) | In-house smart and commercial contract audit agent. |

---

## Repo Structure

```
/frontend               → Next.js UI (Week 3)
/orchestrator           → Intent listener + orchestration chain
/orchestrator/cas-chain → Cosmos SDK app chain
/contracts/evm          → IntentManager.sol (Arbitrum Sepolia + Robinhood Chain)
/contracts/solana       → Anchor settlement contract (Solana Devnet)
/zk                     → SP1 proof programs
/docs                   → Architecture diagrams
```

---

## Smart Contracts

### Solana Anchor Settlement Contract

Anchor 0.31.0 — Solana Devnet

| Instruction | Description |
|---|---|
| `initialize` | Sets admin + orchestrator on ProgramState |
| `submit_intent` | Escrows lamports, stores intent on-chain |
| `receive_settlement` | ZK-verified settlement with slippage enforcement and refund |
| `cancel_intent` | Owner reclaims escrowed lamports after expiry |
| `set_circuit_breaker` | Admin halts/unhalts channels by channel ID |
| `pause` / `unpause` | Emergency admin controls |

Audit: **21/21 exploit checks 0 findings** ([glassofbeer.ai/heist](https://glassofbeer.ai/heist))

### EVM IntentManager

Solidity 0.8.24 Arbitrum Sepolia + Robinhood Chain Testnet

| Function | Description |
|---|---|
| `submitIntent` | Escrows ETH, creates intent, enforces circuit breaker |
| `postCollateral` | Solver posts 150% overcollateralized ETH |
| `confirmSettlement` | Orchestrator releases escrow to solver after ZK proof |
| `slashSolver` | Orchestrator slashes failed solver, reimburses user |
| `cancelIntent` | User reclaims escrow if intent expired with no solver |

Audit: **30/30 exploit checks 0 findings** including live reentrancy attack ([glassofbeer.ai/heist](https://glassofbeer.ai/heist))

---

## Deployed Contracts

### EVM

| Network | Address | Explorer |
|---|---|---|
| Arbitrum Sepolia | `0xab8682775cf43059BCEed90975D8ee8Ac152D505` | [Arbiscan](https://sepolia.arbiscan.io/address/0xab8682775cf43059bceed90975d8ee8ac152d505) |
| Robinhood Chain Testnet | `0xab8682775cf43059BCEed90975D8ee8Ac152D505` | [Explorer](https://explorer.testnet.chain.robinhood.com/address/0xab8682775cf43059bceed90975d8ee8ac152d505) |

### Solana

| Network | Program ID |
|---|---|
| Solana Devnet | `AznTbk9GmjfgF7KHb6pbxXnMG2354HZNVsTi3PSz2JjP` |

---

## Security Model

- **Solver Credit Scoring** — collateral, success rate, and latency determine routing tier
- **Collateral Slashing** — automatic on failed settlement within ZK-verified time limit
- **Circuit Breakers** — channel halts if volume exceeds 500% of daily moving average
- **Finality Thresholds** — destination payout blocked until source-chain hard finality proven
- **ReentrancyGuard** — live reentrancy attack blocked under real chain conditions

All smart contracts audited by [glassofbeer.ai/heist](https://glassofbeer.ai/heist) — in-house and commercial adversarial exploit agent

---

## The Risk Framework

Meridian applies TradFi credit risk principles to DeFi solver infrastructure:

- Solvers are treated as **unsecured credit facilities** and must be underwritten
- Collateral requirements scale with intent size
- High-value intents route only to Tier-1 solvers
- All protocol risk controls are enforced on-chain

---

**Solo Founder Build | NTH MOMENT**

---

## Links

- Website: [fouriers.xyz](https://fouriers.xyz)
- X: [@0xfourier](https://x.com/0xfourier)
- Security: [glassofbeer.ai/heist](https://glassofbeer.ai/heist)
- Smart contract audit tool: [glassofbeer.ai](https://glassofbeer.ai)

---

*Meridian — by NTH MOMENT | Solo Founder Build | Colosseum Crypto World's Fair 2026*
