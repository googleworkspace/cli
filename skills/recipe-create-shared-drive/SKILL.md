---
name: recipe-create-shared-drive
version: 1.0.0
description: "Creates a Google Shared Drive and adds members with appropriate roles using the gws CLI. Use when a user wants to create a shared drive, team drive, or shared folder in Google Drive, add collaborators or members to a drive, manage Google Drive permissions or access, set up a drive for a team or project, or grant user roles like writer or organizer to a shared drive."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Create and Configure a Shared Drive

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

Create a Google Shared Drive and add members with appropriate roles.

## Steps

1. Create shared drive: `gws drive drives create --params '{"requestId": "unique-id-123"}' --json '{"name": "Project X"}'`
2. Verify the drive was created and capture the `DRIVE_ID` from the response before proceeding. If no `id` field is returned, stop and report the error to the user.
3. Add a member: `gws drive permissions create --params '{"fileId": "DRIVE_ID", "supportsAllDrives": true}' --json '{"role": "writer", "type": "user", "emailAddress": "member@company.com"}'`
4. List members to confirm the permission was applied: `gws drive permissions list --params '{"fileId": "DRIVE_ID", "supportsAllDrives": true}'`

## Error Handling

- **Drive creation fails:** Check that `requestId` is unique and that the authenticated account has permission to create Shared Drives in the domain.
- **Invalid email address:** The permissions create command will return an error if the email address does not correspond to a valid Google account. Confirm the address with the user and retry.
- **Insufficient admin permissions:** If the account lacks the necessary privileges to create drives or assign roles, escalate to a Google Workspace admin.
