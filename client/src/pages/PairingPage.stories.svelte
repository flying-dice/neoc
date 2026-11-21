<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import PairingPage from './PairingPage.svelte'
  import type { PairResult } from '../shell/pairing'

  /** The pairing screen: what a browser the host does not know yet sees first. */
  const { Story } = defineMeta({
    title: 'Desktop/Pairing',
    component: PairingPage,
    parameters: { layout: 'fullscreen' }
  })

  const refuse = (result: PairResult) => async (): Promise<PairResult> => result

  const submit = async ({
    canvas,
    userEvent
  }: {
    canvas: { getByTestId(id: string): HTMLElement }
    userEvent: {
      type(el: HTMLElement, text: string): Promise<void>
      click(el: HTMLElement): Promise<void>
    }
  }): Promise<void> => {
    await userEvent.type(canvas.getByTestId('pairing-code'), 'k7qm3xhr')
    await userEvent.click(canvas.getByTestId('pairing-submit'))
  }
</script>

<Story name="Enter the code" args={{ onpair: refuse({ ok: true }) }} />

<Story
  name="Code typed"
  args={{ onpair: refuse({ ok: true }) }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('pairing-code'), 'k7qm3xhr')
    await expect(canvas.getByTestId('pairing-code')).toHaveValue('K7QM-3XHR')
    await expect(canvas.getByTestId('pairing-submit')).toBeEnabled()
  }}
/>

<Story
  name="Wrong or expired code"
  args={{
    onpair: refuse({
      ok: false,
      reason: 'invalid',
      message:
        'That code did not work. It may be mistyped, already used, or older than 10 minutes. Run “agentos-host pair” for a new one.'
    })
  }}
  play={async (context) => {
    await submit(context)
    await expect(context.canvas.getByTestId('pairing-error')).toBeVisible()
  }}
/>

<Story
  name="Too many attempts"
  args={{
    onpair: refuse({
      ok: false,
      reason: 'rate-limited',
      retryAfterSecs: 42,
      message: 'Too many attempts. Try again in 42 seconds.'
    })
  }}
  play={async (context) => {
    await submit(context)
    await expect(context.canvas.getByTestId('pairing-error')).toHaveTextContent('42 seconds')
  }}
/>

<Story
  name="Host not reachable"
  args={{
    onpair: refuse({ ok: true }),
    unreachable: 'The host answered 502.'
  }}
/>
