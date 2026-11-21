# UI Kit: key user journeys

The living catalogue of `vos:ui`, for people building vos apps (the app id is `gallery`, the window is titled
"UI Kit"). It is opened from the Dock's recents or from Discover's "Explore the UI Kit". Start at the host's address in
a paired browser with UI Kit open.

Automated twins (same ids): `e2e/gallery.journeys.spec.ts`.

| Id  | Who / goal                                                               | Starts at   | Done when                                                                         |
| --- | ------------------------------------------------------------------------ | ----------- | --------------------------------------------------------------------------------- |
| G1  | An app developer: find which button style to use for a secondary action. | UI Kit open | They found the button variants (filled, tinted, gray, plain) and picked "tinted" or "gray". |
| G2  | Check how components look in dark mode.                                  | UI Kit open | They switched the catalogue's appearance to Dark and back; only the catalogue changed, not the desktop around it. |
| G3  | Try a sheet and an alert, then dismiss each.                             | UI Kit open | Both opened and were dismissed in the expected way (Cancel, the scrim or Escape). |
| G4  | Find the system colours and the grays.                                   | UI Kit open | They can point to the colour swatches and the gray ramp.                          |
