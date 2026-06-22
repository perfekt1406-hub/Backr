#!/usr/bin/env node
/**
 * Backr — connecting-machine setup questionnaire (OpenClaw-style UX).
 *
 * Purpose: Run an interactive, keyboard-driven wizard using @clack/prompts (same family as OpenClaw’s CLI onboarding).
 * Role: Emits a small shell snippet (--env-file) so setup-connecting-client.sh can source SURVEY_CLIENT_* and BACKUP_SSH_TARGET (minimal two-step flow).
 *
 * External: @clack/prompts — terminal prompts (inputs: message/options; outputs: user selections / cancellation signals).
 */

import * as p from "@clack/prompts";
import { writeFileSync } from "node:fs";
import { parseArgs } from "node:util";

/**
 * Shell-single-quote a value for safe `export VAR='…'` lines.
 *
 * @param {string} s Raw string.
 * @returns {string} Quoted string for bash.
 */
function shellSingleQuote(s) {
  return `'${String(s).replace(/'/g, `'\\''`)}'`;
}

/**
 * Writes fallback «unknown» exports when the user aborts early.
 *
 * @param {string} envFile Absolute path to the env file.
 */
function writeCancelledDefaults(envFile) {
  const lines = [
    "export SURVEY_CLIENT_NETWORK=unknown",
    "export SURVEY_CLIENT_SERVER_READY=unknown",
    "export SURVEY_CLIENT_SSH_PORT=unknown",
    "export SURVEY_CLIENT_SSH_CUSTOM_PORT=",
    "export SURVEY_CLIENT_HOST_PLAN=unknown",
    "export SURVEY_CLIENT_GEN_SSH_KEY=",
    "# BACKUP_SSH_TARGET unchanged — set via CLI/env if needed",
  ];
  writeFileSync(envFile, `${lines.join("\n")}\n`, "utf8");
}

/**
 * Parses argv for --env-file and backup target hints passed from bash.
 *
 * @returns {{ envFile: string, backupCli: string, backupEnv: string }}
 */
function readCli() {
  const { values } = parseArgs({
    options: {
      "env-file": { type: "string" },
      "backup-target-cli": { type: "string", default: "" },
      "backup-target-env": { type: "string", default: "" },
    },
  });
  const envFile = values["env-file"];
  if (!envFile) {
    console.error("error: --env-file PATH is required");
    process.exit(2);
  }
  return {
    envFile,
    backupCli: values["backup-target-cli"] ?? "",
    backupEnv: values["backup-target-env"] ?? "",
  };
}

/**
 * Runs the minimal questionnaire and writes export lines for bash.
 *
 * @param {{ envFile: string, backupCli: string, backupEnv: string }} ctx Parsed CLI context.
 */
async function runWizard(ctx) {
  const { envFile, backupCli, backupEnv } = ctx;
  let backupSshTarget = (backupCli || backupEnv || "").trim();

  p.intro("◆ Backr — laptop setup");

  p.note(
    "Public key = one line from ~/.ssh/id_ed25519.pub (OK to copy). Private key stays on this laptop. Trust keys = Backr screen on the backup PC (#/host/trust) to paste that line.",
    "SSH in one sentence",
  );

  // Ask whether backups should use an SSH key.  "yes" → the bash side reuses an
  // existing ~/.ssh/id_ed25519 or creates one if missing ("create if not found,
  // use if found").  Asked via clack, which attaches /dev/tty, so the prompt works
  // under `curl | bash`.
  const wantKey = await p.confirm({
    message:
      "Use an SSH key for passwordless backups? (creates one if you don't have it — required for scheduled / cron backups)",
    initialValue: true,
  });
  if (p.isCancel(wantKey)) {
    writeCancelledDefaults(envFile);
    process.exit(0);
  }
  const genSshKey = wantKey ? "yes" : "no";

  const portChoice = await p.select({
    message: "Which SSH port does the backup server's sshd listen on (from here)?",
    options: [
      { value: "default", label: "Default 22" },
      { value: "custom", label: "Custom — I'll type the port next" },
      { value: "later", label: "I'll figure it out after testing connectivity" },
      { value: "unknown", label: "I'll set it up myself" },
    ],
  });
  if (p.isCancel(portChoice)) {
    writeCancelledDefaults(envFile);
    process.exit(0);
  }

  let sshPort = "unknown";
  /** @type {string} */
  let sshCustom = "";
  if (portChoice === "default") {
    sshPort = "default";
  } else if (portChoice === "custom") {
    const raw = await p.text({
      message: "SSH TCP port on the backup server",
      placeholder: "2222",
      validate(v) {
        const t = String(v).trim();
        if (!/^\d+$/.test(t)) {
          return "Use digits only";
        }
        return undefined;
      },
    });
    if (p.isCancel(raw)) {
      writeCancelledDefaults(envFile);
      process.exit(0);
    }
    sshPort = "custom";
    sshCustom = String(raw).trim();
  } else {
    sshPort = "unknown";
  }

  /** @type {string} */
  let hostPlanOut = "cli_ok";
  if (!backupSshTarget) {
    const raw = await p.text({
      message:
        "Backup SSH host for testing after setup (optional). IP or hostname — add user@ if not «backr». Empty to skip.",
      placeholder: "192.168.1.50 or backr@nas.local",
    });
    if (p.isCancel(raw)) {
      writeCancelledDefaults(envFile);
      process.exit(0);
    }
    backupSshTarget = String(raw).trim();
    hostPlanOut = backupSshTarget ? "typed_now" : "defer";
  }

  const lines = [
    `export SURVEY_CLIENT_NETWORK=${shellSingleQuote("unknown")}`,
    `export SURVEY_CLIENT_SERVER_READY=${shellSingleQuote("unknown")}`,
    `export SURVEY_CLIENT_SSH_PORT=${shellSingleQuote(sshPort)}`,
    `export SURVEY_CLIENT_SSH_CUSTOM_PORT=${shellSingleQuote(sshCustom)}`,
    `export SURVEY_CLIENT_HOST_PLAN=${shellSingleQuote(hostPlanOut)}`,
    `export SURVEY_CLIENT_GEN_SSH_KEY=${shellSingleQuote(genSshKey)}`,
  ];
  if (backupSshTarget) {
    lines.push(`export BACKUP_SSH_TARGET=${shellSingleQuote(backupSshTarget)}`);
  }

  writeFileSync(envFile, `${lines.join("\n")}\n`, "utf8");
  p.outro("Thanks — continuing Backr setup…");
}

const ctx = readCli();
await runWizard(ctx).catch((err) => {
  console.error("backr-connecting-survey:", err);
  process.exit(1);
});
