import { useCallback, useEffect, useState } from 'react'
import { api, type Task } from '@tasks-react/rpc'
import { host } from '@vos/host'

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

export default function App() {
  const [tasks, setTasks] = useState<Task[]>([])
  const [title, setTitle] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [theme, setTheme] = useState(host.theme)
  // where the app runs: `host.hosted` is true when the host page draws UI of its own (the shell)
  const [where, setWhere] = useState('connecting')

  // run a method that answers with the whole list; a failure is the backend's own message
  const show = useCallback(async (call: Promise<Task[]>): Promise<boolean> => {
    setError(null)
    try {
      setTasks(await call)
      return true
    } catch (e) {
      setError(messageOf(e))
      return false
    }
  }, [])

  // load once, then again whenever the backend says the list changed outside this window (an agent added a task)
  useEffect(() => {
    void show(api.list())
    const stops = [
      host.on('tasks.changed', () => void show(api.list())),
      // the host connection came back: a change made while it was away reached nobody
      host.on('host:reconnected', () => void show(api.list())),
      host.onContext((context) => setTheme(context.theme === 'dark' ? 'dark' : 'light'))
    ]
    void host.ready.then(() => {
      setTheme(host.theme)
      setWhere(host.hosted ? 'in the shell' : 'on its own')
    })
    return () => stops.forEach((stop) => stop())
  }, [show])

  // a real form: the frame is sandboxed with allow-forms, so submit fires (Enter, the button); an unprevented submission would send the
  // frame to another page, so the handler prevents it
  async function add(): Promise<void> {
    if (await show(api.add({ title }))) setTitle('')
  }

  return (
    <main className="app" data-testid="tasks-react-app">
      <h1>Tasks</h1>
      <p className="meta">
        <span data-testid="tasks-react-framework">React</span> ·{' '}
        <span data-testid="tasks-react-theme">{theme}</span> ·{' '}
        <span data-testid="tasks-react-where">{where}</span> ·{' '}
        <span data-testid="tasks-react-count">{tasks.filter((t) => !t.done).length} left</span>
      </p>
      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault()
          void add()
        }}
      >
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          aria-label="New task"
          placeholder="What has to be done?"
          data-testid="tasks-react-title"
        />
        <button type="submit" data-testid="tasks-react-add">
          Add
        </button>
      </form>
      {error && (
        <p className="error" role="alert" data-testid="tasks-react-error">
          {error}
        </p>
      )}
      <ul>
        {tasks.map((task) => (
          <li key={task.id} className={task.done ? 'done' : ''} data-testid={`tasks-react-item-${task.id}`}>
            <label>
              <input
                type="checkbox"
                checked={task.done}
                onChange={() => show(api.toggle({ id: task.id }))}
                data-testid={`tasks-react-toggle-${task.id}`}
              />
              <span>{task.title}</span>
            </label>
          </li>
        ))}
      </ul>
    </main>
  )
}
