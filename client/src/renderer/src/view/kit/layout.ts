/**
 * The tier-1 layout, navigation and collection adapters (wave B1a): one `register` call per catalogue component.
 * Controls, presentation and display live in `kit/controls.ts` (wave B1b); `kit.ts` calls both.
 */
import Adaptive from '../molecules/Adaptive.svelte'
import Card from '../molecules/Card.svelte'
import Divider from '../molecules/Divider.svelte'
import ForEach from '../molecules/ForEach.svelte'
import Grid from '../molecules/Grid.svelte'
import Group from '../molecules/Group.svelte'
import LabeledContent from '../molecules/LabeledContent.svelte'
import Scope from '../molecules/Scope.svelte'
import ScrollView from '../molecules/ScrollView.svelte'
import Show from '../molecules/Show.svelte'
import Spacer from '../molecules/Spacer.svelte'
import Stack from '../molecules/Stack.svelte'
import ZStack from '../molecules/ZStack.svelte'
import DisclosureGroup from '../organisms/DisclosureGroup.svelte'
import EmptyState from '../organisms/EmptyState.svelte'
import NavigationLink from '../organisms/NavigationLink.svelte'
import Row from '../organisms/Row.svelte'
import Form from '../pages/Form.svelte'
import Screen from '../pages/Screen.svelte'
import SplitView from '../pages/SplitView.svelte'
import type { ComponentRegistry } from '../registry'
import List from '../templates/List.svelte'
import NavigationStack from '../templates/NavigationStack.svelte'
import Section from '../templates/Section.svelte'
import TabView from '../templates/TabView.svelte'

export function registerLayout(registry: ComponentRegistry): ComponentRegistry {
  return (
    registry
      // Layout
      .register('Stack', { component: Stack, since: '1.0' })
      .register('ZStack', { component: ZStack, since: '1.0' })
      .register('Grid', { component: Grid, since: '1.0' })
      .register('ScrollView', {
        component: ScrollView,
        since: '1.0',
        bindings: ['scrollTo', 'offset']
      })
      .register('Spacer', { component: Spacer, since: '1.0' })
      .register('Divider', { component: Divider, since: '1.0' })
      .register('Group', { component: Group, since: '1.0' })
      .register('Adaptive', { component: Adaptive, since: '1.0' })
      .register('Scope', { component: Scope, since: '1.0' })
      .register('Show', { component: Show, since: '1.0' })
      .register('ForEach', { component: ForEach, since: '1.0' })
      .register('Card', { component: Card, since: '1.0' })
      // Navigation
      .register('Screen', { component: Screen, since: '1.0' })
      .register('NavigationStack', { component: NavigationStack, since: '1.0', bindings: ['path'] })
      .register('NavigationLink', { component: NavigationLink, since: '1.0' })
      .register('SplitView', { component: SplitView, since: '1.0', bindings: ['visibility'] })
      .register('TabView', { component: TabView, since: '1.0', bindings: ['selection'] })
      // Collections
      .register('List', { component: List, since: '1.0', bindings: ['selection', 'editing'] })
      .register('Section', { component: Section, since: '1.0', bindings: ['expanded'] })
      .register('Row', { component: Row, since: '1.0' })
      .register('Form', { component: Form, since: '1.0' })
      .register('LabeledContent', { component: LabeledContent, since: '1.0' })
      .register('DisclosureGroup', {
        component: DisclosureGroup,
        since: '1.0',
        bindings: ['expanded']
      })
      .register('EmptyState', { component: EmptyState, since: '1.0' })
  )
}
