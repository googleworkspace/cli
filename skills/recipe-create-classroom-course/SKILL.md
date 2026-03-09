---
name: recipe-create-classroom-course
version: 1.0.0
description: "Creates a Google Classroom course and invites students using the gws CLI. Use when a user wants to set up a class, create a new Google Classroom course, enroll or add students to a course, configure classroom settings, or perform LMS setup tasks. Trigger terms include: 'Google Classroom', 'create class', 'add students to course', 'enroll students', 'classroom setup', 'new classroom course', 'invite students'."
metadata:
  openclaw:
    category: "recipe"
    domain: "education"
    requires:
      bins: ["gws"]
      skills: ["gws-classroom"]
---

# Create a Google Classroom Course

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-classroom`

Create a Google Classroom course and invite students.

## Steps

1. Create the course: `gws classroom courses create --json '{"name": "Introduction to CS", "section": "Period 1", "room": "Room 101", "ownerId": "me"}'`
2. **Validate course creation:** Capture the `id` field from the response JSON and use it as `COURSE_ID` in subsequent steps. If no `id` is returned, do not proceed — report the error to the user before continuing.
3. Invite a student: `gws classroom invitations create --json '{"courseId": "COURSE_ID", "userId": "student@school.edu", "role": "STUDENT"}'`
   - If the invitation fails, check for common errors: invalid or non-existent student email, student already enrolled (duplicate invitation), or insufficient permissions on the course.
4. List enrolled students: `gws classroom courses students list --params '{"courseId": "COURSE_ID"}' --format table`
