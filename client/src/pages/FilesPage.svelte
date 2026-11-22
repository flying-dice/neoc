<!--
  @page FilesPage — the host's files, for the person to look through and change. The places first: Home and This
  Computer (the host's own file system, all of it, when the host lets it be browsed), then the installed apps (each
  app's Documents). Inside a place: its folders and files with their size and when they changed. A file row downloads
  the file, a folder row opens it, Back goes up (on This Computer all the way to the top of the disk), and a path
  typed or pasted into the path field goes straight there. The bin beside a row removes it after the alert is
  confirmed (for a folder the alert says how much is in it). New Folder names a folder in a small form; Upload (or
  files dropped on the list) sends files into the open folder, each with its progress, and one whose name is taken
  asks: replace, keep both, or cancel. The search field filters what is on screen by name. One column at every
  width, so it is the same page in the smallest window.
  State: `FileBrowser` (lib/files.svelte.ts), one per window.
  Test ids: `files-nav` (`files-nav-back`), `files-search`, `files-place-home`, `files-place-computer`,
  `files-app-<id>`, `files-path` (`files-path-problem`), `files-new-folder`, `files-upload`, `files-upload-input`,
  `files-folder-name`, `files-folder-create`, `files-folder-cancel`, `files-folder-problem`, `files-uploads`,
  `files-upload-<name>`, `files-uploads-dismiss`, `files-conflict-alert`, `files-entry-<name>`,
  `files-entry-<name>-remove`, `files-remove-alert`, `files-truncated`, `files-loading`, `files-empty`,
  `files-apps-empty`, `files-no-results`, `files-error`, `files-retry`, `files-remove-error`.
-->
<script lang="ts">
  import { untrack } from 'svelte'
  import { ActivityIndicator, Button, Icon, Text, TextInput } from '@vos/ui/atoms'
  import { Alert, ListRow, SearchField } from '@vos/ui/molecules'
  import { ListSection, NavigationBar } from '@vos/ui/organisms'
  import { ScreenTemplate } from '@vos/ui/templates'
  import { FileBrowser, type AppFolder, type RemoveRequest, type Upload } from '../lib/files.svelte'
  import { host } from '../lib/host.svelte'
  import { fileSize, iconOf, tintBackground, type FileEntry } from '../shell/hostApi'
  import { uploader } from '../shell/upload'

  const browser = new FileBrowser(host.api, uploader)

  // the installed apps as the host lists them now: an install or an uninstall anywhere reads them again
  $effect(() => {
    const apps = host.apps
    untrack(() => void browser.setApps(apps))
  })

  const when = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })
  const onComputer = $derived(browser.place?.kind === 'computer')

  function kept(folder: AppFolder): string {
    if (folder.count === null) return 'Could not be read'
    if (folder.count === 0) return 'No documents'
    return folder.count === 1 ? '1 item' : `${folder.count} items`
  }

  /**
   * What a row says under the name: why it cannot be opened, or a file's size and when it last changed. A folder says
   * only that it is a link, when it is one (its size is the directory entry's, not its contents').
   */
  function details(entry: FileEntry): string | undefined {
    if (entry.problem) return entry.problem
    const link = entry.link ? 'Link' : undefined
    if (entry.kind !== 'file') return link
    const size = fileSize(entry.size)
    const text =
      entry.modifiedAt === undefined ? size : `${size} · ${when.format(entry.modifiedAt)}`
    return link ? `${link} · ${text}` : text
  }

  /** Save a file: the host answers with an attachment, so the browser downloads it and the shell stays where it is. */
  function download(entry: FileEntry): void {
    const link = document.createElement('a')
    link.href = browser.downloadUrl(entry)
    link.download = entry.name
    link.click()
  }

  function activate(entry: FileEntry): (() => void) | undefined {
    if (entry.problem) return undefined
    if (entry.kind === 'dir') return () => void browser.enter(entry.name)
    if (entry.kind === 'file') return () => download(entry)
    return undefined
  }

  /** Where the open folder is, for the section's header: `Agents / transcripts`. */
  const trail = $derived(
    browser.place?.kind === 'app' ? [browser.place.app.name, ...browser.path].join(' / ') : ''
  )
  const searching = $derived(browser.query.trim() !== '')

  // ── The path field (This Computer) ──────────────────────────────────
  // the field shows the open folder, until the person types in it
  let typedPath = $derived(browser.pathText)
  function pathKey(event: KeyboardEvent): void {
    if (event.key === 'Enter') void browser.goTo(typedPath)
  }

  // ── Uploads ─────────────────────────────────────────────────────────
  let picker: HTMLInputElement | undefined = $state()
  let dragging = $state(false)

  const send = (files: FileList | null | undefined): void => {
    if (!files?.length) return
    void browser.upload(
      [...files].map((file) => ({ name: file.name, size: file.size, blob: file }))
    )
  }
  function picked(): void {
    send(picker?.files)
    if (picker) picker.value = '' // the same file can be picked again
  }
  const carriesFiles = (event: DragEvent): boolean =>
    !!browser.place && !!event.dataTransfer?.types.includes('Files')
  function dragOver(event: DragEvent): void {
    if (!carriesFiles(event)) return
    event.preventDefault()
    dragging = true
  }
  function drop(event: DragEvent): void {
    if (!carriesFiles(event)) return
    event.preventDefault()
    dragging = false
    send(event.dataTransfer?.files)
  }

  function progress(upload: Upload): string {
    if (upload.status === 'done') return 'Uploaded'
    if (upload.status === 'failed') return upload.error ?? 'Failed'
    if (upload.status === 'conflict') return 'Waiting for your answer'
    if (upload.status === 'waiting') return 'Waiting'
    return `${Math.round(upload.progress * 100)}% of ${fileSize(upload.size)}`
  }
  const settled = $derived(
    browser.uploads.some((u) => u.status === 'done' || u.status === 'failed')
  )

  // ── Removing ────────────────────────────────────────────────────────
  const placeName = $derived(
    browser.place?.kind === 'app' ? `${browser.place.app.name}’s documents` : 'this computer'
  )
  function warning(request: RemoveRequest): string {
    const { entry, inside, more } = request
    if (entry.link) return `Only the link is deleted from ${placeName}, not what it leads to.`
    if (entry.kind !== 'dir') return `It will be deleted from ${placeName}. This cannot be undone.`
    let contents = 'everything in it'
    if (inside === 0) contents = 'nothing: it is empty'
    else if (inside !== null)
      contents = `the ${more ? 'more than ' : ''}${inside} ${inside === 1 ? 'item' : 'items'} in it, and everything inside ${inside === 1 ? 'it' : 'them'}`
    return inside === 0
      ? `The folder is empty. It will be deleted from ${placeName}.`
      : `The folder will be deleted from ${placeName} with ${contents}. This cannot be undone.`
  }
</script>

<ScreenTemplate>
  {#snippet navigationBar()}
    <NavigationBar
      testid="files-nav"
      title={browser.title}
      large={!browser.place}
      backLabel={browser.place ? browser.parentTitle : undefined}
      onback={() => void browser.back()}
    >
      {#snippet trailing()}
        {#if browser.place}
          <Button
            testid="files-new-folder"
            variant="plain"
            square
            hit
            aria-label="New Folder"
            onclick={() => browser.startFolder()}
          >
            <Icon name="plus" size={19} weight={2.2} />
          </Button>
          <Button
            testid="files-upload"
            variant="plain"
            square
            hit
            aria-label="Upload"
            onclick={() => picker?.click()}
          >
            <Icon name="square.and.arrow.up" size={19} weight={2.2} />
          </Button>
        {/if}
      {/snippet}
      {#snippet accessory()}
        <SearchField testid="files-search" bind:value={browser.query} />
      {/snippet}
    </NavigationBar>
  {/snippet}

  {#snippet content()}
    <!-- svelte-ignore a11y_no_static_element_interactions (a drop target for files; Upload is the same thing for the keyboard) -->
    <div
      class="body"
      class:dragging
      ondragover={dragOver}
      ondragleave={() => (dragging = false)}
      ondrop={drop}
    >
      <input
        class="picker"
        type="file"
        multiple
        tabindex="-1"
        aria-hidden="true"
        data-testid="files-upload-input"
        bind:this={picker}
        onchange={picked}
      />

      {#if onComputer}
        <div class="path">
          <TextInput
            testid="files-path"
            bind:value={typedPath}
            aria-label="Path"
            autocapitalize="off"
            autocomplete="off"
            spellcheck="false"
            enterkeyhint="go"
            onkeydown={pathKey}
          />
          {#if browser.pathProblem}
            <p class="note bad" role="alert" data-testid="files-path-problem">
              {browser.pathProblem}
            </p>
          {/if}
        </div>
      {/if}

      {#if browser.error}
        <div class="problem" role="alert" data-testid="files-error">
          <p>{browser.error}</p>
          <Button
            testid="files-retry"
            variant="plain"
            size="small"
            onclick={() => void browser.refresh()}>Try Again</Button
          >
        </div>
      {/if}
      {#if browser.removeError}
        <div class="problem" role="alert" data-testid="files-remove-error">
          <p>{browser.removeError}</p>
        </div>
      {/if}

      {#if browser.place && browser.newFolder !== null}
        <ListSection header="New Folder">
          <div class="form">
            <TextInput
              testid="files-folder-name"
              value={browser.newFolder}
              oninput={(e) => browser.nameFolder(e.currentTarget.value)}
              onkeydown={(e) => {
                if (e.key === 'Enter') void browser.createFolder()
                if (e.key === 'Escape') browser.cancelFolder()
              }}
              placeholder="Name"
              aria-label="Folder name"
              aria-invalid={browser.folderProblem !== null}
              autocapitalize="off"
              autocomplete="off"
              maxlength={255}
            />
            <Button
              testid="files-folder-cancel"
              variant="plain"
              size="small"
              onclick={() => browser.cancelFolder()}>Cancel</Button
            >
            <Button
              testid="files-folder-create"
              size="small"
              disabled={!browser.canCreateFolder}
              onclick={() => void browser.createFolder()}>Create</Button
            >
          </div>
        </ListSection>
        {#if browser.folderProblem}
          <p class="note bad lifted" role="alert" data-testid="files-folder-problem">
            {browser.folderProblem}
          </p>
        {/if}
      {/if}

      {#if browser.uploads.length > 0}
        <div data-testid="files-uploads">
          <ListSection header="Uploads">
            {#each browser.uploads as upload (upload.id)}
              <ListRow
                testid="files-upload-{upload.name}"
                title={upload.name}
                subtitle={progress(upload)}
                icon={upload.status === 'failed'
                  ? 'exclamationmark.triangle'
                  : upload.status === 'done'
                    ? 'checkmark'
                    : 'arrow.up'}
                iconColor={upload.status === 'failed'
                  ? 'var(--ios-red)'
                  : upload.status === 'done'
                    ? 'var(--ios-green)'
                    : 'var(--ios-blue)'}
              />
              {#if upload.status === 'uploading'}
                <div class="bar" aria-hidden="true">
                  <div class="sent" style:width="{Math.round(upload.progress * 100)}%"></div>
                </div>
              {/if}
            {/each}
          </ListSection>
          {#if settled}
            <div class="lifted end">
              <Button
                testid="files-uploads-dismiss"
                variant="plain"
                size="small"
                onclick={() => browser.dismissUploads()}>Clear Finished</Button
              >
            </div>
          {/if}
        </div>
      {/if}

      {#if !browser.place}
        {#if browser.showsHome || browser.showsComputer}
          <ListSection header="Locations">
            {#if browser.showsHome && browser.home}
              {@const home = browser.home}
              <ListRow
                testid="files-place-home"
                title="Home"
                subtitle="The home folder of the user running this host"
                icon="house"
                iconColor="var(--ios-blue)"
                chevron
                onclick={() => void browser.open({ kind: 'computer' }, home)}
              />
            {/if}
            {#if browser.showsComputer}
              <ListRow
                testid="files-place-computer"
                title="This Computer"
                subtitle="Every file on the host"
                icon="cpu"
                iconColor="var(--ios-gray)"
                chevron
                onclick={() => void browser.open({ kind: 'computer' })}
              />
            {/if}
          </ListSection>
        {/if}
        {#if browser.folders.length > 0}
          {#if browser.visibleFolders.length > 0}
            <ListSection
              header="Apps"
              footer="The documents each app keeps on this host. An app's cache and temporary files are not shown."
            >
              {#each browser.visibleFolders as folder (folder.app.id)}
                <ListRow
                  testid="files-app-{folder.app.id}"
                  title={folder.app.name}
                  subtitle={kept(folder)}
                  icon={iconOf(folder.app.icon)}
                  iconColor={tintBackground(folder.app.tint)}
                  chevron
                  onclick={() => void browser.open({ kind: 'app', app: folder.app })}
                />
              {/each}
            </ListSection>
          {:else if !browser.showsHome && !browser.showsComputer}
            <ListSection>
              <ListRow testid="files-no-results" title="Nothing matches “{browser.query.trim()}”" />
            </ListSection>
          {/if}
        {:else if browser.loading}
          <div class="state" data-testid="files-loading">
            <ActivityIndicator label="Loading files" />
          </div>
        {:else if !browser.error && !browser.computer}
          <div class="state" data-testid="files-apps-empty">
            <Icon name="folder" size={40} />
            <Text style="headline" as="p">No apps installed</Text>
            <Text style="subheadline" color="secondary" as="p"
              >Apps you install keep their documents here.</Text
            >
          </div>
        {/if}
      {:else if browser.entries.length > 0}
        <ListSection header={trail || undefined}>
          {#each browser.visibleEntries as entry (entry.name)}
            <div class="entry">
              <ListRow
                testid="files-entry-{entry.name}"
                title={entry.name}
                subtitle={details(entry)}
                icon={entry.problem
                  ? 'exclamationmark.triangle'
                  : entry.kind === 'dir'
                    ? 'folder'
                    : 'doc'}
                iconColor={entry.problem
                  ? 'var(--ios-gray)'
                  : entry.kind === 'dir'
                    ? 'var(--ios-blue)'
                    : 'var(--ios-teal)'}
                chevron={entry.kind === 'dir' && !entry.problem}
                onclick={activate(entry)}
              />
              <Button
                testid="files-entry-{entry.name}-remove"
                variant="plain"
                square
                hit
                destructive
                aria-label="Remove {entry.name}"
                onclick={() => void browser.ask(entry)}
              >
                <Icon name="trash" size={18} weight={2.2} />
              </Button>
            </div>
          {:else}
            <ListRow
              testid="files-no-results"
              title="Nothing here matches “{browser.query.trim()}”"
            />
          {/each}
        </ListSection>
        {#if browser.truncated}
          <p class="note lifted" role="status" data-testid="files-truncated">
            This folder holds more than the {browser.entries.length.toLocaleString()} entries shown. Go
            to one of the others by its path, or search by name among these.
          </p>
        {/if}
      {:else if browser.loading}
        <div class="state" data-testid="files-loading">
          <ActivityIndicator label="Loading files" />
        </div>
      {:else if !browser.error}
        <div class="state" data-testid="files-empty">
          <Icon name="folder" size={40} />
          <Text style="headline" as="p"
            >{browser.path.length > 0 || onComputer ? 'This folder is empty' : 'No documents'}</Text
          >
          <Text style="subheadline" color="secondary" as="p">
            {#if searching}
              There is nothing here to search.
            {:else if browser.path.length > 0 || onComputer}
              Nothing is kept in {browser.title}. Upload a file, or drop one here.
            {:else}
              {browser.placeName} has not saved any files.
            {/if}
          </Text>
        </div>
      {/if}
    </div>

    {#if browser.asked}
      {@const request = browser.asked}
      <Alert
        testid="files-remove-alert"
        bind:open={
          () => browser.asked !== null,
          (open) => {
            if (!open) browser.asked = null
          }
        }
        title="Remove “{request.entry.name}”?"
        message={warning(request)}
        actions={[
          { label: 'Cancel', style: 'cancel' },
          {
            label: 'Remove',
            style: 'destructive',
            onclick: () => void browser.remove(request.entry)
          }
        ]}
      />
    {:else if browser.conflict}
      {@const upload = browser.conflict}
      {#key upload.id}
        <Alert
          testid="files-conflict-alert"
          open
          title="“{upload.name}” is already here"
          message="Replace the file that is there, or keep both? The new one would get another name."
          actions={[
            {
              label: 'Replace',
              style: 'destructive',
              onclick: () => void browser.resolveConflict(upload.id, 'replace')
            },
            { label: 'Keep Both', onclick: () => void browser.resolveConflict(upload.id, 'keep') },
            {
              label: 'Cancel',
              style: 'cancel',
              onclick: () => void browser.resolveConflict(upload.id, 'cancel')
            }
          ]}
        />
      {/key}
    {/if}
  {/snippet}
</ScreenTemplate>

<style>
  .body {
    padding-top: 8px;
    min-height: 100%;
    box-sizing: border-box;
  }
  /* files held over the list: the whole page says it will take them */
  .dragging {
    outline: 2px dashed var(--tint);
    outline-offset: -6px;
    border-radius: var(--radius-md);
  }
  .picker {
    display: none;
  }
  /* the path field: a field to type in, drawn like the search field above it */
  .path {
    margin: 0 var(--list-inset, 20px) 16px;
    padding: 4px 10px;
    border-radius: 10px;
    background: var(--fill-tertiary);
  }
  /* a row and its bin: the row takes what is left and shortens its name, the bin keeps its size */
  .entry {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    padding-right: 4px;
  }
  .form {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 8px;
    padding: 8px var(--row-inset);
  }
  .bar {
    height: 3px;
    margin: 0 var(--row-inset) 6px;
    border-radius: 2px;
    background: var(--fill-secondary);
    overflow: hidden;
  }
  .sent {
    height: 100%;
    background: var(--tint);
  }
  .note {
    margin: 6px 0 0;
    padding: 0 var(--row-inset);
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    color: var(--label-secondary);
    overflow-wrap: anywhere;
  }
  .note.bad {
    color: var(--ios-red);
  }
  /* under a list section, whose own margin is for the next section: pulled up to belong to it */
  .lifted {
    margin: -28px 0 28px;
    padding: 0 calc(var(--list-inset, 20px) + var(--row-inset));
  }
  .end {
    display: flex;
    justify-content: flex-end;
  }
  .problem {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 4px 12px;
    margin: 0 0 16px;
    /* in line with the text of the rows under it */
    padding: 0 calc(var(--list-inset, 20px) + var(--row-inset));
  }
  .problem p {
    margin: 0;
    min-width: 0;
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    color: var(--ios-red);
    overflow-wrap: anywhere;
  }
  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 48px 24px;
    text-align: center;
    color: var(--label-secondary);
  }
</style>
