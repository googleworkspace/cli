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
 * Browser automation for Google Docs operations that the API cannot perform.
 *
 * The Google Docs API v1 has no support for "Suggesting" mode — edits made via
 * the API are always direct writes. This script uses Playwright to drive the
 * Docs UI in a headless browser, switching to Suggesting mode before performing
 * a Find & Replace so the change appears as a tracked suggestion.
 *
 * Usage:
 *   node playwright-suggest.mjs <action> [args...]
 *
 * Actions:
 *   suggest <doc_id> <find> <replace> <state_file>
 *
 * Requires:
 *   - npx playwright install chromium   (one-time browser download)
 *   - A saved browser state JSON file with valid Google session cookies
 *     (obtain by running `npx playwright codegen --save-storage=state.json docs.google.com`)
 *
 * Outputs JSON to stdout: { "ok": true, "message": "..." } or { "ok": false, "error": "..." }
 */

import { chromium } from "playwright";
import { platform } from "node:os";

const DISMISS_LABELS = [
  "got it", "ok", "okay", "dismiss", "close", "no thanks",
  "i understand", "not now", "skip", "next time", "maybe later", "continue",
];

function output(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

async function dismissPopups(page) {
  await page.evaluate((labels) => {
    for (const sel of ['[role="dialog"]', '[role="alertdialog"]', '[class*="Dialog"]']) {
      for (const dialog of document.querySelectorAll(sel)) {
        const buttons = dialog.querySelectorAll('button, [role="button"]');
        for (const btn of buttons) {
          if (labels.includes(btn.textContent.trim().toLowerCase())) {
            btn.click();
            return;
          }
        }
        if (buttons.length === 1) { buttons[0].click(); return; }
      }
    }
  }, DISMISS_LABELS);
}

async function openDoc(browser, stateFile, docId) {
  const context = await browser.newContext({ storageState: stateFile });
  const page = await context.newPage();
  await page.goto(`https://docs.google.com/document/d/${docId}/edit`, {
    waitUntil: "domcontentloaded",
  });
  await page.waitForSelector(".kix-appview-editor", { timeout: 30_000 });
  for (let i = 0; i < 3; i++) {
    await page.waitForTimeout(2000);
    await dismissPopups(page);
  }
  return { context, page };
}

/** Click a button in a dialog by its text. Returns true if found and clicked. */
async function clickDialogButton(page, text) {
  return await page.evaluate((text) => {
    for (const sel of ['[role="dialog"]', '[role="alertdialog"]']) {
      for (const dialog of document.querySelectorAll(sel)) {
        for (const btn of dialog.querySelectorAll('button, [role="button"]')) {
          if (btn.textContent.trim() === text) { btn.click(); return true; }
        }
      }
    }
    return false;
  }, text);
}

async function suggest(docId, find, replace, stateFile) {
  const browser = await chromium.launch({ headless: true });
  try {
    const { context, page } = await openDoc(browser, stateFile, docId);
    await page.locator(".kix-appview-editor").click();
    await page.waitForTimeout(500);

    // Switch to Suggesting mode
    let modeBtn = page.locator("#docs-toolbar-mode-switcher").first();
    if (!(await modeBtn.isVisible({ timeout: 3000 }).catch(() => false))) {
      modeBtn = page.locator("[aria-label='Editing mode']").first();
    }
    await modeBtn.click();
    await page.waitForTimeout(500);
    await page.getByText("Suggesting", { exact: true }).click();
    await page.waitForTimeout(500);

    // Open Find & Replace (platform-aware shortcut)
    await page.keyboard.press(platform() === "darwin" ? "Meta+Shift+h" : "Control+h");
    await page.waitForTimeout(2000);

    // Fill in find/replace fields
    await page.evaluate(() => {
      const inputs = document.querySelectorAll('[role="dialog"] input');
      if (inputs[0]) inputs[0].focus();
    });
    await page.waitForTimeout(300);
    await page.keyboard.type(find, { delay: 10 });
    await page.waitForTimeout(500);
    await page.keyboard.press("Tab");
    await page.waitForTimeout(300);
    await page.keyboard.type(replace, { delay: 10 });
    await page.waitForTimeout(500);

    // Navigate to first match
    if (!(await clickDialogButton(page, "Next"))) {
      await page.keyboard.press("Escape");
      await context.storageState({ path: stateFile }).catch(() => {});
      return { ok: false, error: 'Could not find "Next" button. The Google Docs UI may be in a non-English language.' };
    }
    await page.waitForTimeout(1000);

    // Validate match count
    const matchInfo = await page.evaluate(() => {
      const dialog = document.querySelector('[role="dialog"]');
      if (!dialog) return { status: "unknown" };
      const text = dialog.textContent || "";
      if (text.includes("not found") || text.includes("No results"))
        return { status: "not_found" };
      const m = text.match(/(\d+)\s+of\s+(\d+)/);
      if (m) return { status: "found", current: +m[1], total: +m[2] };
      return { status: "unknown" };
    });

    if (matchInfo.status === "not_found") {
      await page.keyboard.press("Escape");
      await context.storageState({ path: stateFile }).catch(() => {});
      return { ok: false, error: `No match found for "${find}".` };
    }

    if (matchInfo.status === "found" && matchInfo.total > 1) {
      await page.keyboard.press("Escape");
      await context.storageState({ path: stateFile }).catch(() => {});
      return {
        ok: false,
        error: `${matchInfo.total} matches found for "${find}". Use a longer, unique quote.`,
      };
    }

    if (matchInfo.status === "unknown") {
      await page.keyboard.press("Escape");
      await context.storageState({ path: stateFile }).catch(() => {});
      return { ok: false, error: `Could not verify match count for "${find}". The Google Docs UI may be in a non-English language.` };
    }

    // Execute the replacement (recorded as a suggestion)
    if (!(await clickDialogButton(page, "Replace"))) {
      await page.keyboard.press("Escape");
      await context.storageState({ path: stateFile }).catch(() => {});
      return { ok: false, error: 'Could not find "Replace" button. The Google Docs UI may be in a non-English language.' };
    }
    await page.waitForTimeout(2000);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);

    await context.storageState({ path: stateFile }).catch(() => {});
    return { ok: true, message: `Suggested: "${find}" → "${replace}"` };
  } finally {
    await browser.close();
  }
}

// --- CLI entry point ---
const [, , action, ...args] = process.argv;

if (action === "suggest" && args.length === 4) {
  const [docId, find, replace, stateFile] = args;
  suggest(docId, find, replace, stateFile)
    .then((r) => output(r))
    .catch((e) => output({ ok: false, error: e.message }));
} else {
  output({ ok: false, error: `Usage: playwright-suggest.mjs suggest <doc_id> <find> <replace> <state_file>` });
  process.exit(1);
}
