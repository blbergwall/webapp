# /change — Development Change Process

Execute the structured development change process. Follow each step sequentially, updating the tracking file at each stage before moving on.

---

## Step 1: Locate and Read the Tracking File

List all files in `docs/active-change/`, ignoring any `.gitkeep` file.

- If there are **zero** files: stop and tell the user "No tracking file found in docs/active-change/. Please create one before running /change."
- If there are **more than one** file: stop and tell the user "Multiple files found in docs/active-change/. There must be exactly one."

Read the file. Check whether it contains a `**Status:**` line.

- If it **does**, skip to the step that matches the status:
  - `initialized` → Step 3
  - `clarified` → Step 4
  - `planned` → Step 5
  - `developed` → Step 6
  - `verified` → Step 7
- If it **does not**, continue to Step 2.

---

## Step 2: Clean Up Tracking File

Fix all spelling and grammar errors in the tracking file. Then insert the status line immediately after the first heading line so the file structure is:

```
# Heading Title

**Status:** initialized

(remaining content)
```

Continue to Step 3.

---

## Step 3: Clarify Intent

Read the description in the tracking file. Consider it in the context of this project (see CLAUDE.md for project context).

- If anything is ambiguous, ask the user to clarify before proceeding.
- Use your judgment about whether the change is too large for a single tracking file. Think about scope, complexity, and whether there is a natural seam to split on. A change that spans multiple unrelated areas or has distinct deliverables is a good candidate for splitting. If you recommend splitting, explain why and suggest how, then stop — the user will create separate tracking files.

Update the tracking file with any clarifications that were decided. When resolving a question or issue that was written inline in the tracking file, **keep the original question text** and insert a `**Decision:**` line immediately below it with the resolution. Do not remove or replace the question — the question provides context for the decision. Change status to `clarified`.

Continue to Step 4.

---

## Step 4: Create Implementation Plan

Add an `## Implementation Plan` section to the tracking file. Use the template below and omit any sections that do not apply to this specific change:

```markdown
## Implementation Plan

**Files to modify:**
- `path/to/file` — what changes and why

**Files to add:**
- `path/to/file` — what it does

**Files to remove:**
- `path/to/file` — why

**Approach:**
Overall strategy and key decisions.

**Risks / things to watch for:**
Non-obvious constraints or potential issues.
```

Change status to `planned`.

Continue to Step 5.

---

## Step 5: Implement

Make all code changes per the implementation plan. If the plan needs to change mid-implementation, update the `## Implementation Plan` section in the tracking file before continuing.

When all changes are made, if the project has a build step, verify the build using the commands in the `## Build & Run` section of `CLAUDE.md`. If the project has no build step (e.g. a static website), silently skip this.

Change status to `developed`.

Continue to Step 6.

---

## Step 6: User Testing and Verification

Ask the user to test and verify the change. Record any adjustments the user requests in the tracking file under a `## User Feedback` section. Make those changes, then ask for re-verification if needed.

When the user is satisfied, change status to `verified`.

Continue to Step 7.

---

## Step 7: Cleanup and Commit

1. Change status to `done`.
2. Move the tracking file from `docs/active-change/` to `docs/completed-changes/`.
3. If git has been initialized in the current working directory, stage all changed files and commit with `git commit`. Keep the message short and to the point — one line is preferred unless the change genuinely warrants more detail. If git has not been initialized, silently skip this step.
