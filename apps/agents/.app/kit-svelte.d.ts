// Generated from the kit's custom-elements.json by the vos Vite plugin (packages/vite-plugin/src/kit-types.ts). Do not edit.
import type { HTMLAttributes } from 'svelte/elements'

declare module 'svelte/elements' {
  interface SvelteHTMLElements {
    /** An alert that asks before something is done (UIAlertController's alert): a heading, a message, Cancel and the answer that goes ahead. While `open` it is modal: the window behind it is dimmed and inert, the focus stays inside and goes back where it was when the alert closes. The answer that changes nothing is in front. With `destructive` the answer that goes ahead is red, and Cancel is bold and has the focus, so a stray Return deletes nothing; without it the answer that goes ahead is bold and has the focus. Escape, and the browser's own request to close (a back gesture), answer Cancel. So does hiding what holds the alert while it is open: a question nobody can see is not left over an inert page. An answer closes the alert (`open` becomes false, the focus goes back) and then raises its event. The two answers have the test ids `<data-testid of the alert>-cancel` and `<data-testid of the alert>-confirm`. */
    "ui-alert": Omit<HTMLAttributes<HTMLElement>, 'onconfirm' | 'oncancel'> & {
      /** Shows the alert, over everything else in the window. */
      open?: boolean
      /** The question: `Delete Work?` */
      heading?: string
      /** What going ahead does, under the heading. */
      message?: string
      /** The word on the button that goes ahead (default `OK`). */
      "action-label"?: string
      /** The word on the button that changes nothing (default `Cancel`). */
      "cancel-label"?: string
      /** Going ahead deletes something or cannot be undone: its button is red, and Cancel has the focus. */
      destructive?: boolean
      /** The test id; the answers' are this followed by `-cancel` and `-confirm`. */
      "data-testid"?: string | null
      /** The person chose the answer that goes ahead. The alert has closed. It does not bubble. */
      onconfirm?: (event: Event) => void
      /** Cancel, Escape or the browser's request to close. The alert has closed. It does not bubble. */
      oncancel?: (event: Event) => void
    }
    /** An inline message with an optional action, for something the person should know about in place (a failed save, a missing permission). It is announced when it appears: a `status` region, or an `alert` for an `error`. */
    "ui-banner": HTMLAttributes<HTMLElement> & {
      /** `info` (default), `success`, `warning` or `error`. */
      tone?: 'info' | 'success' | 'warning' | 'error'
      /** A bold first line. */
      heading?: string
    }
    /** A button in the four iOS styles. It is a real `<button>` inside, so Enter and Space press it, and it submits or resets the form it sits in (`type`). A form control like the others (see `FormControl`): a disabled fieldset disables it. A native submit button is the `submitter` of the `submit` event; a kit button cannot be, so `event.submitter` is null and the form's data carries the button's `name` and `value` instead. `fill` takes a data colour (a palette token such as `orange`, or `#rrggbb`) for a `filled` button; the text colour is picked for 4.5:1 on it. Unlike `tint`, which is the control's accent, a fill is content. Drawn 28 or 34 pt high. With `hit` it has a 44 x 44 pt hit area, as the shell's `hit` does (see `FormControl`). */
    "ui-button": HTMLAttributes<HTMLElement> & {
      /** `filled` (default), `tinted`, `gray` or `plain`. */
      variant?: 'filled' | 'tinted' | 'gray' | 'plain'
      /** `small`, `medium` (default), `large`, or `fill`, which takes the whole box the parent gives it. */
      size?: 'small' | 'medium' | 'large' | 'fill'
      /** `rounded` (default), `capsule` or `circle` (a round key: use with `size="fill"`). */
      shape?: 'rounded' | 'capsule' | 'circle'
      /** A square button sized for one glyph. */
      square?: boolean
      /** Any CSS colour for the accent; defaults to the inherited `--tint`. */
      tint?: string | undefined
      /** A data colour for a `filled` button: a palette token or `#rrggbb`. */
      fill?: string | undefined
      /** Red, for an action that deletes or cannot be undone. */
      destructive?: boolean
      /** Makes it a toggle button: `aria-pressed` follows `pressed`. */
      toggle?: boolean
      /** The pressed state of a `toggle` button. */
      pressed?: boolean
      /** `button` (default), `submit` or `reset`. */
      type?: 'button' | 'submit' | 'reset'
      /** The value submitted with `name` by a submit button. */
      value?: string
      /** The form field name. Nothing is submitted without one. */
      name?: string
      /** A 44 x 44 pt hit area around the drawn control, as the shell's `hit` is (off by default: the control is drawn at its own size). Set it wherever nothing else gives the control a 44 pt target; a row, a banner and a toolbar item do. */
      hit?: boolean
      /** Dims the control and ignores input; also set by a disabled `<fieldset>` around it. */
      disabled?: boolean
      /** The accessible name. A `<label for>` pointing at the element names it too. */
      label?: string
      /** Read after the name. */
      description?: string
    }
    /** A round check (the selection mark of a list row, a task done). A form control: with a `name` it submits `value` while checked. */
    "ui-check": Omit<HTMLAttributes<HTMLElement>, 'onchange'> & {
      /** Whether it is on. */
      checked?: boolean
      /** The value submitted while on. */
      value?: string
      /** The form is invalid until it is on. */
      required?: boolean
      /** On-state colour: any CSS colour. */
      tint?: string | undefined
      /** The form field name. Nothing is submitted without one. */
      name?: string
      /** A 44 x 44 pt hit area around the drawn control, as the shell's `hit` is (off by default: the control is drawn at its own size). Set it wherever nothing else gives the control a 44 pt target; a row, a banner and a toolbar item do. */
      hit?: boolean
      /** Dims the control and ignores input; also set by a disabled `<fieldset>` around it. */
      disabled?: boolean
      /** The accessible name. A `<label for>` pointing at the element names it too. */
      label?: string
      /** Read after the name. */
      description?: string
      /** The person flipped it. `checked` is already updated. */
      onchange?: (event: CustomEvent<{ checked: boolean }>) => void
    }
    /** What a screen shows when there is nothing to show: a muted glyph, a title, an optional message and one call to action. */
    "ui-empty-state": HTMLAttributes<HTMLElement> & {
      /** The glyph's name from the icon set. */
      icon?: string
      /** The title. */
      heading?: string
      /** A line of explanation under the title. */
      message?: string
    }
    /** A glyph from the vos icon set (the same set the shell draws). Decorative unless it has a `label`, then it is an image with that name. The markup comes only from the constant icon map, looked up by `name`; an unknown name draws nothing, and no app text is ever read as markup. */
    "ui-icon": HTMLAttributes<HTMLElement> & {
      /** The glyph's name, for example `chevron.right` or `sparkles`. */
      name?: string
      /** Width and height in px. */
      size?: number
      /** Stroke weight, as SF Symbols weights: 1.5 light, 2 regular, 2.5 semibold. */
      weight?: number
      /** An accessible name; leave it out for a decorative icon. */
      label?: string
    }
    /** A list of rows in `ui-section`s, in one of the five iPadOS styles. It owns what the rows share: the list semantics (a `listbox` when rows are selectable), one tab stop with arrow-key navigation (Up, Down, Home, End), and the look of its sections. Selection is controlled: set `selected` on the rows you want marked and listen for `click` on a row. */
    "ui-list": HTMLAttributes<HTMLElement> & {
      /** `insetGrouped` (default), `inset`, `plain`, `grouped` or `sidebar`. */
      variant?: 'insetGrouped' | 'inset' | 'plain' | 'grouped' | 'sidebar'
      /** `none` (default), `single` or `multi`: the rows are options of a `listbox`. */
      selection?: 'none' | 'single' | 'multi'
      /** The accessible name of the list. */
      label?: string
    }
    /** A picker (SwiftUI's `Picker` in its menu style, a pop-up button): one choice from a list too long, or too changeable, for a `ui-segmented`. It shows the chosen option and opens the system's own menu of them, so the keyboard, type-ahead and a screen reader work as they do on every select. A form control (see `FormControl`): the chosen value is submitted, and `required` keeps the form invalid until an option is chosen. With a `value` no option has, it shows nothing chosen. Drawn 36 pt high; with `hit` it is a 44 pt high target, all of which opens the menu. */
    "ui-picker": Omit<HTMLAttributes<HTMLElement>, 'oninput' | 'onchange'> & {
      /** The options: `[{ value, label, disabled? }]`. A JSON attribute works too. */
      options?: { value: string; label: string; disabled?: boolean }[]
      /** The chosen option's value. */
      value?: string
      /** The form is invalid until an option is chosen. */
      required?: boolean
      /** `field` (default): a rounded filled field. `bare`: no box, for use inside a `ui-row`. */
      variant?: 'field' | 'bare'
      /** Shows the picker as invalid (`aria-invalid`): a red outline, or a red choice for a `bare` picker in a row. */
      invalid?: boolean
      /** The form field name. Nothing is submitted without one. */
      name?: string
      /** A 44 x 44 pt hit area around the drawn control, as the shell's `hit` is (off by default: the control is drawn at its own size). Set it wherever nothing else gives the control a 44 pt target; a row, a banner and a toolbar item do. */
      hit?: boolean
      /** Dims the control and ignores input; also set by a disabled `<fieldset>` around it. */
      disabled?: boolean
      /** The accessible name. A `<label for>` pointing at the element names it too. */
      label?: string
      /** Read after the name. */
      description?: string
      /** The person chose an option (the native event, passed through). `value` is already updated. */
      oninput?: (event: Event) => void
      /** The person chose an option. `value` is already updated. */
      onchange?: (event: CustomEvent<{ value: string }>) => void
    }
    /** One row of a `ui-list` (UITableViewCell): a title and subtitle, an optional icon tile, a value, a chevron, and slots for anything else. A row is plain text until it is `tappable`. A tappable row has one tab stop and fires `press` when it is tapped; the content in its `trailing` slot (a `ui-toggle`, a button) stays separately clickable and does not press the row. In a list with `selection`, a row is an option: it shows `selected`, and a tap, Enter or Space fires `press`. Listen for `press`, not `click`: a `click` bubbles from everything inside the row, a flipped trailing toggle included. */
    "ui-row": Omit<HTMLAttributes<HTMLElement>, 'onpress'> & {
      /** The row's title. (Not `title`: that attribute is the native tooltip.) */
      heading?: string
      /** A second line under the title. */
      subheading?: string
      /** A secondary value at the right, for example `On` or a network name. */
      value?: string
      /** The icon tile's glyph from the icon set. */
      icon?: string
      /** The icon tile's colour: a palette token or `#rrggbb`. Defaults to gray. */
      "icon-color"?: string
      /** Shows a disclosure chevron at the end. */
      chevron?: boolean
      /** The row is a button. */
      tappable?: boolean
      /** Selected (in a list with `selection`). */
      selected?: boolean
      /** Red title, for a row that deletes. */
      destructive?: boolean
      /** An accent-coloured action row (`Add Account…`). */
      action?: boolean
      /** Dims the row and ignores taps. */
      disabled?: boolean
      /** The accessible name when it should differ from the title and subtitle. */
      label?: string
      /** The person tapped a `tappable` row, or tapped or pressed Enter or Space on an option. */
      onpress?: (event: CustomEvent<void>) => void
    }
    /** A group of rows with an optional header and footer. It takes the look of the `ui-list` it sits in; alone it is the `insetGrouped` block of iPadOS Settings. A form is a section of rows with a field each, and its footer is where the form speaks: a note on what the fields are for, or, with `footer-tone="error"`, what is wrong with what was typed (red, and announced when it appears). */
    "ui-section": HTMLAttributes<HTMLElement> & {
      /** The title above the rows. */
      header?: string
      /** A note under the rows. */
      footer?: string
      /** `note` (default): the footer explains the rows. `error`: it says what is wrong with them, in red, as an alert. */
      "footer-tone"?: 'note' | 'error'
      /** The look; set by the list that holds the section. */
      variant?: 'insetGrouped' | 'inset' | 'plain' | 'grouped' | 'sidebar'
    }
    /** A segmented control (UISegmented Control): one choice from a few. A radio group: Tab reaches the selected segment, the arrow keys move the selection, Home and End jump. A form control (see `FormControl`): the selected value is submitted, and `required` keeps the form invalid until a segment is chosen. Each segment has the test id `<data-testid of the control>-<value>`. The segments are drawn 28 pt high; with `hit` they have a 44 pt high hit area. */
    "ui-segmented": Omit<HTMLAttributes<HTMLElement>, 'onchange'> & {
      /** The segments: `[{ value, label, disabled? }]`. A JSON attribute works too. */
      options?: { value: string; label: string; disabled?: boolean }[]
      /** The selected segment's value. */
      value?: string
      /** Fill the width the parent gives it. */
      block?: boolean
      /** The form is invalid until a segment is chosen. */
      required?: boolean
      /** The test id; each segment's is this followed by `-<value>`. */
      "data-testid"?: string | null
      /** The form field name. Nothing is submitted without one. */
      name?: string
      /** A 44 x 44 pt hit area around the drawn control, as the shell's `hit` is (off by default: the control is drawn at its own size). Set it wherever nothing else gives the control a 44 pt target; a row, a banner and a toolbar item do. */
      hit?: boolean
      /** Dims the control and ignores input; also set by a disabled `<fieldset>` around it. */
      disabled?: boolean
      /** The accessible name. A `<label for>` pointing at the element names it too. */
      label?: string
      /** Read after the name. */
      description?: string
      /** The person chose a segment. `value` is already updated. */
      onchange?: (event: CustomEvent<{ value: string }>) => void
    }
    /** A busy indicator (UIActivityIndicatorView). It says the app is working, so it keeps turning under Reduce Motion, as iOS's does. */
    "ui-spinner": HTMLAttributes<HTMLElement> & {
      /** `medium` (20 px, default) or `large` (37 px). */
      size?: 'medium' | 'large'
      /** The accessible name. */
      label?: string
    }
    /** A sidebar beside a detail (UISplitViewController, SwiftUI's `NavigationSplitView`), with a `content` column between them when one is slotted. Given 700 pt or more it shows every column; given less it is `collapsed` and shows one: the one `column` names (`NavigationSplitView`'s `preferredCompactColumn`). The app sets `column` as the person goes in (they picked a row: `detail`) and back out. The way back is the app's, in the header of what it shows, as a navigation bar's back button is: draw it while the split view is `collapsed`, and set `column` when it is pressed. Read `collapsed`, or hear `collapsedchange`, for that and for whether a selection is worth showing in the sidebar (collapsed, the sidebar is a list to pick from). When the column in front changes while the keyboard is in the one that goes, the focus moves to the column that comes, as iPadOS moves it into the view that was pushed: Tab goes on from there, and a screen reader keeps its place. The column's element takes it, and is given `tabindex="-1"` for that, which it keeps (no ring is drawn around it). It fills the height its parent gives it, and each column gives its one element its whole box. */
    "ui-split-view": Omit<HTMLAttributes<HTMLElement>, 'oncollapsedchange'> & {
      /** The column in front while collapsed: `sidebar` (default), `content` or `detail`. */
      column?: 'sidebar' | 'content' | 'detail'
      /** The view went under or over the width at which it shows one column. `collapsed` is already updated. */
      oncollapsedchange?: (event: CustomEvent<{ collapsed: boolean }>) => void
    }
    /** A multi-line text field (UITextView, SwiftUI's `TextEditor`): what `ui-text-field` is for one line, for many. Return starts a new line and never submits the form. A form control (see `FormControl`): with a `name` it submits its text, and `required`, the length limits and `setCustomValidity` take part in validation. It is `rows` lines high (with `hit`, never under 44 pt) and scrolls in itself past that; given a height by its parent (a flex or grid item, a `height`), it fills it. There is no resize handle, as there is none on iPadOS. */
    "ui-text-area": Omit<HTMLAttributes<HTMLElement>, 'oninput' | 'onchange'> & {
      /** The text. */
      value?: string
      /** A hint shown while empty. It is not the accessible name. */
      placeholder?: string
      /** How many lines high it is when its parent gives it no height. */
      rows?: number
      /** The text can be selected and copied, not changed. */
      readonly?: boolean
      /** The form is invalid while the field is empty. */
      required?: boolean
      /** The longest text, in characters. */
      maxlength?: number | undefined
      /** The shortest text, in characters, when there is any. */
      minlength?: number | undefined
      /** What the keyboard's return key says: `enter`, `done`, `go`, `next`, `previous`, `search` or `send`. */
      enterkeyhint?: string
      /** How the keyboard capitalises: `off`, `none`, `on`, `sentences`, `words` or `characters`. */
      autocapitalize?: string
      /** `field` (default): a rounded filled field. `bare`: no box, for text that is the whole pane. */
      variant?: 'field' | 'bare'
      /** Shows the field as invalid (`aria-invalid`): a red outline, or red text for a `bare` field. */
      invalid?: boolean
      /** The form field name. Nothing is submitted without one. */
      name?: string
      /** A 44 x 44 pt hit area around the drawn control, as the shell's `hit` is (off by default: the control is drawn at its own size). Set it wherever nothing else gives the control a 44 pt target; a row, a banner and a toolbar item do. */
      hit?: boolean
      /** Dims the control and ignores input; also set by a disabled `<fieldset>` around it. */
      disabled?: boolean
      /** The accessible name. A `<label for>` pointing at the element names it too. */
      label?: string
      /** Read after the name. */
      description?: string
      /** The person typed. `value` is already updated (the native event, passed through). */
      oninput?: (event: InputEvent) => void
      /** The person committed the text (blur). It has no `detail`: read `value`. */
      onchange?: (event: Event) => void
    }
    /** A single-line text field that takes what an ordinary text input takes: `type` (including `number`), `inputmode`, `maxlength`, `minlength`, `pattern`, `min`, `max`, `step`, `enterkeyhint`, `autocapitalize`, `autocomplete`. A form control (see `FormControl`): with a `name` it submits its value, Enter submits the form, and `required`, the length and pattern limits and `setCustomValidity` take part in validation (`checkValidity`, `validity`, ...). The field is drawn 36 pt high; with `hit` it has a 44 pt high hit area (a click on the strip around it focuses it). */
    "ui-text-field": Omit<HTMLAttributes<HTMLElement>, 'oninput' | 'onchange'> & {
      /** The text. */
      value?: string
      /** A hint shown while empty. It is not the accessible name. */
      placeholder?: string
      /** `text` (default), `email`, `password`, `url`, `search`, `tel` or `number`. */
      type?: 'text' | 'email' | 'password' | 'url' | 'search' | 'tel' | 'number'
      /** The text can be selected and copied, not changed. */
      readonly?: boolean
      /** The form is invalid while the field is empty. */
      required?: boolean
      /** The `autocomplete` hint for the browser, for example `email`. */
      autocomplete?: string
      /** The on-screen keyboard to offer: `numeric`, `decimal`, `email`, `tel`, `url`, `search`, `text` or `none`. */
      inputmode?: string
      /** The longest text, in characters. */
      maxlength?: number | undefined
      /** The shortest text, in characters, when there is any. */
      minlength?: number | undefined
      /** A regular expression the whole text must match. */
      pattern?: string | undefined
      /** For `type="number"`: the smallest value. */
      min?: string | undefined
      /** For `type="number"`: the largest value. */
      max?: string | undefined
      /** For `type="number"`: the step between values, or `any`. */
      step?: string | undefined
      /** What the keyboard's return key says: `enter`, `done`, `go`, `next`, `previous`, `search` or `send`. */
      enterkeyhint?: string
      /** How the keyboard capitalises: `off`, `none`, `on`, `sentences`, `words` or `characters`. */
      autocapitalize?: string
      /** `field` (default): a rounded filled field. `bare`: no box, for use inside a `ui-row`. */
      variant?: 'field' | 'bare'
      /** Shows the field as invalid (`aria-invalid`): a red outline, or red text for a `bare` field in a row. The message is the footer of the row's `ui-section` (`footer-tone="error"`). */
      invalid?: boolean
      /** The form field name. Nothing is submitted without one. */
      name?: string
      /** A 44 x 44 pt hit area around the drawn control, as the shell's `hit` is (off by default: the control is drawn at its own size). Set it wherever nothing else gives the control a 44 pt target; a row, a banner and a toolbar item do. */
      hit?: boolean
      /** Dims the control and ignores input; also set by a disabled `<fieldset>` around it. */
      disabled?: boolean
      /** The accessible name. A `<label for>` pointing at the element names it too. */
      label?: string
      /** Read after the name. */
      description?: string
      /** The person typed. `value` is already updated (the native event, passed through). */
      oninput?: (event: InputEvent) => void
      /** The person committed the text (blur). It has no `detail`: read `value`. */
      onchange?: (event: Event) => void
    }
    /** An on/off switch (UISwitch). A form control: with a `name` it submits `value` while on. */
    "ui-toggle": Omit<HTMLAttributes<HTMLElement>, 'onchange'> & {
      /** Whether it is on. */
      checked?: boolean
      /** The value submitted while on. */
      value?: string
      /** The form is invalid until it is on. */
      required?: boolean
      /** On-state colour: any CSS colour. */
      tint?: string | undefined
      /** The form field name. Nothing is submitted without one. */
      name?: string
      /** A 44 x 44 pt hit area around the drawn control, as the shell's `hit` is (off by default: the control is drawn at its own size). Set it wherever nothing else gives the control a 44 pt target; a row, a banner and a toolbar item do. */
      hit?: boolean
      /** Dims the control and ignores input; also set by a disabled `<fieldset>` around it. */
      disabled?: boolean
      /** The accessible name. A `<label for>` pointing at the element names it too. */
      label?: string
      /** Read after the name. */
      description?: string
      /** The person flipped it. `checked` is already updated. */
      onchange?: (event: CustomEvent<{ checked: boolean }>) => void
    }
  }
}

export {}
