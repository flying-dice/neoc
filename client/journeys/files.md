# Files: key user journeys

A workspace file browser. **Placeholder page:** the locations and recents are fixed sample data, nothing opens when a row
is tapped, and the "More" button does nothing yet; the journeys describe what the page does today. Start at the host's
address in a paired browser with Files open.

Automated twins (same ids): `e2e/files.journeys.spec.ts`.

| Id  | Who / goal                                         | Starts at  | Done when                                                                                         |
| --- | -------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------------- |
| F1  | Find the refactor plan that the Coder agent wrote. | Files open | They located `refactor-plan.md` and know which agent made it, and when.                           |
| F2  | Search for a file by part of its name.             | Files open | Searching narrowed the list to the right file; clearing the search restored it.                   |
| F3  | Find where deleted files go.                       | Files open | They found Recently Deleted.                                                                      |
| F4  | Browse files in a small window.                    | Files open | At the smallest window size every row is readable, with long names truncated and nothing cut off. |
