<script module lang="ts">
  import type { ComponentProps } from 'svelte'
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import Button from './Button.svelte'
  import Icon from './Icon.svelte'

  const { Story } = defineMeta({
    title: 'Atoms/Button',
    component: Button,
    tags: ['autodocs'],
    args: { testid: 'button', variant: 'filled', size: 'medium', capsule: false, destructive: false, disabled: false },
    argTypes: {
      variant: { control: 'inline-radio', options: ['filled', 'tinted', 'gray', 'plain'] },
      size: { control: 'inline-radio', options: ['small', 'medium', 'large'] },
      tint: { control: 'color' }
    },
    render: template
  })
</script>

{#snippet template(args: Omit<ComponentProps<typeof Button>, 'children'>)}
  <Button {...args}>Continue</Button>
{/snippet}

<Story name="Filled" />
<Story name="Tinted" args={{ variant: 'tinted' }} />
<Story name="Gray" args={{ variant: 'gray' }} />
<Story name="Plain" args={{ variant: 'plain' }} />
<Story name="Capsule" args={{ capsule: true }} />
<Story name="Destructive" args={{ variant: 'tinted', destructive: true }} />
<Story name="Disabled" args={{ disabled: true }} />

<Story name="All Variants" asChild parameters={{ controls: { disable: true } }}>
  <div style="display:grid;grid-template-columns:repeat(4,auto);gap:12px;align-items:center">
    {#each ['small', 'medium', 'large'] as const as size (size)}
      {#each ['filled', 'tinted', 'gray', 'plain'] as const as variant (variant)}
        <Button testid="button-{size}-{variant}" {variant} {size}>{variant}</Button>
      {/each}
    {/each}
  </div>
</Story>

<Story name="With Icon" asChild parameters={{ controls: { disable: true } }}>
  <div style="display:flex;gap:12px">
    <Button testid="button-new-agent"><Icon name="plus" size={17} weight={2.2} /> New Agent</Button>
    <Button testid="button-run" variant="tinted" capsule><Icon name="play" size={15} /> Run</Button>
    <Button testid="button-more" variant="gray" square aria-label="More"><Icon name="ellipsis" size={19} /></Button>
  </div>
</Story>
