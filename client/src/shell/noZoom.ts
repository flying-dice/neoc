/**
 * A host page is an app's surface, not a document to zoom: pinching and double-tapping do not scale it. Most of this is
 * CSS (`touch-action` on the root); Safari also has gesture events of its own that scale a page whatever the CSS says,
 * and a trackpad or `Ctrl` + wheel pinch, which are refused here. Keyboard zoom (`Ctrl` and `+`) is the browser's and
 * stays, so text can still be made larger.
 */
export function preventZoom(target: Document = document): void {
  target.documentElement.style.touchAction = 'pan-x pan-y'
  const refuse = (event: Event): void => event.preventDefault()
  for (const gesture of ['gesturestart', 'gesturechange', 'gestureend'])
    target.addEventListener(gesture, refuse)
  // a pinch on a trackpad arrives as a wheel event with `ctrlKey`
  target.addEventListener(
    'wheel',
    (event) => {
      if (event.ctrlKey) event.preventDefault()
    },
    { passive: false }
  )
}
