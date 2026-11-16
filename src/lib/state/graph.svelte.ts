import { invoke } from "@tauri-apps/api/core";
import { CHAIN_KINDS, defaultRequirements, type ChainBuildResult, type ChainJob, type GraphDoc, type GraphRequirements } from "$lib/graph/types";

const REQ_KEY = "pob-redux:graph-requirements";
const DOC_KEY = "pob-redux:graph-doc";

/** The kinds that existed before "reaction". A saved filter with all four meant "every kind". */
const FIRST_KINDS = ["self_feed", "enabler", "graft", "bridge"];

/**
 * Saved chain kinds, read again against today's list: unknown kinds are dropped, and a filter that had every kind
 * of its day ticked becomes no filter, so a kind added later is not hidden by an old "all of them".
 */
export function savedKinds(saved: unknown): string[] {
  const known: readonly string[] = CHAIN_KINDS;
  const kinds = Array.isArray(saved) ? saved.filter((k): k is string => typeof k === "string" && known.includes(k)) : [];
  const allThen = FIRST_KINDS.every((k) => kinds.includes(k)) && kinds.length === FIRST_KINDS.length;
  return allThen || kinds.length === known.length ? [] : kinds;
}

/** The Graph view's file and the requirements in front of it. */
class GraphStore {
  doc = $state.raw<GraphDoc | null>(null);
  fileName = $state<string | null>(null);
  loadError = $state<string | null>(null);
  req = $state<GraphRequirements>(defaultRequirements());
  /** Chain builds asked for in this session, by chain id. They outlive the view, since one takes a minute or two. */
  jobs = $state<Record<string, ChainJob>>({});
  readonly building = $derived(Object.values(this.jobs).some((j) => j.status === "running"));

  constructor() {
    try {
      const saved = JSON.parse(localStorage.getItem(REQ_KEY) ?? "null");
      if (saved && typeof saved === "object") this.req = { ...defaultRequirements(), ...saved, chain_kinds: savedKinds(saved.chain_kinds) };
      const kept = JSON.parse(localStorage.getItem(DOC_KEY) ?? "null");
      if (kept?.doc) {
        this.doc = kept.doc;
        this.fileName = kept.name ?? null;
      }
    } catch {}
  }

  setReq(patch: Partial<GraphRequirements>) {
    this.req = { ...this.req, ...patch };
    try {
      localStorage.setItem(REQ_KEY, JSON.stringify(this.req));
    } catch {}
  }

  /** Drop every filter that hides chains (class, damage types, kinds, uniques ...). Ranking, Spirit, leveling and excluded supports stay. */
  showAll() {
    const d = defaultRequirements();
    this.setReq({
      constraints: d.constraints,
      cls: d.cls,
      ascendancy: d.ascendancy,
      damage_types: d.damage_types,
      uniques: d.uniques,
      forbid_uniques: d.forbid_uniques,
      keystones: d.keystones,
      chain_kinds: d.chain_kinds,
      min_novelty: d.min_novelty,
      online_by: d.online_by,
    });
  }

  /**
   * Build one chain with exilegen at a level (null: the end level) without the excluded supports. One at a time.
   * `spiritGems` are added to the build as named, with `spiritGear` Spirit assumed from equipment.
   */
  async buildChain(chain: string, level: number | null, excluded: string[], spiritGems: string[] = [], spiritGear = 0) {
    if (this.building) return;
    const job: ChainJob = { status: "running", level, excluded, spiritGems, started: Date.now() };
    this.jobs = { ...this.jobs, [chain]: job };
    let next: ChainJob;
    try {
      const result = await invoke<ChainBuildResult>("lab_chain_build", { chain, level, excludeSupports: excluded, spiritGems, spiritGear: Math.max(0, Math.round(spiritGear)) });
      next = { ...job, status: "done", result };
    } catch (e) {
      const err = e as { kind?: string; message?: string } | null;
      next = { ...job, status: "error", error: String(err?.message ?? e), setup: err?.kind === "setup" };
    }
    this.jobs = { ...this.jobs, [chain]: next };
  }

  load(text: string, name: string) {
    this.loadError = null;
    try {
      const doc = JSON.parse(text);
      if (!doc || typeof doc !== "object" || Array.isArray(doc)) throw new Error("not a JSON object");
      this.doc = doc as GraphDoc;
      this.fileName = name;
      // A kind filter saved before this file existed (e.g. without "reaction") can hide every chain in it:
      // when none of the file's chains is of a ticked kind, show all kinds rather than an empty graph.
      const kinds = new Set((this.doc.mechanics?.chains ?? []).map((c) => c.kind));
      if (this.req.chain_kinds.length && kinds.size && !this.req.chain_kinds.some((k) => kinds.has(k))) {
        this.setReq({ chain_kinds: [] });
      }
    } catch (e) {
      this.doc = null;
      this.fileName = null;
      this.loadError = String(e instanceof Error ? e.message : e);
      return;
    }
    try {
      localStorage.setItem(DOC_KEY, JSON.stringify({ name, doc: this.doc }));
    } catch {
      // too large to keep: the file is reopened by hand next time
      localStorage.removeItem(DOC_KEY);
    }
  }

  clear() {
    this.doc = null;
    this.fileName = null;
    this.loadError = null;
    localStorage.removeItem(DOC_KEY);
  }
}

export const graph = new GraphStore();
