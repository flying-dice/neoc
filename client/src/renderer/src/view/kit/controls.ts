/**
 * Tier-1 controls, presentation and display adapters (wave B1b), plus the decorator that gives every node the
 * common `contextMenu` slot and `drag` / `drop` behaviour. One `register` call per catalogue type; `bindings` names
 * the props that are two-way `Binding`s (the schema's `bindings` table, `types/generated/agentos.ts`).
 *
 *   Controls      Shortcut, Button, Toggle, Slider, Stepper, Picker, DatePicker, ColorPicker, TextField,
 *                 SecureField, TextEditor, SearchField
 *   Presentation  Sheet, Popover, Alert, ConfirmationDialog, Menu (and the contextMenu slot, via the decorator)
 *   Display       Text, Label, Icon, Image, Badge, ProgressView, Markdown, Avatar
 */
import Avatar from '../molecules/Avatar.svelte'
import Badge from '../molecules/Badge.svelte'
import Button from '../molecules/Button.svelte'
import Icon from '../molecules/Icon.svelte'
import Image from '../molecules/Image.svelte'
import Markdown from '../molecules/Markdown.svelte'
import ProgressView from '../molecules/ProgressView.svelte'
import SecureField from '../molecules/SecureField.svelte'
import Shortcut from '../molecules/Shortcut.svelte'
import Slider from '../molecules/Slider.svelte'
import Stepper from '../molecules/Stepper.svelte'
import Text from '../molecules/Text.svelte'
import TextEditor from '../molecules/TextEditor.svelte'
import TextField from '../molecules/TextField.svelte'
import Toggle from '../molecules/Toggle.svelte'
import Alert from '../organisms/Alert.svelte'
import ColorPicker from '../organisms/ColorPicker.svelte'
import ConfirmationDialog from '../organisms/ConfirmationDialog.svelte'
import DatePicker from '../organisms/DatePicker.svelte'
import Decorated from '../organisms/Decorated.svelte'
import Label from '../organisms/Label.svelte'
import Menu from '../organisms/Menu.svelte'
import Picker from '../organisms/Picker.svelte'
import Popover from '../organisms/Popover.svelte'
import SearchField from '../organisms/SearchField.svelte'
import Sheet from '../organisms/Sheet.svelte'
import type { ComponentRegistry } from '../registry'

export function registerControls(registry: ComponentRegistry): ComponentRegistry {
  registry.decorator = Decorated
  return (
    registry
      // Controls
      .register('Shortcut', { component: Shortcut, since: '1.0' })
      .register('Button', { component: Button, since: '1.0' })
      .register('Toggle', { component: Toggle, since: '1.0', bindings: ['on'] })
      .register('Slider', { component: Slider, since: '1.0', bindings: ['value'] })
      .register('Stepper', { component: Stepper, since: '1.0', bindings: ['value'] })
      .register('Picker', { component: Picker, since: '1.0', bindings: ['selection'] })
      .register('DatePicker', { component: DatePicker, since: '1.0', bindings: ['value'] })
      .register('ColorPicker', { component: ColorPicker, since: '1.0', bindings: ['value'] })
      .register('TextField', { component: TextField, since: '1.0', bindings: ['text'] })
      .register('SecureField', { component: SecureField, since: '1.0', bindings: ['text'] })
      .register('TextEditor', { component: TextEditor, since: '1.0', bindings: ['text', 'selection'] })
      .register('SearchField', { component: SearchField, since: '1.0', bindings: ['text'] })
      // Presentation
      .register('Sheet', { component: Sheet, since: '1.0', bindings: ['presented', 'item'] })
      .register('Popover', { component: Popover, since: '1.0', bindings: ['presented'] })
      .register('Alert', { component: Alert, since: '1.0', bindings: ['presented'] })
      .register('ConfirmationDialog', { component: ConfirmationDialog, since: '1.0', bindings: ['presented'] })
      .register('Menu', { component: Menu, since: '1.0' })
      // Display
      .register('Text', { component: Text, since: '1.0' })
      .register('Label', { component: Label, since: '1.0' })
      .register('Icon', { component: Icon, since: '1.0' })
      .register('Image', { component: Image, since: '1.0' })
      .register('Badge', { component: Badge, since: '1.0' })
      .register('ProgressView', { component: ProgressView, since: '1.0' })
      .register('Markdown', { component: Markdown, since: '1.0' })
      .register('Avatar', { component: Avatar, since: '1.0' })
  )
}
