<script lang="ts">
  import { Todos, type TodosEvents, type TodosRpc } from './lib/todos.svelte'
  import TodosPage from './pages/TodosPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: TodosRpc = {
    board: () => RPC.send_request('todos', '_board'),
    addList: (params) => RPC.send_request('todos', '_add_list', params),
    removeList: (params) => RPC.send_request('todos', '_remove_list', params),
    add: (params) => RPC.send_request('todos', '_add', params),
    update: (params) => RPC.send_request('todos', '_update', params),
    toggle: (params) => RPC.send_request('todos', '_toggle', params),
    remove: (params) => RPC.send_request('todos', '_remove', params)
  }
  const events: TodosEvents = {
    on: (name, fn) => (name === 'todos_changed' ? RPC.on_notification('todos', 'todos_changed', fn) : vos.host.on(name, fn))
  }
  const todos = new Todos(rpc, events)
</script>

<TodosPage {todos} />
