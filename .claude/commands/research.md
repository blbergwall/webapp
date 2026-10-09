# /research — Research Process

Execute the structured research process. Follow each step sequentially, updating the tracking file at each stage before moving on.

---

## Step 1: Locate and Read the Tracking File

List all files in `docs/active-research/`, ignoring any `.gitkeep` file.

- If there are **zero** files: stop and tell the user "No tracking file found in docs/active-research/. Please create one before running /research."
- If there are **more than one** file: stop and tell the user "Multiple files found in docs/active-research/. There must be exactly one."

Read the file. Check whether it contains a `**Status:**` line.

- If it **does**, skip to the step that matches the status:
  - `initialized` → Step 3
  - `clarified` → Step 4
  - `researched` → Step 5
  - `verified` → Step 6
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

Update the tracking file with any clarifications that were decided. When resolving a question or issue that was written inline in the tracking file, **keep the original question text** and insert a `**Decision:**` line immediately below it with the resolution. Do not remove or replace the question — the question provides context for the decision. Change status to `clarified`.

Continue to Step 4.

---

## Step 4: Do Research

Carry out the research described in the tracking file. Interact with the user to decide what direction to go when needed. Document findings clearly in the tracking file as you go — include what was found, what was considered and rejected, and any open questions that arise.

Do not make any code changes to the project during this step.

Change status to `researched`.

Continue to Step 5.

---

## Step 5: User Verification

Ask the user to read over the research results. Invite follow-up questions and document both the questions and answers in the tracking file under a `## Follow-Up` section. Make any corrections or additions the user requests.

When the user is satisfied, change status to `verified`.

Continue to Step 6.

---

## Step 6: Create Change or Follow-On Research Tracking File(s) if Desired

Ask the user what they'd like to do next, offering three options:

1. **Research is done** — nothing further to create.
2. **Create a change tracking file** — this research led to a decision to make a project change.
3. **Create a follow-on research tracking file** — this research raised a new question that warrants its own research process.

Handle the choice:

- If **option 1**: proceed to Step 7.
- If **option 2**: create a change tracking file in `docs/backlog-changes/` with name format `yyyymmdd-change-description-text.md`. The file should contain a heading and a short description of the decided change so it is ready to hand off to the `/change` skill. Then re-ask the same three options.
- If **option 3**: create a research tracking file in `docs/backlog-research/` with name format `yyyymmdd-research-description-text.md`. The file should contain a heading and a short description of the new question to research, ready to be picked up by a future `/research` run. Then re-ask the same three options.

Repeat this step, creating as many change and/or research tracking files as the user wants, until option 1 is selected.

**Note:** during this research process no code changes should be made to the project — only changes to the tracking document, possibly creating change tracking document(s) in `docs/backlog-changes/`, supporting research documents in `docs/research/`, and follow-on research tracking document(s) in `docs/backlog-research/` if asked to.

Continue to Step 7.

---

## Step 7: Cleanup and Commit

1. Change status to `done`.
2. Move the tracking file from `docs/active-research/` to `docs/completed-research/`.
3. If git has been initialized in the current working directory, stage all changed files and commit with `git commit`. Keep the message short and to the point — one line is preferred unless the research genuinely warrants more detail. If git has not been initialized, silently skip this step.
