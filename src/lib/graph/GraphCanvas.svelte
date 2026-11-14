<script lang="ts" module>
  export interface DrawNode {
    id: string;
    label: string;
    /** A CSS colour or custom property name (`--c-unique`). */
    colour: string;
    r: number;
    shape: "circle" | "diamond";
    cluster?: number;
  }
  export interface DrawEdge {
    s: string;
    t: string;
    w: number;
    /** Drawn with an arrow head at `t`. */
    directed?: boolean;
  }
</script>

<script lang="ts">
  import { layout } from "./layout";

  let {
    nodes,
    edges,
    highlight = null,
    selected = null,
    onpick,
  }: {
    nodes: DrawNode[];
    edges: DrawEdge[];
    /** Ids to keep lit; everything else fades. Null lights everything. */
    highlight?: Set<string> | null;
    selected?: string | null;
    onpick: (id: string | null) => void;
  } = $props();

  let canvas = $state<HTMLCanvasElement>();
  let wrap = $state<HTMLDivElement>();
  let size = $state({ w: 0, h: 0 });
  let view = $state({ x: 0, y: 0, k: 1 });
  let hover = $state<string | null>(null);
  let fitted = "";

  const pos = $derived(layout(nodes.map((n) => ({ id: n.id, cluster: n.cluster, r: n.r })), edges));
  const byId = $derived(new Map(nodes.map((n) => [n.id, n])));

  function fit() {
    if (!nodes.length || !size.w) return;
    let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
    for (const p of pos.values()) {
      x0 = Math.min(x0, p.x);
      x1 = Math.max(x1, p.x);
      y0 = Math.min(y0, p.y);
      y1 = Math.max(y1, p.y);
    }
    const k = Math.min(2, Math.min(size.w / (x1 - x0 + 160), size.h / (y1 - y0 + 120)));
    view = { k, x: size.w / 2 - ((x0 + x1) / 2) * k, y: size.h / 2 - ((y0 + y1) / 2) * k };
  }

  $effect(() => {
    if (!wrap) return;
    const ro = new ResizeObserver(() => {
      size = { w: wrap!.clientWidth, h: wrap!.clientHeight };
    });
    ro.observe(wrap);
    return () => ro.disconnect();
  });

  // refit when the graph itself changes, not on every pan
  $effect(() => {
    const key = `${nodes.length}:${nodes[0]?.id ?? ""}:${nodes[nodes.length - 1]?.id ?? ""}:${size.w > 0}`;
    if (size.w && key !== fitted) {
      fitted = key;
      fit();
    }
  });

  function colour(css: CSSStyleDeclaration, c: string) {
    return c.startsWith("--") ? css.getPropertyValue(c).trim() || "#888" : c;
  }

  $effect(() => {
    const c = canvas;
    if (!c || !size.w) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = Math.round(size.w * dpr);
    c.height = Math.round(size.h * dpr);
    const g = c.getContext("2d")!;
    const css = getComputedStyle(c);
    const fg = css.getPropertyValue("--fg-0").trim() || "#eee";
    const line = css.getPropertyValue("--fg-3").trim() || "#777";
    const accent = css.getPropertyValue("--focus").trim() || "#7f9dff";
    const bg = css.getPropertyValue("--bg-0").trim() || "#111";
    const { x, y, k } = view;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, size.w, size.h);
    g.translate(x, y);
    g.scale(k, k);
    const lit = (id: string) => !highlight || highlight.has(id);

    for (const e of edges) {
      const a = pos.get(e.s);
      const b = pos.get(e.t);
      if (!a || !b) continue;
      const on = lit(e.s) && lit(e.t);
      g.globalAlpha = on ? (highlight ? 0.9 : 0.28 + 0.3 * Math.min(1, e.w)) : 0.05;
      g.strokeStyle = on && highlight ? accent : line;
      g.lineWidth = (on && highlight ? 1.8 : 0.6 + Math.min(1, e.w)) / Math.sqrt(k);
      g.beginPath();
      g.moveTo(a.x, a.y);
      g.lineTo(b.x, b.y);
      g.stroke();
      if (e.directed && (on || !highlight)) {
        const tr = byId.get(e.t)?.r ?? 6;
        const ang = Math.atan2(b.y - a.y, b.x - a.x);
        const hx = b.x - Math.cos(ang) * (tr + 2);
        const hy = b.y - Math.sin(ang) * (tr + 2);
        const s = 6 / Math.sqrt(k);
        g.fillStyle = g.strokeStyle;
        g.beginPath();
        g.moveTo(hx, hy);
        g.lineTo(hx - Math.cos(ang - 0.4) * s, hy - Math.sin(ang - 0.4) * s);
        g.lineTo(hx - Math.cos(ang + 0.4) * s, hy - Math.sin(ang + 0.4) * s);
        g.fill();
      }
    }

    const labelled: DrawNode[] = [];
    for (const n of nodes) {
      const p = pos.get(n.id);
      if (!p) continue;
      const on = lit(n.id);
      g.globalAlpha = on ? 1 : 0.12;
      g.fillStyle = colour(css, n.colour);
      g.beginPath();
      if (n.shape === "diamond") {
        g.moveTo(p.x, p.y - n.r * 1.25);
        g.lineTo(p.x + n.r * 1.25, p.y);
        g.lineTo(p.x, p.y + n.r * 1.25);
        g.lineTo(p.x - n.r * 1.25, p.y);
        g.closePath();
      } else {
        g.arc(p.x, p.y, n.r, 0, Math.PI * 2);
      }
      g.fill();
      if (n.id === selected || n.id === hover) {
        g.strokeStyle = fg;
        g.lineWidth = 2 / k;
        g.stroke();
      }
      const always = n.id === selected || n.id === hover || (highlight ? on && highlight.size <= 40 : n.r * k >= 7.5);
      if (always) labelled.push(n);
    }

    g.globalAlpha = 1;
    const fs = Math.max(10, 11.5 / k);
    g.font = `${fs}px ${css.getPropertyValue("--font-ui") || "sans-serif"}`;
    g.textBaseline = "middle";
    g.lineJoin = "round";
    for (const n of labelled) {
      const p = pos.get(n.id)!;
      g.lineWidth = 3 / k;
      g.strokeStyle = bg;
      g.strokeText(n.label, p.x + n.r + 4 / k, p.y);
      g.fillStyle = fg;
      g.fillText(n.label, p.x + n.r + 4 / k, p.y);
    }
  });

  function at(e: MouseEvent): string | null {
    const rect = canvas!.getBoundingClientRect();
    const gx = (e.clientX - rect.left - view.x) / view.k;
    const gy = (e.clientY - rect.top - view.y) / view.k;
    let best: string | null = null;
    let bestD = Infinity;
    for (const n of nodes) {
      const p = pos.get(n.id);
      if (!p) continue;
      const d = Math.hypot(p.x - gx, p.y - gy);
      if (d <= n.r + 5 / view.k && d < bestD) {
        best = n.id;
        bestD = d;
      }
    }
    return best;
  }

  let drag: { x: number; y: number; vx: number; vy: number; moved: boolean } | null = null;

  function onDown(e: PointerEvent) {
    drag = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y, moved: false };
    canvas!.setPointerCapture(e.pointerId);
  }
  function onMove(e: PointerEvent) {
    if (drag) {
      const dx = e.clientX - drag.x;
      const dy = e.clientY - drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
      if (drag.moved) view = { ...view, x: drag.vx + dx, y: drag.vy + dy };
    } else {
      hover = at(e);
    }
  }
  function onUp(e: PointerEvent) {
    if (drag && !drag.moved) onpick(at(e));
    drag = null;
  }
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const rect = canvas!.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const k = Math.min(6, Math.max(0.08, view.k * Math.exp(-e.deltaY * 0.0015)));
    view = { k, x: mx - ((mx - view.x) / view.k) * k, y: my - ((my - view.y) / view.k) * k };
  }

  export function reset() {
    fit();
  }
</script>

<div class="wrap" bind:this={wrap}>
  <canvas
    bind:this={canvas}
    style:width="{size.w}px"
    style:height="{size.h}px"
    style:cursor={hover ? "var(--cursor-pointer, pointer)" : "grab"}
    onpointerdown={onDown}
    onpointermove={onMove}
    onpointerup={onUp}
    onpointerleave={() => (hover = null)}
    onwheel={onWheel}
  ></canvas>
</div>

<style>
  .wrap {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--bg-0);
  }
  canvas {
    display: block;
    touch-action: none;
  }
</style>
