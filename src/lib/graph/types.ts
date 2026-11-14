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
  /** Rank chains by how early they work while leveling and what has to be farmed for them. */
  leveling: boolean;
  /** Hide chains that are not complete by this character level. 0 means no limit. */
  online_by: number;
  /** Overrides of the leveling defaults: trial levels, boss level, scoring thresholds, quest schedule. */
  leveling_config: Partial<LevelConfig>;
  /** "formula" ranks by the score; "measured" puts chains with the highest PoB-measured gain first; "fitted" ranks by the score fitted to PoB runs. */
  rank_by: "formula" | "measured" | "fitted";
}

/** The score fitted to PoB measurements (exilegen chainfit): what adding the chain is expected to do. */
export interface ChainPredicted {
  /** Chance the chain turns into a build with damage, 0 to 1. */
  works: number;
  /** Expected DPS multiple over the plain build when it does. */
  gain: number;
  /** works × gain. */
  score: number;
  /** The parts and traits that move the prediction most, as multiples. */
  why: { part: string; effect: number }[];
}

/** What PoB said when the chain was built for real (exilegen chainscore). */
export interface ChainMeasured {
  built: boolean;
  feasible: boolean;
  unverified: boolean;
  /** Honest DPS of the chain build, and of the same setup built without the chain. */
  dps: number;
  ehp?: number | null;
  base_dps?: number;
  /** Chain DPS over baseline DPS, with every conditional state off. */
  gain?: number;
  /** Median DPS of the plain builds on the same main skill, over typical_n builds. */
  typical_dps?: number;
  typical_n?: number;
  /** Chain DPS over the stronger of its own baseline and typical_dps. */
  gain_fair?: number;
  /** Fewer than 3 plain builds exist for the skill, so the comparison rests on little. */
  thin?: boolean;
  /** DPS with the chain's own states switched on over the same build with them off. */
  state_lift?: number;
  base_state_lift?: number;
  /** state_lift over the baseline's: above 1 means the chain's payoff part uses the state better. */
  payoff?: number;
  /** gain_fair × state_lift: the chain credited for the state it produces, assuming it is always up. */
  gain_credited?: number;
  error?: string;
}

export interface MeasuredSummary {
  scored: number;
  pool: number;
  built: number;
  feasible: number;
  compared: number;
  median_gain: number | null;
  better_by_10pct: number;
  worse_by_10pct: number;
}

export interface LevelConfig {
  /** Character level at which each of the four ascendancy trials is done. */
  trial_levels: number[];
  /** Character level → passive points from campaign quests held by then. */
  quest_passive_points: Record<string, number>;
  /** Character level → Spirit from campaign quests held by then. */
  quest_spirit: Record<string, number>;
  /** Level at which boss-only uniques can be farmed. */
  boss_level: number;
  /** A chain complete by here keeps its full score. */
  early_level: number;
  /** A chain complete only here gets `late_factor`. */
  late_level: number;
  late_factor: number;
  /** Score multiplier per boss-only unique, never below `farm_floor`. */
  boss_factor: number;
  farm_floor: number;
  max_level: number;
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
/** One part of a chain and the level at which it becomes available. */
export interface LevelStep {
  kind: "skill" | "unique" | "keystone" | "asc" | "spirit";
  name: string;
  level: number;
  /** The item's own level requirement, before the boss level is applied. */
  item_level?: number;
  gate: "gem" | "base" | "item" | "drop" | "boss" | "unobtainable" | "passives" | "ascendancy" | "spirit" | "spirit_gear";
  known: boolean;
  main?: boolean;
  boss?: string;
  area?: string;
  /** Weapon base or unique that grants the skill. */
  base?: string;
  item?: string;
  league?: string;
  points?: number | null;
  cls?: string | null;
  ascendancy?: string;
  /** Spirit that campaign quests give in total (for the Spirit step). */
  quest?: number;
}

export interface ChainLeveling {
  steps: LevelStep[];
  /** Level at which the main skill can be used. */
  start_level: number;
  /** Level at which every part of the chain is available. */
  online_level: number;
  /** Bosses that have to be farmed for boss-only uniques. */
  farm: string[];
  drops: string[];
  unobtainable: boolean;
  unknown: string[];
  /** Score multiplier when the leveling option is on. */
  factor: number;
}

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
  leveling?: ChainLeveling;
  measured?: ChainMeasured;
  predicted?: ChainPredicted;
  formula_rank?: number;
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
    leveling: false,
    online_by: 0,
    leveling_config: {},
    rank_by: "formula",
  };
}
