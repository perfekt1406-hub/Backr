#!/usr/bin/env node
/**
 * Backr — backup-host setup questionnaire (@clack/prompts).
 *
 * Purpose: Collect two setup choices in the same terminal UX as the laptop wizard.
 * Role: Writes export lines for bash (`setup-backup-host.sh` sources the --env-file).
 *
 * External: @clack/prompts — terminal prompts (inputs: messages/options; outputs: selections / cancel).
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
    "export SURVEY_REACH=unknown",
    "export SURVEY_KEYPATH=unknown",
    "export SURVEY_DEPLOYMENT=unknown",
    "export SURVEY_SSH_PORT=unknown",
    "export SURVEY_SSH_CUSTOM_PORT=",
    "export SURVEY_PLATFORM=unknown",
  ];
  writeFileSync(envFile, `${lines.join("\n")}\n`, "utf8");
}

/**
 * Parses argv for --env-file and --backr-user.
 *
 * @returns {{ envFile: string, backrUser: string }}
 */
function readCli() {
  const { values } = parseArgs({
    options: {
      "env-file": { type: "string" },
      "backr-user": { type: "string", default: "backr" },
    },
  });
  const envFile = values["env-file"];
  if (!envFile) {
    console.error("error: --env-file PATH is required");
    process.exit(2);
  }
  return {
    envFile,
    backrUser: (values["backr-user"] ?? "backr").trim() || "backr",
  };
}

/**
 * Runs the host questionnaire and writes export lines for bash.
 *
 * @param {{ envFile: string, backrUser: string }} ctx Parsed CLI context.
 */
async function runWizard(ctx) {
  const { envFile, backrUser } = ctx;

  p.intro("◆ Backr — backup host setup");

  p.note(
    "SSH «public key» = one line from the laptop's ~/.ssh/id_ed25519.pub (safe to share). Trust keys = Backr screen on this machine (#/host/trust). authorized_keys = server file listing allowed keys for login.",
    "SSH in one sentence",
  );

  const reach = await p.select({
    message: "How will backup laptops reach SSH on this machine?",
    options: [
      { value: "lan_only", label: "Same LAN only (private IPs)" },
      { value: "internet", label: "Over the internet (public IP, DDNS, port forward)" },
      { value: "vpn", label: "VPN to this network first" },
      { value: "unknown", label: "I'll set it up myself" },
    ],
  });
  if (p.isCancel(reach)) {
    writeCancelledDefaults(envFile);
    process.exit(0);
  }

  const keypath = await p.select({
    message: `How will each laptop's public key get into ${backrUser}'s authorized_keys?`,
    options: [
      { value: "backr_trust_ui", label: "Backr on this machine → Trust keys (#/host/trust)" },
      { value: "console_later", label: "SSH or console — edit ~/.ssh/authorized_keys manually" },
      { value: "other_admin", label: "Someone else administers SSH here" },
      { value: "unknown", label: "I'll set it up myself" },
    ],
  });
  if (p.isCancel(keypath)) {
    writeCancelledDefaults(envFile);
    process.exit(0);
  }

  const lines = [
    `export SURVEY_REACH=${shellSingleQuote(String(reach))}`,
    `export SURVEY_KEYPATH=${shellSingleQuote(String(keypath))}`,
    `export SURVEY_DEPLOYMENT=${shellSingleQuote("unknown")}`,
    `export SURVEY_SSH_PORT=${shellSingleQuote("unknown")}`,
    `export SURVEY_SSH_CUSTOM_PORT=${shellSingleQuote("")}`,
    `export SURVEY_PLATFORM=${shellSingleQuote("unknown")}`,
  ];
  writeFileSync(envFile, `${lines.join("\n")}\n`, "utf8");
  p.outro("Thanks — continuing Backr setup…");
}

const ctx = readCli();
await runWizard(ctx).catch((err) => {
  console.error("backr-host-survey:", err);
  process.exit(1);
});
