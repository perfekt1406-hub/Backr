/*
 * ipc/protocol.rs — daemon-side re-export of the shared IPC wire types.
 *
 * The wire types (`IpcRequest`, `IpcResponse`, `IpcError`, `IpcEvent`) now live
 * in `backr_core::ipc_protocol` so the daemon and the GUI client share one
 * definition and cannot drift apart. This module re-exports them under their
 * original `crate::ipc::protocol::*` path so the rest of the daemon (server,
 * handlers, scheduler, tray) keeps compiling unchanged.
 */

pub use backr_core::ipc_protocol::{IpcError, IpcEvent, IpcRequest, IpcResponse};
