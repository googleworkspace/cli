// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

import { access, constants } from "node:fs/promises";
import { join } from "node:path";

/**
 * Locate an executable on $PATH (pure-Node, no external deps).
 * Returns the full path or null if not found.
 */
export async function which(name: string): Promise<string | null> {
  const dirs = (process.env.PATH ?? "").split(":");
  for (const dir of dirs) {
    const full = join(dir, name);
    try {
      await access(full, constants.X_OK);
      return full;
    } catch {
      // not here, try next
    }
  }
  return null;
}
