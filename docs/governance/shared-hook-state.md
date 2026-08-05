# Shared hook state authority

shared-hook-state.v1.json is the repository-owned, machine-readable contract
for exactly these common-Git-dir targets:

- .git/hooks/pre-commit
- .git/hooks/pre-push
- .git/info/lefthook.checksum

The current contract is intentionally PROPOSED. It does not choose
absent or present, and therefore is not an authority for restoration.

## Contract rules

The contract has schema shared_hook_state_authority_v1 and version 1.
targets must contain exactly the three names and canonical relative paths
above, without duplicates or extra entries. A target expectation is either:

- {"state":"absent"}; or
- {"state":"present","sha256":"<64 lowercase hex>","mode":"0xyz","artifact":{"encoding":"base64","content":"..."}}.

Present artifacts are bounded to 64 KiB and their decoded bytes must hash to
the declared SHA-256. Modes contain four octal digits, start with 0, and
do not contain special permission bits. The verifier rejects unknown fields,
duplicate JSON keys, partial expectations, symlinks, traversal/control
characters, arbitrary paths, invalid hashes/modes, and target-set drift.

Only authority.status = "DECIDED" with a non-empty decisionId, complete
exact expectations, and mutationAllowed = false can authorize a verification
PASS. UNDECIDED and PROPOSED are valid non-authoritative contract states;
the verifier reports them and blocks with exit code 20.

## Read-only verification

From the repository or any directory inside it, run:

~~~text
gws shared-hook-state
~~~

The command resolves the repository root and common Git directory using
read-only git rev-parse calls. It accepts no root, contract, Git-directory,
or target path arguments. It never runs package managers, lifecycle scripts,
Lefthook installation, or mutation/restoration code. The output is structured
JSON containing authority status, contract status, observed and expected
values for every target, bounded drift, and failClosed.

Stable exit codes are:

| Code | Meaning |
| ---: | --- |
| 0 | Exact decided state observed |
| 20 | Authority is UNDECIDED or PROPOSED |
| 21 | Contract missing or invalid |
| 22 | Common Git directory cannot be safely resolved |
| 23 | Decided target drift |
| 24 | Target observation failed closed |
| 25 | CLI arguments are outside the fixed interface |

## Future restoration handoff

R2 does not restore anything. A separately authorized future task must:

1. Receive an exact decision for all three targets; if the decision is absent,
   leave this contract UNDECIDED/PROPOSED and stop.
2. Update the repository-owned contract to DECIDED, freeze the manifest
   bytes and every present artifact, and verify the artifact hashes before any
   shared-Git-dir mutation.
3. Keep the mutation/restoration operation separate from this verifier. The
   operation must use only the exact canonical targets, preserve a preimage
   receipt, and perform a readback through gws shared-hook-state.
4. Treat any invalid, stale, partial, symlinked, or mismatched receipt as a
   fail-closed stop. Do not substitute a guessed absent/present state or
   run pnpm, npm, yarn, npx, or an install command to obtain evidence.

Green local tests, a valid proposal, or a zero-drift observation do not by
themselves constitute human acceptance, restoration authorization, delivery,
or a live hook effect.
