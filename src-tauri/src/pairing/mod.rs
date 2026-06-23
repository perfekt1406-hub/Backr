/*
 * One-tap LAN pairing.
 *
 * Lets a laptop discover this host over mDNS and get its SSH public key trusted
 * via a time-boxed 6-digit code, replacing the manual host-IP + pubkey-copy setup.
 * This module holds the pairing primitives; the Tauri commands that drive them
 * live in `commands/pairing_cmd.rs`.
 */

pub mod code;

/// mDNS service type a host advertises while in pairing mode and a client browses for.
pub const PAIRING_SERVICE_TYPE: &str = "_backr._tcp.local.";
