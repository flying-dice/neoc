<script lang="ts">
  import { api } from 'app:rpc'
  import { Calculator } from './lib/calculator.svelte'
  import CalculatorPage from './pages/CalculatorPage.svelte'

  // The composition root: the one place the state meets the real backend. Each window holds a calculator of its own.
  const calculator = new Calculator(api)
</script>

<CalculatorPage {calculator} />
