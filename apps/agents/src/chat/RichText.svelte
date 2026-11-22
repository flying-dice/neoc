<!--
  RichText — renders agent text with a safe subset of Markdown
  (paragraphs, **bold**, `code`, "- " bullets). Builds nodes, never {@html},
  so agent output can't inject markup.
-->
<script lang="ts">
  interface Props {
    text: string
  }

  let { text }: Props = $props()

  type Span = { kind: 'text' | 'bold' | 'code'; value: string }
  type Block = { kind: 'p' | 'li'; spans: Span[] }

  function spans(line: string): Span[] {
    const out: Span[] = []
    const re = /(\*\*[^*]+\*\*|`[^`]+`)/g
    let last = 0
    for (const m of line.matchAll(re)) {
      if (m.index > last) out.push({ kind: 'text', value: line.slice(last, m.index) })
      const token = m[0]
      out.push(
        token.startsWith('**')
          ? { kind: 'bold', value: token.slice(2, -2) }
          : { kind: 'code', value: token.slice(1, -1) }
      )
      last = m.index + token.length
    }
    if (last < line.length) out.push({ kind: 'text', value: line.slice(last) })
    return out
  }

  const blocks = $derived<Block[]>(
    text
      .split('\n')
      .filter((l) => l.trim() !== '')
      .map((l) =>
        /^\s*[-*] /.test(l)
          ? { kind: 'li', spans: spans(l.replace(/^\s*[-*] /, '')) }
          : { kind: 'p', spans: spans(l) }
      )
  )
</script>

<div class="rich">
  {#each blocks as block, i (i)}
    <p class:li={block.kind === 'li'}>
      {#each block.spans as span, j (j)}
        {#if span.kind === 'bold'}<strong>{span.value}</strong>
        {:else if span.kind === 'code'}<code>{span.value}</code>
        {:else}{span.value}{/if}
      {/each}
    </p>
  {/each}
</div>

<style>
  .rich {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  p {
    margin: 0;
  }
  .li {
    padding-left: 14px;
    position: relative;
  }
  .li::before {
    content: '•';
    position: absolute;
    left: 2px;
  }
  code {
    font-family: var(--font-mono);
    font-size: 0.88em;
    padding: 1px 5px;
    border-radius: 5px;
    background: var(--fill-tertiary);
  }
</style>
