//! Instruction helpers for building Solana transactions from Titan swap routes.

use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::address_lookup_table::AddressLookupTableAccount;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use titan_api_types::ws::v1::SwapRoute;

use crate::error::TitanClientError;

/// Output from preparing swap instructions for transaction building.
#[derive(Debug, Clone)]
pub struct TitanInstructionsOutput {
    /// Address lookup table accounts (fetched from chain using ALT pubkeys)
    pub address_lookup_table_accounts: Vec<AddressLookupTableAccount>,

    /// Instructions to include in the transaction
    pub instructions: Vec<Instruction>,

    /// Recommended compute units for the transaction
    pub compute_units: Option<u64>,

    /// Safe compute units estimate (includes buffer)
    pub compute_units_safe: Option<u64>,
}

/// Helper for preparing Titan swap instructions.
pub struct TitanInstructions;

impl TitanInstructions {
    /// Fetch address lookup tables from chain and convert a SwapRoute to transaction-ready output.
    ///
    /// # Arguments
    /// * `route` - The swap route from a quote response
    /// * `rpc_client` - Solana RPC client for fetching ALT accounts
    ///
    /// # Returns
    /// Instructions and ALT accounts ready for building a transaction.
    #[tracing::instrument(skip_all)]
    pub async fn prepare_instructions(
        route: &SwapRoute,
        rpc_client: &RpcClient,
    ) -> Result<TitanInstructionsOutput, TitanClientError> {
        // Fetch address lookup table accounts
        let address_lookup_table_accounts =
            Self::fetch_address_lookup_tables(&route.address_lookup_tables, rpc_client).await?;

        // Convert instructions from titan-api-types to solana-sdk types
        let instructions = Self::convert_instructions(&route.instructions);

        Ok(TitanInstructionsOutput {
            address_lookup_table_accounts,
            instructions,
            compute_units: route.compute_units,
            compute_units_safe: route.compute_units_safe,
        })
    }

    /// Fetch address lookup table accounts from the chain.
    #[tracing::instrument(skip_all)]
    pub async fn fetch_address_lookup_tables(
        addresses: &[titan_api_types::common::Pubkey],
        rpc_client: &RpcClient,
    ) -> Result<Vec<AddressLookupTableAccount>, TitanClientError> {
        let mut accounts = Vec::with_capacity(addresses.len());

        for key in addresses {
            // Convert titan Pubkey to solana Pubkey
            let solana_key: Pubkey = (*key).into();

            match rpc_client.get_account(&solana_key).await {
                Ok(account) => match AddressLookupTable::deserialize(&account.data) {
                    Ok(address_lookup_table) => {
                        accounts.push(AddressLookupTableAccount {
                            key: solana_key,
                            addresses: address_lookup_table.addresses.to_vec(),
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to deserialize ALT {}: {}", solana_key, e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to fetch ALT {}: {}", solana_key, e);
                }
            }
        }

        Ok(accounts)
    }

    /// Convert titan-api-types instructions to solana-sdk instructions.
    pub fn convert_instructions(
        titan_instructions: &[titan_api_types::common::Instruction],
    ) -> Vec<Instruction> {
        titan_instructions
            .iter()
            .map(|ix| Instruction {
                program_id: ix.program_id.into(),
                accounts: ix
                    .accounts
                    .iter()
                    .map(|acc| AccountMeta {
                        pubkey: acc.pubkey.into(),
                        is_signer: acc.is_signer,
                        is_writable: acc.is_writable,
                    })
                    .collect(),
                data: ix.data.clone(),
            })
            .collect()
    }

    /// Convert a single titan-api-types instruction to solana-sdk instruction.
    pub fn convert_instruction(titan_ix: &titan_api_types::common::Instruction) -> Instruction {
        Instruction {
            program_id: titan_ix.program_id.into(),
            accounts: titan_ix
                .accounts
                .iter()
                .map(|acc| AccountMeta {
                    pubkey: acc.pubkey.into(),
                    is_signer: acc.is_signer,
                    is_writable: acc.is_writable,
                })
                .collect(),
            data: titan_ix.data.clone(),
        }
    }
}
