# App frames: key user journeys

An app can draw its own screens in the web framework its developer knows (Svelte, React, plain JavaScript), on a
backend written in Luau. Its window is a frame of the shell that loads the app's client; the client calls its
backend (`rpc`, typed from the app's one RPC definition), hears what the backend sends (`host.on`), and reaches nothing else. The journeys use the two
framework fixtures, `tasks-svelte` and `tasks-react`: the same task list written once in each framework, Vite
projects on one RPC definition (`e2e/apps`; they are built for the tests and are not in Discover). Every journey runs against both.

Automated twins have the same ids: `e2e/appframes.journeys.spec.ts` (the two apps share their data, so they run one
after another).

| Id  | Who / goal                                                     | Starts at                          | Done when                                                                                                                                  |
| --- | -------------------------------------------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| AF1 | Install an app that has a client of its own and open it.       | A host that does not have the app. | The app is in the Dock; opened, its window shows the app drawn by its client.                                                              |
| AF2 | Add a task and see it in the list.                             | The app open.                      | The task is listed and the field is empty; after a reload of the page the task is still there (it is the backend's data); it ticks off.   |
| AF3 | Add a task with no title.                                      | The app open.                      | A message from the backend says what is wrong; the window stays live, and the next add works and clears the message.                       |
| AF4 | Let an agent add a task while the window is open.              | The app open, an agent connected.  | The task appears in the open window with no reload and no click.                                                                           |
| AF5 | Try to make the client reach out (network, the shell, storage). | The app open.                      | Every attempt fails: no fetch, no socket, no access to the shell's page, its cookie or its storage.                                        |
| AF6 | Switch the shell between light and dark.                       | The app open.                      | The app follows at once, without reloading; the app's styles did not change the shell.                                                     |
| AF7 | Use two apps with clients side by side.                        | Both apps open.                    | A task added in one is not in the other: each client reaches only its own backend.                                                         |
| AF8 | Send the backend a call its definition does not allow.         | The app open.                      | The router refuses it with a JSON-RPC error that says what is wrong (invalid params, method not found); the window is unharmed.           |

## Manual verification

Recorded in the pull request (`verify-change`).
