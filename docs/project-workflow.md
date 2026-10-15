# GitHub Project workflow

## Model

Create a **private owner-level GitHub Project (Projects v2)** and link it to the private repository. A Project is not created inside the repository; linking makes it accessible from the repository's Projects tab. Repository and Project visibility are managed separately.

The provisioning bundle creates Issues (not draft Project notes), six milestones, labels, and the following Project fields:

| Field | Values / purpose |
| --- | --- |
| Workflow | Backlog, Ready, In Progress, In Review, Blocked, Done |
| Priority | P0, P1, P2 |
| Area | api, ci, cli, installer, oss, registry, release, research, resolver, security, sources, testing, tooling, validation |
| Size | S, M, L (relative review scope, not elapsed-time estimates) |
| Plan ID | stable ASK-### key |
| Dependencies | actual #issue references after provisioning |

Use **Workflow** as the plan's authoritative progress field. The GitHub-provided `Status` field is left intact; the script does not delete or rewrite built-in settings. Default Project views remain usable as a table. A Board grouped by Workflow and a release view filtered by milestone can be configured in the UI; saved custom views are not created by the script.

## Initial state

Dependency-free work starts in Ready. Work with dependencies starts in Backlog. M5 starts in Backlog regardless. Backlog is not the same as an active blocker; use Blocked when a specific discovered issue prevents otherwise-ready work.

Dependencies are explicit, linked references in issue bodies and the Project Dependencies field. They are not claimed to be native GitHub blocked-by relationships. Live issue/PR state is authoritative. Closing a dependency does not automatically promote another item: a maintainer or local agent verifies the remaining prerequisites and moves it to Ready.

## Flow

Ready -> In Progress -> In Review -> Done. Use Blocked with a written reason; return it to Ready only after the blocker is resolved. Done requires merged work and satisfied acceptance criteria. Future ideas stay in Backlog.

Use one active implementer per issue. Keep PRs small; split large adapter waves into host-sized PRs when necessary. Prefer distinct branches/worktrees for concurrent agents.

## Bootstrap behavior

The importer uses stable hidden plan markers to avoid duplicate Issues. It preserves human-edited issue bodies outside its dependency block, existing labels/milestones, and existing Project field values. It refuses unmarked existing repositories and any public repository/Project. It does not change visibility to public, invite collaborators, merge PRs, or publish artifacts.

Local bootstrap state and a generated report are kept outside the repository payload. They contain IDs/URLs, not tokens. Do not commit account credentials or bootstrap state.

## Sources

- https://docs.github.com/en/issues/planning-and-tracking-with-projects/learning-about-projects/about-projects
- https://docs.github.com/en/issues/planning-and-tracking-with-projects/automating-your-project/using-the-api-to-manage-projects
- https://cli.github.com/manual/gh_project_link
