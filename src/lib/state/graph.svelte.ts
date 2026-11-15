import { invoke } from "@tauri-apps/api/core";
import { defaultRequirements, type ChainBuildResult, type ChainJob, type GraphDoc, type GraphRequirements } from "$lib/graph/types";

const REQ_KEY = "pob-redux:graph-requirements";
const DOC_KEY = "pob-redux:graph-doc";

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
      if (saved && typeof saved === "object") this.req = { ...defaultRequirements(), ...saved };
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

  /** Build one chain with exilegen at a level (null: the end level) without the excluded supports. One at a time. */
  async buildChain(chain: string, level: number | null, excluded: string[]) {
    if (this.building) return;
    const job: ChainJob = { status: "running", level, excluded, started: Date.now() };
    this.jobs = { ...this.jobs, [chain]: job };
    let next: ChainJob;
    try {
      const result = await invoke<ChainBuildResult>("lab_chain_build", { chain, level, excludeSupports: excluded });
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
