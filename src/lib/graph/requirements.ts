import { GRAPH_FORMAT, GRAPH_VERSION, type CheckStatus, type GraphChain, type GraphDoc, type GraphRequirements } from "./types";

/** One line of the gate. `id` picks the label; `values` fill its detail message. */
export interface GateCheck {
  id:
    | "file"
    | "format"
    | "game"
    | "export"
    | "age"
    | "ladder"
    | "scope_ascendancy"
    | "scope_damage"
    | "scope_constraints"
    | "scope_uniques"
    | "budget"
    | "matches";
  status: CheckStatus;
  values?: Record<string, string | number>;
  /** exilegen's own check lines, shown under the export line. */
  notes?: { status: CheckStatus; text: string }[];
}

const STALE_DAYS = 30;

function names(doc: GraphDoc, kind: string): Set<string> {
  return new Set((doc.mechanics?.nodes ?? []).filter((n) => n.kind === kind).map((n) => n.name));
}

export function chainMatches(c: GraphChain, req: GraphRequirements, doc: GraphDoc): boolean {
  const ascs = new Set(c.asc_nodes.map(([a]) => a));
  if (req.ascendancy && ascs.size && !ascs.has(req.ascendancy)) return false;
  if (req.cls && !req.ascendancy && ascs.size) {
    const cls = new Map((doc.catalog?.ascendancies ?? []).map((a) => [a.name, a.class]));
    if (![...ascs].some((a) => cls.get(a) === req.cls)) return false;
  }
  if (req.damage_types.length && !c.main_damage_types.some((d) => req.damage_types.includes(d))) return false;
  if (req.constraints.includes("no_cast:strict") && c.main_spell) return false;
  if (req.constraints.includes("no_minions") && c.main_minion) return false;
  if (req.chain_kinds.length && !req.chain_kinds.includes(c.kind)) return false;
  if (c.novelty < req.min_novelty) return false;
  if (c.uniques.some((u) => req.forbid_uniques.includes(u))) return false;
  const wantU = req.uniques.map((u) => u.name);
  if ((wantU.length || req.keystones.length) && !c.uniques.some((u) => wantU.includes(u)) && !c.keystones.some((k) => req.keystones.includes(k))) return false;
  return true;
}

export function matchingChains(doc: GraphDoc | null, req: GraphRequirements): GraphChain[] {
  if (!doc?.mechanics) return [];
  return doc.mechanics.chains.filter((c) => chainMatches(c, req, doc)).slice(0, req.top);
}

/**
 * The gate in front of the analysis. The app can narrow a file but not widen
 * it, so a file exported for a narrower request than this one fails.
 */
export function gate(doc: GraphDoc | null, req: GraphRequirements, game: string, now = Date.now()): GateCheck[] {
  if (!doc) return [{ id: "file", status: "fail" }];
  const out: GateCheck[] = [{ id: "file", status: "ok" }];
  const formatOk = doc.format === GRAPH_FORMAT && doc.version === GRAPH_VERSION;
  out.push({ id: "format", status: formatOk ? "ok" : "fail", values: { format: String(doc.format), version: String(doc.version) } });
  if (!formatOk) return out;
  out.push({ id: "game", status: doc.game === game ? "ok" : "fail", values: { file: doc.game, app: game } });
  const notes = (doc.preflight ?? []).filter((c) => c.status !== "ok").map((c) => ({ status: c.status, text: `${c.label}: ${c.detail}` }));
  const exported = doc.passed && !!doc.mechanics;
  out.push({ id: "export", status: !exported ? "fail" : notes.length ? "warn" : "ok", notes });
  if (!exported) return out;
  const days = Math.floor((now - Date.parse(doc.generated_at)) / 86_400_000);
  out.push({ id: "age", status: Number.isFinite(days) && days <= STALE_DAYS ? "ok" : "warn", values: { days: Number.isFinite(days) ? days : "?" } });
  out.push({ id: "ladder", status: doc.ladder ? "ok" : "warn" });

  const file = doc.requirements;
  const ascOk = !file.ascendancy || file.ascendancy === req.ascendancy;
  const clsOk = !file.cls || file.cls === req.cls || (!!req.ascendancy && doc.catalog?.ascendancies.find((a) => a.name === req.ascendancy)?.class === file.cls);
  out.push({ id: "scope_ascendancy", status: ascOk && clsOk ? "ok" : "fail", values: { file: [file.cls, file.ascendancy].filter(Boolean).join(" / ") } });
  const damageOk = !file.damage_types.length || (req.damage_types.length > 0 && req.damage_types.every((d) => file.damage_types.includes(d)));
  out.push({ id: "scope_damage", status: damageOk ? "ok" : "fail", values: { file: file.damage_types.join(", ") } });
  const constraintsOk = file.constraints.every((c) => req.constraints.includes(String(c)));
  out.push({ id: "scope_constraints", status: constraintsOk ? "ok" : "fail", values: { file: file.constraints.join(", ") } });
  const known = names(doc, "unique");
  const keys = names(doc, "keystone");
  const missing = [...req.uniques.map((u) => u.name).filter((u) => !known.has(u)), ...req.keystones.filter((k) => !keys.has(k))];
  out.push({ id: "scope_uniques", status: missing.length ? "fail" : "ok", values: { missing: missing.join(", ") } });
  // the weights in the file were worked out for the file's budget
  const weighted = doc.mechanics?.chains.some((c) => c.tempo) ?? false;
  const sameBudget = file.mana_per_second === req.mana_per_second && file.spirit === req.spirit;
  out.push({ id: "budget", status: weighted && sameBudget ? "ok" : "warn", values: { weighted: weighted ? 1 : 0, mana: file.mana_per_second ?? "?", spirit: file.spirit ?? "?" } });
  const n = matchingChains(doc, req).length;
  out.push({ id: "matches", status: n ? "ok" : "fail", values: { n } });
  return out;
}

export function gateOpen(checks: GateCheck[]): boolean {
  return checks.length > 1 && checks.every((c) => c.status !== "fail");
}

/** The command that produces a file for these requirements. */
export function exportCommand(file = "requirements.json"): string {
  return `python3 -m exilegen.builder.graphx --requirements ${file} --corpus-dir <corpus> --out graph.json`;
}
