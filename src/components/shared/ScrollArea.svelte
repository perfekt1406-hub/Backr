<!--
  Purpose: Reusable scroll container that hides the unstyleable native scrollbar
           and overlays a custom, design-token-matched bar instead.
  Role: WebKitGTK (the Tauri Linux webview) cannot restyle native scrollbars, so
        this component draws its own slim thumb/track over a natively-scrolling
        viewport. The viewport still scrolls via wheel/trackpad/keyboard; the
        overlay only reflects and drives scroll position. Wrap any overflow pane
        with it and pass the viewport's layout classes via `viewportClass`.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * Props:
   * - children: content rendered inside the scrolling viewport.
   * - viewportClass: classes applied to the scrolling viewport itself
   *   (e.g. layout + responsive `overflow-y-auto`). `backr-scroll` is added
   *   automatically to suppress the native bar. The overlay bar lives in the
   *   rightmost ~10px, so reserve a right gutter here (e.g. `pr-3`) wherever the
   *   pane scrolls, so content isn't drawn under the thumb.
   * - class: classes for the outer positioning wrapper (it is always `relative`).
   */
  interface Props {
    children: Snippet;
    viewportClass?: string;
    class?: string;
  }

  let { children, viewportClass = "", class: className = "" }: Props = $props();

  // Vertical inset of the rail from the pane's top/bottom edges, in px. Kept in
  // sync with the `.sa-rail` top/bottom values below so thumb math stays exact.
  const PAD = 4;
  // Minimum thumb length so it stays grabbable even with very long content.
  const MIN_THUMB = 28;

  let viewportEl = $state<HTMLDivElement | null>(null);
  let thumbEl = $state<HTMLDivElement | null>(null);

  let overflowing = $state(false);
  let thumbTop = $state(0);
  let thumbHeight = $state(0);
  let dragging = $state(false);

  // Pointer-drag anchors: cursor Y and scroll offset captured at grab time.
  let dragStartY = 0;
  let dragStartScroll = 0;
  // rAF handle so scroll-driven measures coalesce to one per frame.
  let rafId = 0;

  /** Clamp `value` into the inclusive [min, max] range. */
  function clamp(value: number, min: number, max: number): number {
    return Math.min(max, Math.max(min, value));
  }

  /**
   * Recompute thumb size/position from the viewport's live scroll metrics and
   * whether the content overflows. No-op when the viewport isn't mounted.
   */
  function measure(): void {
    const vp = viewportEl;
    if (!vp) return;

    const clientH = vp.clientHeight;
    const scrollH = vp.scrollHeight;
    const scrollTop = vp.scrollTop;

    // Treat a 1px slack as "not overflowing" to avoid a flickering bar from
    // sub-pixel rounding. Below `lg` the viewport grows with content (no
    // overflow style), so this stays false and no bar is drawn.
    overflowing = scrollH - clientH > 1;
    if (!overflowing) return;

    const trackH = Math.max(0, clientH - PAD * 2);
    const ratio = clientH / scrollH; // visible fraction of the content
    const th = Math.max(MIN_THUMB, Math.round(trackH * ratio));
    const maxThumbTop = trackH - th;
    const maxScroll = scrollH - clientH;

    thumbHeight = th;
    thumbTop = maxScroll > 0 ? Math.round((scrollTop / maxScroll) * maxThumbTop) : 0;
  }

  /** Coalesce scroll events into one measure per animation frame. */
  function onScroll(): void {
    if (rafId) return;
    rafId = requestAnimationFrame(() => {
      rafId = 0;
      measure();
    });
  }

  /**
   * Translate a thumb pointer-drag into a scroll offset. The viewport's own
   * `scroll` event then re-runs `measure` to reposition the thumb.
   */
  function onThumbMove(e: PointerEvent): void {
    const vp = viewportEl;
    if (!vp) return;

    const trackH = Math.max(0, vp.clientHeight - PAD * 2);
    const maxThumbTop = trackH - thumbHeight;
    const maxScroll = vp.scrollHeight - vp.clientHeight;
    if (maxThumbTop <= 0 || maxScroll <= 0) return;

    const deltaY = e.clientY - dragStartY;
    const scrollDelta = (deltaY / maxThumbTop) * maxScroll;
    vp.scrollTop = clamp(dragStartScroll + scrollDelta, 0, maxScroll);
  }

  /** Release pointer capture and tear down the drag listeners. */
  function onThumbUp(e: PointerEvent): void {
    dragging = false;
    thumbEl?.releasePointerCapture(e.pointerId);
    thumbEl?.removeEventListener("pointermove", onThumbMove);
    thumbEl?.removeEventListener("pointerup", onThumbUp);
  }

  /** Begin a thumb drag: capture the pointer and anchor cursor/scroll origins. */
  function onThumbDown(e: PointerEvent): void {
    const vp = viewportEl;
    const thumb = thumbEl;
    if (!vp || !thumb) return;

    e.preventDefault();
    dragging = true;
    dragStartY = e.clientY;
    dragStartScroll = vp.scrollTop;
    thumb.setPointerCapture(e.pointerId);
    thumb.addEventListener("pointermove", onThumbMove);
    thumb.addEventListener("pointerup", onThumbUp);
  }

  // Wire observers once the viewport is bound: ResizeObserver catches pane/window
  // and breakpoint (absolute↔static) size changes; MutationObserver catches async
  // content loads (e.g. projects arriving from the store) that change scrollHeight.
  $effect(() => {
    const vp = viewportEl;
    if (!vp) return;

    measure();

    const resizeObserver = new ResizeObserver(() => measure());
    resizeObserver.observe(vp);
    const mutationObserver = new MutationObserver(() => measure());
    mutationObserver.observe(vp, { childList: true, subtree: true, characterData: true });
    const onWindowResize = (): void => measure();
    window.addEventListener("resize", onWindowResize);

    return () => {
      resizeObserver.disconnect();
      mutationObserver.disconnect();
      window.removeEventListener("resize", onWindowResize);
      if (rafId) cancelAnimationFrame(rafId);
    };
  });
</script>

<div class={`scroll-area relative ${className}`}>
  <div bind:this={viewportEl} class={`backr-scroll ${viewportClass}`} onscroll={onScroll}>
    {@render children()}
  </div>

  {#if overflowing}
    <!-- Decorative overlay: native scroll stays the source of truth and remains
         keyboard-accessible, so the rail is aria-hidden. -->
    <div class="sa-rail" aria-hidden="true">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- Pointer-only convenience; the native scroller below is the real,
           keyboard-accessible control, hence the aria-hidden rail. -->
      <div
        bind:this={thumbEl}
        class="sa-thumb"
        class:dragging
        style={`top:${thumbTop}px;height:${thumbHeight}px`}
        onpointerdown={onThumbDown}
      ></div>
    </div>
  {/if}
</div>

<style>
  /* Slim lane pinned to the right edge. pointer-events:none so it never steals
     clicks from content beneath it — only the thumb opts back in. */
  .sa-rail {
    position: absolute;
    top: 4px;
    right: 2px;
    bottom: 4px;
    width: 8px;
    z-index: 5;
    pointer-events: none;
  }

  /* Hairline trough, revealed only on hover for a quiet instrument-panel cue. */
  .sa-rail::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    right: 3px;
    width: 1px;
    border-radius: 9999px;
    background: var(--border);
    opacity: 0;
    transition: opacity 150ms ease;
  }
  .scroll-area:hover .sa-rail::before {
    opacity: 0.7;
  }

  /* The draggable thumb. Anchored to the right edge so it widens leftward. */
  .sa-thumb {
    position: absolute;
    right: 0;
    width: 4px;
    border-radius: 9999px;
    background: var(--border-glow);
    pointer-events: auto;
    cursor: grab;
    transition: background-color 150ms ease, width 150ms ease;
  }
  .scroll-area:hover .sa-thumb,
  .sa-thumb:hover {
    width: 6px;
    background: var(--muted2);
  }
  .sa-thumb.dragging {
    width: 6px;
    background: var(--accent);
    cursor: grabbing;
  }
</style>
