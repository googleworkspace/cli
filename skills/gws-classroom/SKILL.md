---
name: gws-classroom
version: 1.0.0
description: "Google Classroom skill for the gws CLI. Manages courses, student rosters, assignments, course materials, announcements, grades, topics, and invitations via the Classroom API. Use when the user mentions Google Classroom, creating or managing courses, enrolling or removing students, creating assignments or course materials, posting announcements, managing grading periods, or interacting with the classroom API or LMS."
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

  - `create` — Creates a course. The `ownerId` user becomes the owner and is added as a teacher. Non-admin users can only create courses they own; domain admins can assign any owner.
  - `delete` — Deletes a course by ID.
  - `get` — Returns a single course by ID.
  - `getGradingPeriodSettings` — Returns grading period settings for a course.
  - `list` — Returns courses the requesting user can view, ordered by creation time (newest first).
  - `patch` — Updates one or more fields in a course.
  - `update` — Fully updates a course.
  - `updateGradingPeriodSettings` — Adds, removes, or modifies individual grading periods. Requires eligibility; see [licensing requirements](https://developers.google.com/workspace/classroom/grading-periods/manage-grading-periods#licensing_requirements).
  - `aliases` — Operations on the 'aliases' resource
  - `announcements` — Operations on the 'announcements' resource
  - `courseWork` — Operations on the 'courseWork' resource (assignments, quizzes)
  - `courseWorkMaterials` — Operations on the 'courseWorkMaterials' resource
  - `posts` — Operations on the 'posts' resource
  - `studentGroups` — Operations on the 'studentGroups' resource
  - `students` — Operations on the 'students' resource
  - `teachers` — Operations on the 'teachers' resource
  - `topics` — Operations on the 'topics' resource

### invitations

  - `accept` — Accepts an invitation; adds the invited user as teacher or student. Only the invited user may accept.
  - `create` — Creates an invitation. Only one invitation per user/course may exist at a time; delete and recreate to change it.
  - `delete` — Deletes an invitation by ID.
  - `get` — Returns an invitation by ID.
  - `list` — Returns invitations the user may view. At least one of `user_id` or `course_id` must be supplied.

### registrations

  - `create` — Creates a `Registration`, causing Classroom to start sending notifications from the provided feed to a Cloud Pub/Sub topic.
  - `delete` — Deletes a `Registration`, stopping notifications for that registration.

### userProfiles

  - `get` — Returns a user profile by ID.
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

## Examples

```bash
# Create a new course owned by a specific teacher
gws classroom courses create --json '{"name":"Biology 101","ownerId":"teacher@school.edu","courseState":"ACTIVE"}'

# List all courses for the authenticated user
gws classroom courses list

# Enroll a student in a course
gws classroom courses students create --params 'courseId=abc123' --json '{"userId":"student@school.edu"}'

# Create an assignment in a course
gws classroom courses courseWork create --params 'courseId=abc123' \
  --json '{"title":"Chapter 5 Essay","workType":"ASSIGNMENT","state":"PUBLISHED","dueDate":{"year":2025,"month":6,"day":30}}'

# Post an announcement to a course
gws classroom courses announcements create --params 'courseId=abc123' \
  --json '{"text":"Class is cancelled Friday.","state":"PUBLISHED"}'

# Invite a teacher to a course
gws classroom invitations create --json '{"courseId":"abc123","userId":"newteacher@school.edu","role":"TEACHER"}'
```

## Common Workflows

### Set Up a New Class with Students and an Assignment

```bash
# 1. Create the course
gws classroom courses create \
  --json '{"name":"History 201","ownerId":"teacher@school.edu","courseState":"ACTIVE"}'
# → note the returned course `id` (e.g. "xyz789")

# 2. Verify the course was created successfully before proceeding
gws classroom courses get --params 'courseId=xyz789'
# → confirm `courseState` is "ACTIVE" and `name` is correct

# 3. Enroll students (repeat per student)
gws classroom courses students create --params 'courseId=xyz789' \
  --json '{"userId":"student1@school.edu"}'

# 4. Verify student enrollment succeeded
gws classroom courses students get --params 'courseId=xyz789&userId=student1@school.edu'
# → confirm the student profile is returned before continuing with additional enrollments

# 5. Create a topic to organise work
gws classroom courses topics create --params 'courseId=xyz789' \
  --json '{"name":"Unit 1: Foundations"}'

# 6. Create an assignment under that topic
gws classroom courses courseWork create --params 'courseId=xyz789' \
  --json '{"title":"Unit 1 Essay","workType":"ASSIGNMENT","state":"PUBLISHED","topicId":"<topicId>"}'
```

### Add a Teacher via Invitation

```bash
# 1. Create the invitation
gws classroom invitations create \
  --json '{"courseId":"xyz789","userId":"coteacher@school.edu","role":"TEACHER"}'
# → note the returned invitation `id`

# 2. Verify the invitation exists before asking the user to accept
gws classroom invitations get --params 'id=<invitationId>'

# 3. The invited user accepts (must be authenticated as that user)
gws classroom invitations accept --params 'id=<invitationId>'
```
