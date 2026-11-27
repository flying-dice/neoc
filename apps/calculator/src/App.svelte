<script lang="ts">
  import { Calculator, type CalculatorRpc } from './lib/calculator.svelte'
  import CalculatorPage from './pages/CalculatorPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private method on the bus, the
  // global `RPC`). Each window holds a calculator of its own.
  const rpc: CalculatorRpc = {
    press: (params) => RPC.send_request('calculator', '_press', params)
  }
  const calculator = new Calculator(rpc)
</script>

<CalculatorPage {calculator} />
