import { useCallback, useEffect, useState } from 'react'

const messageOf = (e) => (e instanceof Error ? e.message : String(e))

export default function App({ rpc, host }) {
  const [tasks, setTasks] = useState([])
  const [title, setTitle] = useState('')
  const [error, setError] = useState(null)
  const [theme, setTheme] = useState(host.theme)

  // run a handler that answers with the whole list; a failure is the handler's own message
  const call = useCallback(
    async (method, args) => {
      setError(null)
      try {
        setTasks(await rpc(method, args))
        return true
      } catch (e) {
        setError(messageOf(e))
        return false
      }
    },
    [rpc]
  )

  // load once, then again whenever the backend says the list changed outside this window (an agent added a task)
  useEffect(() => {
    void call('list')
    const stops = [
      host.on('tasks.changed', () => void call('list')),
      host.on('host:theme', (t) => setTheme(t === 'dark' ? 'dark' : 'light'))
    ]
    return () => stops.forEach((stop) => stop())
  }, [call, host])

  // not a <form>: the frame is sandboxed without allow-forms, so a form cannot submit; Enter is handled here
  async function add() {
    if (await call('add', { title })) setTitle('')
  }

  return (
    <main className="app" data-testid="tasks-react-app">
      <h1>Tasks</h1>
      <p className="meta">
        <span data-testid="tasks-react-framework">React</span> ·{' '}
        <span data-testid="tasks-react-theme">{theme}</span> ·{' '}
        <span data-testid="tasks-react-count">{tasks.filter((t) => !t.done).length} left</span>
      </p>
      <div className="row">
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && add()}
          aria-label="New task"
          placeholder="What has to be done?"
          data-testid="tasks-react-title"
        />
        <button type="button" onClick={add} data-testid="tasks-react-add">
          Add
        </button>
      </div>
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
                onChange={() => call('toggle', { id: task.id })}
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
