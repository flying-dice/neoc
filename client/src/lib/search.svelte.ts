import { appOf, appOrder, installedApp } from './apps.svelte'
import { settingsNavigation } from './settingsPanes.svelte'
import { builtinSources, RecentlyOpened, Spotlight, type SearchableApp } from './spotlight.svelte'
import { windowManager } from './windowManager.svelte'

/**
 * The shell's system search: the one `Spotlight` the desktop shows, over the sources registered here. To have search
 * look somewhere else as well (inside an app's own content), add a `SpotlightSource` to the list below.
 */

/** The apps brought to the front in this session, most recent first (the desktop notes them). */
export const recentlyOpened = new RecentlyOpened()

function searchableApps(): SearchableApp[] {
  return appOrder().flatMap((id) => {
    const app = appOf(id)
    return app
      ? [
          {
            id,
            name: app.name,
            description: installedApp(id)?.description ?? undefined,
            icon: app.icon,
            background: app.background
          }
        ]
      : []
  })
}

export const spotlight = new Spotlight([
  ...builtinSources({
    apps: searchableApps,
    recent: () => [...recentlyOpened.ids],
    openApp: (id) => windowManager.open(id),
    openSettings: (pane) => {
      settingsNavigation.show(pane)
      windowManager.open('settings')
    },
    atHome: () => windowManager.atHome,
    showHome: () => windowManager.showHome()
  })
  // further sources go here
])
