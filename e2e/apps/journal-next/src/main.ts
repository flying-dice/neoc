// The page of the fixture's next version (0.2.0): its version, a field to write an entry in, and the entries.
type Entry = VosApp.Journal.Entry

const VERSION = '0.2.0'

const api = {
  list: (): Promise<Entry[]> => RPC.send_request('journal', '_list'),
  add: (params: VosApp.Journal.AddParams): Promise<Entry[]> =>
    RPC.send_request('journal', '_add', params)
}

const element = <T extends HTMLElement>(id: string): T => {
  const found = document.querySelector<T>(`[data-testid="${id}"]`)
  if (!found) throw new Error(`no ${id}`)
  return found
}

const entries = element<HTMLUListElement>('journal-entries')
const body = element<HTMLInputElement>('journal-body')

function show(list: Entry[]): void {
  entries.replaceChildren(
    ...list.map((entry) => {
      const item = document.createElement('li')
      item.dataset.testid = `journal-entry-${entry.id}`
      item.textContent = `${entry.body} (written in ${entry.edition})`
      return item
    })
  )
}

element('journal-version').textContent = `Journal ${VERSION}`
element<HTMLFormElement>('journal-form').addEventListener('submit', (e) => {
  e.preventDefault()
  void api.add({ body: body.value }).then((list) => {
    body.value = ''
    show(list)
  })
})
void api.list().then(show)
