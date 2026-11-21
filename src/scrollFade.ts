// Scrollers that show there is more to see: edge classes fade any side with hidden content (see
// `.scroll-x` in styles.css), since macOS hides scrollbars at rest. `more-start`/`more-end` are
// left and right, `more-top`/`more-bottom` up and down.

/** Keep `el`'s edge classes current as it scrolls or resizes; `onChange` hears whether it
 * overflows sideways. Returns a function that stops. */
export function trackOverflow(el: HTMLElement, onChange?: (overflowsX: boolean) => void): () => void {
  let last: boolean | null = null;
  const update = () => {
    const maxX = el.scrollWidth - el.clientWidth;
    const maxY = el.scrollHeight - el.clientHeight;
    el.classList.toggle('more-start', maxX > 1 && el.scrollLeft > 1);
    el.classList.toggle('more-end', maxX > 1 && el.scrollLeft < maxX - 1);
    el.classList.toggle('more-top', maxY > 1 && el.scrollTop > 1);
    el.classList.toggle('more-bottom', maxY > 1 && el.scrollTop < maxY - 1);
    const overflowsX = maxX > 1;
    if (overflowsX !== last) onChange?.((last = overflowsX));
  };
  const resize = new ResizeObserver(update);
  resize.observe(el);
  for (const child of el.children) resize.observe(child);
  el.addEventListener('scroll', update, { passive: true });
  update();
  return () => {
    resize.disconnect();
    el.removeEventListener('scroll', update);
  };
}

/** Scroll `target` into view inside `scroller` only (never the page), with `margin` px to spare. */
export function revealIn(scroller: HTMLElement, target: HTMLElement, margin = 24) {
  const outer = scroller.getBoundingClientRect();
  const inner = target.getBoundingClientRect();
  const delta = (start: number, end: number, viewStart: number, viewEnd: number) =>
    end - start > viewEnd - viewStart - 2 * margin ? start - viewStart - margin
      : start < viewStart + margin ? start - viewStart - margin
      : end > viewEnd - margin ? end - viewEnd + margin
      : 0;
  const left = delta(inner.left, inner.right, outer.left, outer.right);
  const top = delta(inner.top, inner.bottom, outer.top, outer.bottom);
  if (left || top) {
    const smooth = !matchMedia('(prefers-reduced-motion: reduce)').matches;
    scroller.scrollBy({ left, top, behavior: smooth ? 'smooth' : 'auto' });
  }
}
