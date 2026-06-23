---
title: "feat: One-tap LAN pairing for Backr host/client setup"
type: feat
date: 2026-06-22
status: planned
origin: in-session brainstorm (no requirements doc — decisions captured below)
---

# feat: One-tap LAN pairing for Backr host/client setup

## Summary

Replace Backr's tedious manual two-machine setup (hand-typed host IP/user/key/port/paths plus a manual SSH pubkey copy) with a one-gesture LAN flow: the host app shows a 6-digit code and advertises itself over mDNS; the laptop discovers it, the user types the code, and the laptop auto-generates its SSH key, gets it trusted on the host, pins the host fingerprint, and prefills the setup wizard. Same-LAN only; manual entry remains as a fallback.

---

## Problem Frame

Today setup spans two scripts plus a 3-screen in-app wizard. The two real frictions are (1) **finding** the host — the user hand-types its IP — and (2) **trusting** the key — the laptop's pubkey must land in `~backr/.ssh/authorized_keys`, but `backr` is passwordless so `ssh-copy-id` fails, leaving a confusing manual paste / `sudo tee` step. One-tap pairing collapses discovery + key trust + config prefill into a single code-gated gesture.

---

## Scope Boundaries

**In scope**
- Host: a time-boxed "pairing mode" that generates a 6-digit code, advertises an mDNS service, and runs a temporary local pairing listener.
- Client: mDNS discovery of hosts in pairing mode, code entry, auto SSH key-gen if missing, key submission, receipt of host config, fingerprint pinning, and a prefilled single confirm screen.
- Manual entry preserved as a fallback path into the existing wizard.

**Deferred to Follow-Up Work**
- Headless-host pairing via the installer (`backr --pair` printing a code from the terminal).
- QR codes.

**Outside this scope**
- Off-LAN / internet / VPN pairing (mDNS does not cross subnets).
- Stronger cryptographic pairing (PAKE/SPAKE2) — the brainstorm accepted a 6-digit code as sufficient; see Risks.

---

## Requirements

- **R1** — Host enters an explicit, time-boxed (~3 min) pairing mode from the host dashboard, producing a 6-digit numeric code shown on screen.
- **R2** — While in pairing mode, the host advertises a discoverable service on the LAN (mDNS `_backr._tcp`) and runs a temporary pairing listener; both stop on timeout, on success, or when pairing is cancelled.
- **R3** — The 6-digit code is single-use, expires with the pairing window, and is rate-limited (lock the session after ~5 failed attempts).
- **R4** — The client discovers hosts in pairing mode and lists them (hostname + address); selecting one and entering the code initiates pairing.
- **R5** — During pairing the client generates `~/.ssh/id_ed25519` (passphraseless) if absent, and reuses an existing key otherwise.
- **R6** — On a valid code, the host appends the client's public key to `~backr/.ssh/authorized_keys` (reusing the existing trust path) and replies with `{ ssh_user, ssh_port, backup_root, host_key_fingerprint }`.
- **R7** — The client pins the returned host key into `~/.config/backr/known_hosts` and prefills the setup wizard from the reply, requiring one confirm before saving config.
- **R8** — A manual-entry path into the existing 3-step wizard remains available at all times (discovery failure, AP isolation, firewall, headless host).

---

## Key Technical Decisions

- **mDNS via `mdns-sd`** (pure-Rust, no avahi/C build dependency) for both advertise (host) and browse (client). Keeps the cross-distro installer story intact — no new system package. *(No mDNS crate exists in `src-tauri/Cargo.toml` today.)*
- **Pairing transport = a minimal embedded HTTP listener** (e.g. `tiny_http`) bound to an ephemeral port on the LAN interface, advertised via the mDNS SRV record; single `POST /pair` endpoint taking `{ pubkey, code }` and returning the host config JSON. Chosen over the rejected "temporary one-time SSH password" approach, which would relax `sshd` and widen attack surface.
- **Code gate hardening in `AppState`**: 6 random digits, constant-time compare, consumed on first success, invalidated after ~5 wrong attempts or TTL expiry. The listener only runs while a live session exists.
- **Key generation** shells out to `ssh-keygen -t ed25519 -N "" -f ~/.ssh/id_ed25519` (matches the installer), gated on absence.
- **Host fingerprint** read from the host's `ssh_host_ed25519_key.pub` (or via `ssh-keygen -lf`), returned in the pair reply, and pinned client-side to the app's isolated `~/.config/backr/known_hosts`.
- **Reuse `host_trust::host_append_authorized_pubkey_impl`** for the append so validation/dedup/permissions stay in one place.
- **Pairing UI placement**: host side augments the host dashboard ("Add a laptop"); client side fronts the existing Setup route (shown when no config + no host marker) and falls through to the current `SetupWizard` for manual entry.

---

## High-Level Technical Design

```mermaid
sequenceDiagram
    participant HU as Host UI (Svelte)
    participant HR as Host Rust (pairing)
    participant mDNS as LAN mDNS
    participant CR as Client Rust (pairing)
    participant CU as Client UI (Svelte)

    HU->>HR: start_pairing()
    HR->>HR: gen 6-digit code + session (TTL, attempts)
    HR->>mDNS: advertise _backr._tcp (SRV: ip:port)
    HR-->>HU: { code, expires_at }
    Note over HU: shows code + countdown

    CU->>CR: discover_hosts()
    CR->>mDNS: browse _backr._tcp
    mDNS-->>CR: [host: name, ip, port]
    CR-->>CU: host list
    CU->>CR: pair_with_host(host, code)
    CR->>CR: ssh-keygen if missing
    CR->>HR: POST /pair { pubkey, code }
    HR->>HR: validate code (const-time, attempts, TTL)
    alt valid
        HR->>HR: append pubkey to ~backr authorized_keys
        HR-->>CR: { ssh_user, ssh_port, backup_root, host_fingerprint }
        CR->>CR: pin fingerprint -> ~/.config/backr/known_hosts
        CR-->>CU: prefilled config draft
        Note over CU: single confirm -> save -> dashboard
        HR->>mDNS: stop advertise + close listener
    else invalid / expired / locked
        HR-->>CR: error
        CR-->>CU: surface error; manual fallback available
    end
```

---

## Implementation Units

### U1. Add mDNS + pairing dependencies and module scaffold
- **Goal**: Introduce `mdns-sd` and the HTTP-listener/code dependencies and a `pairing` module skeleton.
- **Requirements**: R2, R4
- **Dependencies**: none
- **Files**: `src-tauri/Cargo.toml`, `src-tauri/src/pairing/mod.rs`, `src-tauri/src/lib.rs` (module decl)
- **Approach**: Add `mdns-sd`, a minimal HTTP server crate, and a small RNG; create `pairing` module with submodule stubs (`code`, `listener`, `discovery`, `client`). Service type constant `_backr._tcp.local.`.
- **Patterns to follow**: existing module layout under `src-tauri/src/` and command registration in `src-tauri/src/lib.rs`.
- **Test scenarios**: Test expectation: none — dependency + scaffold only; behavior lands in later units.
- **Verification**: `cargo build` succeeds with the new crates and module tree.

### U2. Pairing session + 6-digit code state
- **Goal**: Model the code session (generate, validate, expire, single-use, rate-limit) in shared state.
- **Requirements**: R1, R3
- **Dependencies**: U1
- **Files**: `src-tauri/src/pairing/code.rs`, `src-tauri/src/state.rs` (add `pairing: Mutex<Option<PairingSession>>`)
- **Approach**: `PairingSession { code, expires_at, attempts_left, consumed }`. Generation = 6 random digits. `validate(code)` does constant-time compare, decrements attempts, enforces TTL, and consumes on success. Expiry/lockout invalidate the session.
- **Patterns to follow**: `AppState` field style in `src-tauri/src/state.rs` (Tokio `Mutex`).
- **Test scenarios**:
  - Generated code is exactly 6 digits.
  - Correct code within TTL and attempts → valid, and marks session consumed (second attempt with same code → rejected).
  - Wrong code decrements attempts; after 5 wrong attempts the session is locked (further attempts rejected even if correct).
  - Code past `expires_at` → rejected as expired.
  - Comparison is constant-time (no early return on first mismatched digit).
- **Verification**: `cargo test` for `pairing::code` passes all scenarios.

### U3. Host pairing listener
- **Goal**: Temporary `POST /pair` endpoint that validates the code, trusts the key, and returns host config.
- **Requirements**: R2, R6
- **Dependencies**: U2
- **Files**: `src-tauri/src/pairing/listener.rs`, reuse `src-tauri/src/host_trust.rs`, read host marker via `src-tauri/src/host_config.rs`
- **Approach**: Bind ephemeral port on the LAN interface. On `POST /pair { pubkey, code }`: validate via the U2 session; on success call `host_trust::host_append_authorized_pubkey_impl(pubkey)` and respond `{ ssh_user, ssh_port, backup_root, host_key_fingerprint }` sourced from `/etc/backr/host.toml` + effective sshd port + the host's `ssh_host_ed25519_key.pub` fingerprint. Reject invalid/expired/locked with a non-revealing error. Listener lifecycle owned by U4.
- **Patterns to follow**: error-string return convention used across `src-tauri/src/commands/*` and `host_trust.rs`.
- **Test scenarios**:
  - Valid code → appends pubkey (assert authorized_keys contains it) and returns all four config fields.
  - Invalid/expired/locked code → no append, error response.
  - Malformed body (missing pubkey/code, non-pubkey string) → rejected without touching authorized_keys.
  - Duplicate pubkey already trusted → succeeds idempotently (reuses `host_append` dedup behavior).
- **Verification**: integration test drives the endpoint against a temp authorized_keys and asserts append + response shape.

### U4. Host pairing-mode commands + mDNS advertise
- **Goal**: `start_pairing` / `stop_pairing` / `pairing_status` commands that wire the code, listener, and mDNS advertisement together with the TTL teardown.
- **Requirements**: R1, R2
- **Dependencies**: U2, U3
- **Files**: `src-tauri/src/commands/pairing_cmd.rs`, `src-tauri/src/pairing/discovery.rs` (advertise half), `src-tauri/src/lib.rs` (register commands)
- **Approach**: `start_pairing` creates the session (U2), starts the listener (U3), registers `_backr._tcp` with SRV (ip:port) + TXT (hostname), returns `{ code, expires_at }`. A timer (and `stop_pairing`) tears down advertise + listener and clears the session; success also tears down.
- **Patterns to follow**: command signature + `generate_handler!` registration in `src-tauri/src/lib.rs`; async command style in `src-tauri/src/commands/host_cmd.rs`.
- **Test scenarios**:
  - `start_pairing` returns a 6-digit code and a future `expires_at`; service becomes discoverable (assert registration call).
  - `stop_pairing` removes the advertisement and closes the listener; subsequent `/pair` fails.
  - TTL elapse tears down advertise + listener without an explicit stop.
- **Verification**: `cargo test` + manual: start pairing, confirm the service is browsable from another machine, confirm teardown on timeout.

### U5. Client discovery + pairing
- **Goal**: `discover_hosts` (mDNS browse) and `pair_with_host` (key-gen, submit, receive, pin fingerprint, build draft config).
- **Requirements**: R4, R5, R6, R7
- **Dependencies**: U1, U3
- **Files**: `src-tauri/src/pairing/discovery.rs` (browse half), `src-tauri/src/pairing/client.rs`, `src-tauri/src/config.rs` (known_hosts pin helper + `PrefilledConfig` DTO), `src-tauri/src/commands/pairing_cmd.rs`
- **Approach**: `discover_hosts` browses `_backr._tcp` for a short window, returns `[{ hostname, ip, port }]`. `pair_with_host(host, code)` ensures `~/.ssh/id_ed25519` (shell `ssh-keygen` if missing), `POST`s `{ pubkey, code }` to the host, on success pins `host_key_fingerprint` into `~/.config/backr/known_hosts` and returns a `Config`-shaped draft (`remote.host/user/port/backup_path`, `local.projects_path` default `~/Projects`). Surface a typed error on failure so the UI can offer manual fallback.
- **Patterns to follow**: `Config` shape in `src-tauri/src/config.rs`; the app's isolated `known_hosts` convention noted in `src/components/setup/StepRemote.svelte`.
- **Test scenarios**:
  - Key absent → generated passphraseless at `~/.ssh/id_ed25519`; key present → reused, not overwritten.
  - Successful pair → returns draft with all remote fields populated and pins the fingerprint to the isolated known_hosts.
  - Host rejects code → typed error returned; no config draft, no known_hosts write.
  - Discovery with no hosts → empty list (not an error).
- **Verification**: `cargo test` against a stubbed listener; manual end-to-end pair on a real LAN.

### U6. IPC wrappers + types
- **Goal**: Typed `src/lib/commands.ts` wrappers and TS types for the pairing commands.
- **Requirements**: R1, R4, R7
- **Dependencies**: U4, U5
- **Files**: `src/lib/commands.ts`, `src/types/pairing.ts`
- **Approach**: Add `startPairing`, `stopPairing`, `pairingStatus`, `discoverHosts`, `pairWithHost` wrappers. **Args must be camelCase** (Tauri v2 — e.g. `pairWithHost(host, code)` → `invoke("pair_with_host", { host, code })`). Result DTOs use snake_case to match serde defaults, consistent with existing types.
- **Patterns to follow**: `src/lib/commands.ts` wrapper style and the camelCase-arg / snake_case-result convention established across existing commands and `src/types/*`.
- **Test scenarios**: Test expectation: none — thin typed IPC pass-throughs; behavior covered in U2–U5 and the UI units. (Optional: a dev-mock entry mirroring existing `src/lib/devMock/backend.ts` patterns.)
- **Verification**: `npm run check` (svelte-check/tsc) passes; arg keys are camelCase.

### U7. Host UI — "Add a laptop" pairing panel
- **Goal**: Host-dashboard panel to start pairing, show the code + countdown, and confirm when a laptop pairs.
- **Requirements**: R1, R2
- **Dependencies**: U6
- **Files**: `src/components/host/PairingPanel.svelte` (new), wired into `src/components/host/HostDashboardView.svelte` or `src/components/host/HostTrustKeysView.svelte`
- **Approach**: "Add a laptop" button → `startPairing` → display the 6-digit code large with a live countdown to `expires_at`; poll/await pairing result and show "Laptop trusted." On unmount/timeout/cancel call `stopPairing`.
- **Patterns to follow**: existing host views and the `host_trust` UI in `src/components/host/HostTrustKeysView.svelte`; Svelte 5 runes style used across `src/components`.
- **Test scenarios**:
  - Clicking "Add a laptop" shows a 6-digit code and a counting-down timer.
  - Countdown reaching zero returns the panel to idle and stops advertising (asserts `stopPairing` called).
  - Successful pair shows the trusted-confirmation state.
- **Verification**: manual — code displays, countdown works, success state shows after a real pair; dev-mock renders without a backend.

### U8. Client UI — discovery + code entry feeding the prefilled wizard
- **Goal**: Front the Setup flow with host discovery + code entry that prefills `SetupWizard`, preserving manual entry.
- **Requirements**: R4, R7, R8
- **Dependencies**: U6
- **Files**: `src/components/setup/PairHost.svelte` (new), `src/components/setup/SetupWizard.svelte` (accept a prefilled draft + entry routing), `src/routes.ts` if the Setup entry point needs a branch
- **Approach**: On the Setup route (no config), default to `PairHost`: poll `discoverHosts`, list found hosts, let the user pick one and enter the code → `pairWithHost` → pass the returned draft into `SetupWizard` positioned at the Verify/confirm step. A persistent "Enter details manually" link routes to the existing blank `SetupWizard` (R8). Surface discovery/pair errors with the manual fallback.
- **Patterns to follow**: `SetupWizard` step/draft model and `replace()` navigation in `src/components/setup/SetupWizard.svelte`; existing field components `StepRemote`/`StepPaths`/`StepVerify`.
- **Test scenarios**:
  - Discovery list renders found hosts; empty discovery shows an empty state with the manual fallback visible.
  - Valid pick + code → wizard opens prefilled (host/user/port/backup_root populated) at the confirm step.
  - Pair failure (bad code) → inline error; manual fallback still reachable.
  - "Enter details manually" → blank 3-step wizard (existing behavior unchanged).
- **Verification**: manual end-to-end against a host in pairing mode; dev-mock path renders discovery + prefill without a backend.

---

## Risks & Mitigations

- **mDNS unreliable / blocked** (corporate APs, client isolation, multiple subnets) → manual entry fallback (R8) always present; discovery empty-state guides to it.
- **6-digit brute force** → single-use + ~3-min TTL + ~5-attempt lockout make online guessing within the window infeasible (U2/U3).
- **Malicious host on a shared LAN (code leak / MITM)**: an attacker advertising the same service could capture the typed code and replay it on the real host to trust *their* key. Accepted tradeoff per brainstorm (code is "enough"); mitigations: short TTL, single-use, the user picks the host by name/IP, and the host fingerprint is shown/pinned. **Documented residual risk**; PAKE/SPAKE2 hardening is deferred (see Scope Boundaries).
- **Firewall blocks the ephemeral pairing port** → pairing fails cleanly with a typed error → manual fallback.
- **Multiple hosts / multi-NIC** → discovery lists all; user selects; advertise binds the LAN interface.

---

## System-Wide Impact

- New runtime dependency surface (`mdns-sd` + HTTP listener) in `src-tauri`; verify the cross-distro installers still build (no new system packages expected since `mdns-sd` is pure Rust).
- New Tauri commands registered in `src-tauri/src/lib.rs` and exposed via `src/lib/commands.ts`.
- The Setup route gains a discovery front-end; the existing manual wizard is preserved as the fallback, not replaced.
- A temporary inbound listener is a new (time-boxed, code-gated) network surface — keep its lifecycle strictly bounded to pairing mode.

---

## Open Questions / Deferred

- **Headless hosts** (no GUI to click "Add a laptop"): deferred — would add `backr --pair` to the binary + a host-installer step.
- **QR codes** and **off-LAN/internet pairing**: deferred / out of scope.
- **Stronger pairing crypto (PAKE)**: deferred; revisit if the shared-LAN MITM residual risk becomes a real concern.
- **Exact crate choices** (`tiny_http` vs alternative; fingerprint via `ssh-keygen -lf` vs computed) are KTDs to confirm at implementation against current versions.
