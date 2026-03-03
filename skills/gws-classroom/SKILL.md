---
name: gws-classroom
version: 1.0.0
description: "Google Classroom: Manage classes, rosters, and coursework."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws classroom --help"
---

# classroom (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws classroom <resource> <method> [flags]
```

## API Resources

### courses

  - `create` — Creates a course. The user specified in `ownerId` is the owner of the created course and added as a teacher. A non-admin requesting user can only create a course with themselves as the owner. Domain a
  - `delete` — Deletes a course. This method returns the following error codes: * `PERMISSION_DENIED` if the requesting user is not permitted to delete the requested course or for access errors. * `NOT_FOUND` if no 
  - `get` — Returns a course. This method returns the following error codes: * `PERMISSION_DENIED` if the requesting user is not permitted to access the requested course or for access errors. * `NOT_FOUND` if no 
  - `getGradingPeriodSettings` — Returns the grading period settings in a course. This method returns the following error codes: * `PERMISSION_DENIED` if the requesting user isn't permitted to access the grading period settings in th
  - `list` — Returns a list of courses that the requesting user is permitted to view, restricted to those that match the request. Returned courses are ordered by creation time, with the most recently created comin
  - `patch` — Updates one or more fields in a course. This method returns the following error codes: * `PERMISSION_DENIED` if the requesting user is not permitted to modify the requested course or for access errors
  - `update` — Updates a course. This method returns the following error codes: * `PERMISSION_DENIED` if the requesting user is not permitted to modify the requested course or for access errors. * `NOT_FOUND` if no 
  - `updateGradingPeriodSettings` — Updates grading period settings of a course. Individual grading periods can be added, removed, or modified using this method. The requesting user and course owner must be eligible to modify Grading Pe
  - `aliases` — Operations on the 'aliases' resource
  - `announcements` — Operations on the 'announcements' resource
  - `courseWork` — Operations on the 'courseWork' resource
  - `courseWorkMaterials` — Operations on the 'courseWorkMaterials' resource
  - `posts` — Operations on the 'posts' resource
  - `studentGroups` — Operations on the 'studentGroups' resource
  - `students` — Operations on the 'students' resource
  - `teachers` — Operations on the 'teachers' resource
  - `topics` — Operations on the 'topics' resource

### invitations

  - `accept` — Accepts an invitation, removing it and adding the invited user to the teachers or students (as appropriate) of the specified course. Only the invited user may accept an invitation. This method returns
  - `create` — Creates an invitation. Only one invitation for a user and course may exist at a time. Delete and re-create an invitation to make changes. This method returns the following error codes: * `PERMISSION_D
  - `delete` — Deletes an invitation. This method returns the following error codes: * `PERMISSION_DENIED` if the requesting user is not permitted to delete the requested invitation or for access errors. * `NOT_FOUN
  - `get` — Returns an invitation. This method returns the following error codes: * `PERMISSION_DENIED` if the requesting user is not permitted to view the requested invitation or for access errors. * `NOT_FOUND`
  - `list` — Returns a list of invitations that the requesting user is permitted to view, restricted to those that match the list request. *Note:* At least one of `user_id` or `course_id` must be supplied. Both fi

### registrations

  - `create` — Creates a `Registration`, causing Classroom to start sending notifications from the provided `feed` to the destination provided in `cloudPubSubTopic`. Returns the created `Registration`. Currently, th
  - `delete` — Deletes a `Registration`, causing Classroom to stop sending notifications for that `Registration`.

### userProfiles

  - `get` — Returns a user profile. This method returns the following error codes: * `PERMISSION_DENIED` if the requesting user is not permitted to access this user profile, if no profile exists with the requeste
  - `guardianInvitations` — Operations on the 'guardianInvitations' resource
  - `guardians` — Operations on the 'guardians' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws classroom --help

# Inspect a method's required params, types, and defaults
gws schema classroom.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

