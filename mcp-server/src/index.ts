#!/usr/bin/env node

// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

/**
 * MCP Server for Google Workspace CLI (gws)
 *
 * Wraps the `gws` CLI as MCP tools, enabling any MCP-compatible client
 * (Claude Desktop, Cursor, Zed, etc.) to interact with Google Workspace APIs.
 *
 * Tools are generated dynamically from the gws service registry.
 */

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { which } from "./which.js";

const execFileAsync = promisify(execFile);

// ---------------------------------------------------------------------------
// Service registry — mirrors src/services.rs
// ---------------------------------------------------------------------------

interface ServiceDef {
  name: string;
  description: string;
}

const SERVICES: ServiceDef[] = [
  { name: "drive", description: "Manage files, folders, and shared drives" },
  { name: "sheets", description: "Read and write spreadsheets" },
  { name: "gmail", description: "Send, read, and manage email" },
  { name: "calendar", description: "Manage calendars and events" },
  { name: "docs", description: "Read and write Google Docs" },
  { name: "slides", description: "Read and write presentations" },
  { name: "tasks", description: "Manage task lists and tasks" },
  { name: "people", description: "Manage contacts and profiles" },
  { name: "chat", description: "Manage Chat spaces and messages" },
  { name: "classroom", description: "Manage classes, rosters, and coursework" },
  { name: "forms", description: "Read and write Google Forms" },
  { name: "keep", description: "Manage Google Keep notes" },
  { name: "meet", description: "Manage Google Meet conferences" },
  { name: "admin-reports", description: "Audit logs and usage reports" },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Maximum output size (bytes) to prevent unbounded responses. */
const MAX_OUTPUT = 100_000;

function truncate(text: string, max = MAX_OUTPUT): string {
  if (text.length <= max) return text;
  return text.slice(0, max) + `\n... (truncated at ${max} bytes)`;
}

/** Run a gws command and return { stdout, stderr, exitCode }. */
async function runGws(
  args: string[],
  timeoutMs = 30_000,
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const bin = await which("gws");
  if (!bin) {
    return {
      stdout: "",
      stderr:
        "gws binary not found on $PATH. Install: npm i -g @googleworkspace/cli",
      exitCode: 127,
    };
  }

  try {
    const { stdout, stderr } = await execFileAsync(bin, args, {
      timeout: timeoutMs,
      maxBuffer: MAX_OUTPUT * 2,
      env: { ...process.env },
    });
    return { stdout: truncate(stdout), stderr: truncate(stderr), exitCode: 0 };
  } catch (err: unknown) {
    const e = err as {
      stdout?: string;
      stderr?: string;
      code?: number | string;
    };
    return {
      stdout: truncate(e.stdout ?? ""),
      stderr: truncate(e.stderr ?? String(err)),
      exitCode: typeof e.code === "number" ? e.code : 1,
    };
  }
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

const server = new McpServer({
  name: "gws-mcp-server",
  version: "0.1.0",
});

// ── Generic gws_run tool ──────────────────────────────────────────────────
// Accepts arbitrary gws arguments. This is the escape hatch for any command
// not covered by the per-service tools.
server.tool(
  "gws_run",
  "Run any gws CLI command. Pass the full argument list (e.g. ['drive', 'files', 'list', '--params', '{\"pageSize\":5}']). Use this for advanced commands, auth management, or services without a dedicated tool.",
  {
    args: z
      .array(z.string())
      .describe("Arguments to pass to gws (e.g. ['drive', 'files', 'list'])"),
    timeout_ms: z
      .number()
      .optional()
      .describe("Timeout in milliseconds (default 30000)"),
  },
  async ({ args, timeout_ms }) => {
    const result = await runGws(args, timeout_ms);
    const text = [
      result.stdout,
      result.stderr ? `\n--- stderr ---\n${result.stderr}` : "",
      `\n--- exit code: ${result.exitCode} ---`,
    ].join("");
    return {
      content: [{ type: "text" as const, text }],
      isError: result.exitCode !== 0,
    };
  },
);

// ── Per-service tools ─────────────────────────────────────────────────────
// Each service gets a tool like gws_drive, gws_gmail, etc.
for (const svc of SERVICES) {
  const toolName = `gws_${svc.name.replace("-", "_")}`;
  server.tool(
    toolName,
    `Google Workspace — ${svc.description}. Runs: gws ${svc.name} <resource> <method> [flags]. Use gws_schema to discover available resources and methods.`,
    {
      resource: z.string().describe("API resource (e.g. 'files', 'messages', 'events')"),
      method: z.string().describe("API method (e.g. 'list', 'get', 'create', 'delete')"),
      params: z
        .string()
        .optional()
        .describe("JSON string of query/path parameters (passed to --params)"),
      body: z
        .string()
        .optional()
        .describe("JSON string of request body (passed to --json)"),
      extra_args: z
        .array(z.string())
        .optional()
        .describe("Additional CLI flags (e.g. ['--page-all', '--format', 'csv'])"),
      dry_run: z
        .boolean()
        .optional()
        .describe("If true, validate locally without calling the API"),
      timeout_ms: z
        .number()
        .optional()
        .describe("Timeout in milliseconds (default 30000)"),
    },
    async ({ resource, method, params, body, extra_args, dry_run, timeout_ms }) => {
      const args: string[] = [svc.name, resource, method];
      if (params) args.push("--params", params);
      if (body) args.push("--json", body);
      if (dry_run) args.push("--dry-run");
      if (extra_args) args.push(...extra_args);

      const result = await runGws(args, timeout_ms);
      const text = [
        result.stdout,
        result.stderr ? `\n--- stderr ---\n${result.stderr}` : "",
        `\n--- exit code: ${result.exitCode} ---`,
      ].join("");
      return {
        content: [{ type: "text" as const, text }],
        isError: result.exitCode !== 0,
      };
    },
  );
}

// ── Schema introspection tool ─────────────────────────────────────────────
server.tool(
  "gws_schema",
  "Introspect a Google Workspace API method schema. Returns parameters, request/response shapes. Path format: service.resource.method (e.g. 'drive.files.list').",
  {
    path: z
      .string()
      .describe("Dotted schema path: service.resource.method (e.g. 'drive.files.list', 'gmail.users.messages.get')"),
    resolve_refs: z
      .boolean()
      .optional()
      .describe("If true, inline $ref schemas for a self-contained view"),
  },
  async ({ path, resolve_refs }) => {
    const args = ["schema", path];
    if (resolve_refs) args.push("--resolve-refs");
    const result = await runGws(args);
    const text = [
      result.stdout,
      result.stderr ? `\n--- stderr ---\n${result.stderr}` : "",
    ].join("");
    return {
      content: [{ type: "text" as const, text }],
      isError: result.exitCode !== 0,
    };
  },
);

// ── Auth status tool ──────────────────────────────────────────────────────
server.tool(
  "gws_auth_status",
  "Check gws authentication status. Returns current login state and available scopes.",
  {},
  async () => {
    const result = await runGws(["auth", "status"]);
    const text = [result.stdout, result.stderr].filter(Boolean).join("\n");
    return {
      content: [{ type: "text" as const, text }],
      isError: result.exitCode !== 0,
    };
  },
);

// ── Services list resource ────────────────────────────────────────────────
server.resource(
  "services",
  "gws://services",
  async (uri) => {
    const text = SERVICES.map(
      (s) => `- **${s.name}**: ${s.description}`,
    ).join("\n");
    return {
      contents: [
        {
          uri: uri.href,
          mimeType: "text/markdown",
          text: `# Available Google Workspace Services\n\n${text}\n\nUse \`gws_schema\` to discover resources and methods for each service.`,
        },
      ],
    };
  },
);

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error("gws MCP server running on stdio");
}

main().catch((err) => {
  console.error("Fatal:", err);
  process.exit(1);
});
