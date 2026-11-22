# Notifications: key user journeys

An app's backend can tell the person something with one call, `app.notify { title, body }`, from a handler, an agent's
tool or its own background work. The shell draws it as a banner at the top of the desktop, above every window, naming
the app it came from (the shell's own name and icon for that app, never what the app wrote). The journeys use the
Notifier, a test app whose buttons start background work that notifies two seconds later (it is built for the tests and
is not in Discover; the specs install it and remove it again), so a window can be closed or another app brought forward
by the time the banner arrives.

What the person can rely on:

- The banner shows when the app has no window open and while another app is in front. It does not show on a client where
  one of the app's windows is in front; every other client that is connected shows it.
- Choosing it (click, Enter) opens the app's main window, or brings it forward; Dismiss (or Escape on the banner) closes
  it and opens nothing. It also goes by itself after ten seconds of the page being on screen (a banner that arrives in a
  background tab starts its time when the tab is shown), and waits while keyboard focus is on it or the pointer is being
  moved over it (a pointer resting where the banner appeared holds nothing).
- A tab that is in the background, or a browser with another application in front, does not count as the app being in
  front: the banner shows.
- At most three are on screen, newest on top. A fourth takes the place of the same app's oldest banner, so one app's burst
  does not push another app's banner off; an app with none on screen displaces the oldest. A banner someone is reading is
  the last to go.
- Keyboard focus is never dropped to the page: when the banner that has it goes, it moves to the nearest banner left, else
  back to where it was, else to the front window or the Dock.
- A client that is not connected when the app notifies never gets it, and nothing is kept: a banner that has gone is gone
  (there is no notification list in vos yet).
- Keyboard: Alt+Shift+N moves focus to the newest banner from anywhere, Enter opens the app and Escape dismisses the banner
  and gives focus back to where it was. The banners are also first in the page's order (before the widgets, the strip, the
  windows and the Dock): Tab from the very top of the page, or Shift+Tab back out of a window, reaches them; Tab from
  inside a window goes on to what follows that window, so from there the shortcut is the way. Screen readers hear the
  sender, the title and the start of the body in a polite live region.
- A page with no shell, an app opened on its own at `/apps/<id>`, draws no banner: the notification is the shell's.

Automated twins have the same ids: `e2e/notifications.journeys.spec.ts` (they share one Notifier and run last, because a
notification reaches every open page).

| Id  | Who / goal                                              | Starts at                                            | Done when                                                                                                                                                   |
| --- | ------------------------------------------------------- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| NO1 | Be told by an app that has no window open, and dismiss. | The Notifier open; "Notify in a moment" tapped.      | The window was closed and a banner still came, naming Notifier, with the title and body, announced; Dismiss closed it and opened nothing.                   |
| NO2 | Go to the app that sent it.                             | A banner, with another app in front.                 | Choosing the banner closed it, brought the Notifier's window forward and put the keys in it.                                                                |
| NO3 | Do all of it by keyboard.                               | A banner, with another app in front.                 | Alt+Shift+N focused the banner and Escape dismissed it; for the next one the same shortcut and Enter opened the app.                                    |
| NO4 | Read a burst, long text and odd text.                   | Another app in front; the Notifier's buttons tapped. | Four notifications showed three, newest first; long text stayed small; a title with a line break, an override and a zero-width space showed as one clean line; markup in the body showed as characters. |
| NO5 | Read a banner in light and dark at a small window.      | A 360-point-wide window, each theme.                 | The banner fitted the screen and axe found no violation, in both themes.                                                                                    |
| NO6 | Dismiss a banner with the mouse.                        | A banner, with another app in front.                 | The keys were not left on the page: focus was on the front window or the Dock.                                                                              |
| NO7 | Have two clients connected at once.                     | Two paired browsers, the Notifier in front on one.   | Only the browser where the Notifier was not in front showed the banner; once the first moved to another app, both did.                                      |
| NO8 | Reach a banner by Tab and Shift+Tab.                    | A banner, with another app in front.                 | The banner's control was the first thing in the page that takes Tab, and Shift+Tab from the front window reached it.                                        |

## Manual verification

Ran the real shell on the real host headless (Chromium; `vos-host` on port 10310 with a fresh data directory and the
Notifier installed from its app ref, driven by a Playwright script) after the first review round, and looked at the
screenshots, and at every new story baseline (light and dark, over a busy window):

- [x] Four notifications raised by the Notifier's background task while Files was in front: three banners stacked under the
      status bar, newest on top ("Message 4", "3", "2"), each with the Notifier's bell icon and pink plate and the name
      "NOTIFIER" from the shell's list, above the Files window. The live region says "Notifier: Message 4  Notifier:
      Message 3  Notifier: Message 2".
- [x] Odd text (a line break, a right-to-left override and a zero-width space in the title, markup in the body), light and
      dark at 1280x800 and 360x640: the title is one line, "Odd title gnp.exe"; the body shows its line break and
      `<b>not bold</b>` as characters. The banner fits the 360-point screen with its margins.
- [x] Keyboard: Alt+Shift+N moves focus to the newest banner's control with a visible focus ring round the card (also when
      the last thing done was a click); Escape removes the banner and focus returns to the Dock button it came from.
- [x] Stories: the focus ring is in the Keyboard baselines (molecule and desktop); the banners are drawn over a busy window
      in light and dark; a family emoji, the flag of England, a Persian word and an Indic conjunct keep their joiners
      (the emoji shows as one glyph, the flag as the cross of St George); one app's burst leaves another app's banner.
- [x] `bun run gate` on the head commit: neoc checks, lint, typecheck, test, test:e2e (the NO1 to NO7 journeys, the story
      renders, axe in light and dark and the snapshots of the new stories).

Not checked: a real screen reader (the roles, names, descriptions and live region were checked in the accessibility tree
and by axe), Split View and Slide Over with a banner (the focus rule has unit tests for both), the text-size setting
(nothing in the shell scales with the root font size today: the banner is set in the shell's own type tokens, so it
follows them when that setting lands), a frame-based app as the sender (#56/#57 are not on `main` yet).
