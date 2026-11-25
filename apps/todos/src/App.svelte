<script lang="ts">
  import { api } from '@todos/rpc'
  import { host } from '@vos/host'
  import { Todos } from './lib/todos.svelte'
  import TodosPage from './pages/TodosPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const todos = new Todos(api, host)
</script>

<TodosPage {todos} />
