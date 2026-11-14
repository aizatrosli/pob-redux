/** The graph document exilegen writes (`python3 -m exilegen.builder.graphx`). Field names are exilegen's. */

export const GRAPH_FORMAT = "exilegen-graph";
export const GRAPH_VERSION = 1;

/** What to analyse. The same object drives exilegen's export and its builder. */
export interface GraphRequirements {
  constraints: string[];
  profiles: string[];
  budget_tier: "low" | "mid" | "high";
  cls: string | null;
  ascendancy: string | null;
  damage_types: string[];
  uniques: { name: string }[];
  forbid_uniques: string[];
  keystones: string[];
  chain_kinds: string[];
  min_novelty: number;
  top: number;
  /** Resource the build can spend each second, and Spirit it can reserve. Chains are weighted against both. */
  mana_per_second: number;
  spirit: number;
}

export type CheckStatus = "ok" | "warn" | "fail";

export interface GraphCheck {
  id: string;
  group: "data" | "requirements";
  status: CheckStatus;
  label: string;
  detail: string;
}

/** What one skill costs to use, how long a use takes and how often it can be used (gem level 20). */
export interface SkillTempo {
  use_time: number;
  cooldown: number;
  stored_uses: number;
  cost: Record<string, number>;
  cost_per_second: number;
  spirit: number;
  persistent: boolean;
  instant: boolean;
  triggered: boolean;
  /** Cost resources the budget does not price (Ward, Rage). */
  unpriced: string[];
}

/** The 0..1 weight on a chain's score, and the four factors it is the product of. */
export interface ChainTempo {
  tempo: number;
  factors: { resource: number; spirit: number; cooldown: number; time: number };
  main: SkillTempo;
  helpers: (SkillTempo & { id: string; name: string })[];
  spirit_total: number;
  cost_per_second: number;
}

export type NodeKind = "unique" | "keystone" | "asc" | "skill" | "util" | "token" | "M" | "A" | "S" | "U" | "K" | "AN" | "G";

export interface GraphNode {
  id: string;
  kind: NodeKind;
  name: string;
  ascendancy?: string;
  class?: string;
  slot?: string;
  group?: string;
  kit?: number;
  n?: number;
  tempo?: SkillTempo;
}

export interface GraphEdge {
  s: string;
  t: string;
  type: "produces" | "consumes" | "cooc";
  w: number;
  line?: string;
  n?: number;
}

export interface GraphChain {
  id: string;
  kind: string;
  path: string[];
  path_tokens: string[];
  tokens: string[];
  main_skill: string;
  main_name: string;
  uniques: string[];
  keystones: string[];
  asc_nodes: [string, string][];
  main_damage_types: string[];
  main_spell: boolean;
  main_minion: boolean;
  fit: number;
  novelty: number;
  /** Fit × novelty × tempo. */
  score: number;
  score_raw?: number;
  tempo?: ChainTempo;
  thesis: string;
  evidence: string[];
}

export interface GraphKit {
  id: number;
  label: string;
  size: number;
  builds: number;
  mean_resid: number | null;
  skills: { name: string; n: number }[];
  core: string[];
}

export interface GraphGap {
  skill: string;
  skill_builds: number;
  kind: "S" | "U";
  candidate: string;
  candidate_builds: number;
  score: number;
  driven_by: string[];
}

export interface GraphDoc {
  format: string;
  version: number;
  game: string;
  generated_at: string;
  requirements: GraphRequirements;
  preflight: GraphCheck[];
  passed: boolean;
  catalog?: { ascendancies: { name: string; class: string }[] };
  mechanics?: { nodes: GraphNode[]; edges: GraphEdge[]; chains: GraphChain[]; info: Record<string, unknown> };
  ladder?: { nodes: GraphNode[]; edges: GraphEdge[]; kits: GraphKit[]; gaps: GraphGap[]; info: Record<string, unknown> };
}

export const DAMAGE_TYPES = ["Physical", "Fire", "Cold", "Lightning", "Chaos"] as const;
export const CHAIN_KINDS = ["self_feed", "enabler", "graft", "bridge"] as const;
export const CONSTRAINT_CHOICES = ["one_button", "no_cast", "no_cast:strict", "no_minions"] as const;

export function defaultRequirements(): GraphRequirements {
  return {
    constraints: [],
    profiles: ["pinnacle_v1", "mapping_v1"],
    budget_tier: "mid",
    cls: null,
    ascendancy: null,
    damage_types: [],
    uniques: [],
    forbid_uniques: [],
    keystones: [],
    chain_kinds: [],
    min_novelty: 0,
    top: 150,
    mana_per_second: 250,
    spirit: 200,
  };
}
