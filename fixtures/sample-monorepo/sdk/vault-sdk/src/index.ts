/**
 * Vault SDK — TypeScript client for the vault program.
 */

export interface VaultState {
  admin: string;
  totalDeposits: number;
  feeBps: number;
}

export interface UserPosition {
  owner: string;
  shares: number;
  lastDepositTs: number;
}

export class VaultClient {
  private endpoint: string;

  constructor(endpoint: string) {
    this.endpoint = endpoint;
  }

  async deposit(amount: number): Promise<string> {
    // RPC call
    return "tx_signature";
  }

  async withdraw(shares: number): Promise<string> {
    // RPC call
    return "tx_signature";
  }

  async getVaultState(): Promise<VaultState> {
    return {
      admin: "",
      totalDeposits: 0,
      feeBps: 0,
    };
  }
}

export type TransactionResult = {
  signature: string;
  slot: number;
};

export function createVaultClient(endpoint: string): VaultClient {
  return new VaultClient(endpoint);
}
