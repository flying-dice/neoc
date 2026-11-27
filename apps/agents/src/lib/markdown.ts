/**
 * What the agent writes is Markdown (GitHub's flavour): this draws it as HTML. micromark runs in its safe mode, which
 * is its default: HTML in the text is written out as text, never markup, and a link or an image whose address has a
 * protocol it does not know as safe (`javascript:`, `data:`, `vbscript:` ...) keeps its words and loses its address. Every
 * `<` of the agent's own is escaped, so each tag in what comes out is micromark's.
 */
import { micromark } from 'micromark'
import { gfm, gfmHtml } from 'micromark-extension-gfm'

/**
 * The HTML of `text`. It is drawn again from the whole text each time more of it arrives: a code fence not closed yet
 * runs to the end, as Markdown says, and is closed with it.
 */
export function markdownOf(text: string): string {
  return (
    micromark(text, { extensions: [gfm()], htmlExtensions: [gfmHtml()] })
      // a link out of the page opens outside it (the page's click handler sends it to the host), and gives it nothing of
      // the page; a link to a footnote stays in the page
      .replace(/<a href="(?!#)/g, '<a target="_blank" rel="noopener noreferrer" href="')
      // a task's box says whether it is done (micromark's has no name)
      .replace(/<input type="checkbox" disabled="" checked="" \/>/g, '<input type="checkbox" disabled="" checked="" aria-label="Done" />')
      .replace(/<input type="checkbox" disabled="" \/>/g, '<input type="checkbox" disabled="" aria-label="Not done" />')
      // a code block and a table scroll sideways rather than widen the chat, and a keyboard can reach them to scroll
      .replace(/<pre>/g, '<pre data-scrolls-x tabindex="0">')
      .replace(/<table>/g, '<div class="table" data-scrolls-x tabindex="0"><table>')
      .replace(/<\/table>/g, '</table></div>')
  )
}
