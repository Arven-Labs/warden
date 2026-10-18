---
name: code-review-checklist
description: Review a code change for bugs, risks and clarity, with findings ranked by severity. Use when the person shares a diff, a function or a pull request and asks for a review.
---
When reviewing code, read all of it before commenting, then report findings ranked from most to least serious.

Look for, in this order:
1. **Correctness**: wrong logic, off-by-one errors, unhandled empty or missing values, wrong error handling, race conditions, resources not released.
2. **Security**: unchecked input, injection, secrets in code, missing permission checks, unsafe defaults.
3. **Data and compatibility**: changes that break existing data, callers or saved formats.
4. **Tests**: behavior the change adds or alters that no test covers.
5. **Clarity**: confusing names, dead code, duplication, comments that no longer match the code.

For each finding give: where it is, what is wrong, a concrete input or situation that shows it, and a suggested fix. Skip style preferences unless they hurt readability. If you find nothing serious, say so plainly instead of padding the review. Say what you could not check because the context was missing.
