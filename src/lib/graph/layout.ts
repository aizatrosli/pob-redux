export interface LayoutNode {
  id: string;
  /** Nodes sharing a cluster start near each other. */
  cluster?: number;
  r: number;
}
export interface LayoutEdge {
  s: string;
  t: string;
  w: number;
}
export type Positions = Map<string, { x: number; y: number }>;

function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return (h >>> 0) / 4294967296;
}

/**
 * Force-directed layout, run to rest in one go. Seeded from the node ids, so
 * the same graph always lands the same way. Fine up to a few hundred nodes.
 */
export function layout(nodes: LayoutNode[], edges: LayoutEdge[], iterations = 320): Positions {
  const n = nodes.length;
  const idx = new Map(nodes.map((v, i) => [v.id, i]));
  const clusters = new Set(nodes.map((v) => v.cluster ?? 0)).size;
  const span = 60 * Math.sqrt(n + 1);
  const x = new Float64Array(n);
  const y = new Float64Array(n);
  nodes.forEach((v, i) => {
    const a = clusters > 1 ? (((v.cluster ?? 0) % clusters) / clusters) * Math.PI * 2 : hash(v.id) * Math.PI * 2;
    const base = clusters > 1 ? span * 0.6 : span * 0.5 * hash(v.id + "r");
    x[i] = Math.cos(a) * base + (hash(v.id + "x") - 0.5) * span * 0.35;
    y[i] = Math.sin(a) * base + (hash(v.id + "y") - 0.5) * span * 0.35;
  });
  const links = edges.map((e) => [idx.get(e.s), idx.get(e.t), e.w] as const).filter((l): l is readonly [number, number, number] => l[0] != null && l[1] != null && l[0] !== l[1]);
  const k = 70;
  const dx = new Float64Array(n);
  const dy = new Float64Array(n);
  for (let it = 0; it < iterations; it++) {
    const temp = span * 0.12 * (1 - it / iterations) + 0.5;
    dx.fill(0);
    dy.fill(0);
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        let ex = x[i] - x[j];
        let ey = y[i] - y[j];
        let d2 = ex * ex + ey * ey;
        if (d2 < 0.01) {
          ex = hash(nodes[i].id + j) - 0.5;
          ey = hash(nodes[j].id + i) - 0.5;
          d2 = 0.01;
        }
        const f = (k * k) / d2;
        dx[i] += ex * f;
        dy[i] += ey * f;
        dx[j] -= ex * f;
        dy[j] -= ey * f;
      }
      dx[i] -= x[i] * 0.02;
      dy[i] -= y[i] * 0.02;
    }
    for (const [a, b, w] of links) {
      const ex = x[a] - x[b];
      const ey = y[a] - y[b];
      const d = Math.sqrt(ex * ex + ey * ey) || 0.1;
      const f = ((d * d) / k) * (0.35 + 0.65 * Math.min(1, w)) * 0.5;
      dx[a] -= (ex / d) * f;
      dy[a] -= (ey / d) * f;
      dx[b] += (ex / d) * f;
      dy[b] += (ey / d) * f;
    }
    for (let i = 0; i < n; i++) {
      const d = Math.sqrt(dx[i] * dx[i] + dy[i] * dy[i]) || 1;
      const step = Math.min(d, temp);
      x[i] += (dx[i] / d) * step;
      y[i] += (dy[i] / d) * step;
    }
  }
  return new Map(nodes.map((v, i) => [v.id, { x: x[i], y: y[i] }]));
}
