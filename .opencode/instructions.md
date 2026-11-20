# GitHub Backlog Auto-Logging Rule

## Triggers
You MUST automatically create a GitHub backlog card using the GitHub MCP tools if:
1. You leave a task incomplete, partially done, or blocked.
2. You skip, stub, mock, or hardcode an implementation instead of finishing it.
3. You forget to implement a required feature, test, or edge case.
4. The user says "we should", "later", "TODO", or suggests a future improvement.

## Required Action
1. **Check first:** Use GitHub MCP to search existing issues. If it exists, comment on it instead of duplicating.
2. **Create Issue:** Use GitHub MCP `create_issue` with this format:
   - **Title:** `[type]: <short summary>` (e.g., `fix: handle timeout in API client`)
   - **Body:** Include Context, Missing Implementation, File Location, and Acceptance Criteria.
3. **Add to Project:** Use GitHub MCP to add the new issue to the Project Board and set its Status to **Backlog**.
4. **Notify:** Briefly tell the user: "Logged to backlog: [Issue URL]".

## Constraints
- Never leave a `// TODO` in code without creating the corresponding GitHub card.
- Do not ask for permission to create the card; just do it and report the URL.
- If the GitHub MCP is unavailable, append the task to a local `BACKLOG.md` file instead.

## GitHub Project

All backlog cards, bugs, and follow-up tasks belong to project **EAT**:

- URL: https://github.com/users/madjsmail/projects/17
- Project number: `17`
- Project node ID: `PVT_kwHOAm22Fs4BsZjp`
- Repository: `madjsmail/EAT-BAVCEND`

When creating a backlog card, add the issue to this project and set its
Status to **Backlog**. The GitHub MCP issue tools do not expose Projects v2,
so use the GraphQL API (`addProjectV2ItemById`) or the GitHub UI after
creating the issue. Do not create a separate project or a local-only backlog
unless GitHub is unreachable.

Project field IDs:

- Status field: `PVTSSF_lAHOAm22Fs4BsZjpzhqn4tI`
- Backlog option: `f75ad846`

## Scope

Create cards only while working inside:

- `/home/madjidsmail/Projects/asset tracking/backend`
- `/home/madjidsmail/Projects/asset tracking/enterprise-at`

Never create project cards from any other directory or repository.
