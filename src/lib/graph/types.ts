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
  /** Where the Spirit limit comes from: the fixed `spirit`, or quest Spirit at `spirit_level` plus `spirit_gear`. */
  spirit_source: "fixed" | "level";
  /** Character level for the Spirit limit when the source is "level". 0 means the final level. */
  spirit_level: number;
  /** Spirit from equipment, added on top of quest Spirit when the source is "level". */
  spirit_gear: number;
  /** Attach Spirit gem suggestions to chains. */
  spirit_gems: boolean;
  /** Rank chains by how early they work while leveling and what has to be farmed for them. */
  leveling: boolean;
  /** Hide chains that are not complete by this character level. 0 means no limit. */
  online_by: number;
  /** Overrides of the leveling defaults: trial levels, boss level, scoring thresholds, quest schedule. */
  leveling_config: Partial<LevelConfig>;
  /** "formula" ranks by the score; "measured" puts chains with the highest PoB-measured gain first; "fitted" ranks by the score fitted to PoB runs. */
  rank_by: "formula" | "measured" | "fitted";
  /** Support gems (display names) that a build must not socket. Chains that used one are marked, not hidden. */
  exclude_supports: string[];
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

/** How the chain sits against the poe.ninja ladder sample (strong builds from the top of the experience ladder). */
export interface ChainLadder {
  cls: "meta" | "known skill, new pairing" | "off-meta skill";
  known: boolean;
  /** Share of ladder characters using the main skill (0 to 1), its rank by share, and sampled builds. */
  share?: number;
  rank?: number;
  n?: number;
  /** What ladder builds of this skill socket and wear most. */
  supports?: string[];
  uniques?: string[];
  dps_median?: number;
  dps_p90?: number;
  /** Chain build DPS over the ladder median for the skill, same damage measure. */
  dps_vs_median?: number;
  thin?: boolean;
  /** Chain parts that ladder builds of this skill commonly hold. */
  paired?: string[];
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
  /** Support gems the search socketed on the main skill in the chain build, and in the plain build. */
  supports?: string[];
  base_supports?: string[];
  /** The main skill comes from an item, which the search does not give support gems. */
  main_granted?: boolean;
  /** Fewer than 3 plain builds exist for the skill, so the comparison rests on little. */
  thin?: boolean;
  /** DPS with the chain's own states switched on over the same build with them off. */
  state_lift?: number;
  base_state_lift?: number;
  /** state_lift over the baseline's: above 1 means the chain's payoff part uses the state better. */
  payoff?: number;
  /** gain_fair × state_lift: the chain credited for the state it produces, assuming it is always up. */
  gain_credited?: number;
  /** PoB share code of the measured chain build (exilegen graphx `--codes`). */
  code?: string;
  error?: string;
}

/** A persistent skill that reserves Spirit, suggested for a chain. */
export interface SpiritGem {
  name: string;
  skill: string;
  spirit: number;
  /** feeds: makes a state the chain's payoff uses. pays_off: uses a state the main skill makes. scales: adds damage. */
  role: "feeds" | "pays_off" | "scales" | "utility";
  /** The state that links it to the chain, for feeds and pays_off. */
  token?: string | null;
  why: string;
  /** Character level at which the gem can be used. */
  level: number | null;
  score: number;
  /** Why a candidate is not in the set. Only on `more`. */
  blocked?: "budget" | "weaker" | "level";
}

/** The Spirit gems suggested for one chain, for the budget the file was exported with. */
export interface ChainSpirit {
  budget: number;
  /** Spirit the chain's own skills reserve. */
  own: number;
  /** budget - own. Negative when the chain itself does not fit. */
  free: number;
  picks: SpiritGem[];
  more: SpiritGem[];
  used: number;
}

export interface SpiritGearPreset {
  id: string;
  label: string;
  spirit: number;
  note?: string;
}

/** `mechanics.info.spirit`: the Spirit budget of the export and the equipment that gives Spirit. */
export interface SpiritInfo {
  budget: number;
  source: "fixed" | "level";
  level?: number;
  quest?: number;
  gear?: number;
  gear_presets?: SpiritGearPreset[];
  gems?: number;
}

/** The Spirit side of a finished chain build, as PoB reports it. */
export interface ChainBuildSpirit {
  budget: number;
  reserved: number;
  unreserved: number;
  /** A gem is not enabled when it had to be switched off for lack of Spirit. */
  gems: { name: string; spirit: number; enabled: boolean }[];
  /** DPS of the same chain build without the Spirit gems. */
  dps_without?: number | null;
}

/** One excluded support in a chain build, and what took its place. Null means the plain build was not made. */
export interface ChainBuildAlternative {
  excluded: string;
  was_used: boolean | null;
  replaced_by: string | null;
  dps_without_exclusion?: number | null;
}

/** What `exilegen.lab chain` returns: one chain built at a level without the excluded supports. */
export interface ChainBuildResult {
  lab: number;
  chain: string;
  main_name: string;
  level: number;
  ok: boolean;
  feasible: boolean;
  violations: string;
  code?: string | null;
  dps: number;
  ehp?: number | null;
  supports: string[];
  excluded: string[];
  alternatives?: ChainBuildAlternative[];
  /** The chain's parts; one whose level is above the build's is left out. */
  parts: (Pick<LevelStep, "kind" | "name" | "level" | "gate"> & { included: boolean })[];
  /** Items the build still holds although they need a higher level, because the main skill cannot exist without them. */
  over_level?: { kind: string; name: string; level: number }[];
  /** Present when the build was asked for with Spirit gems. */
  spirit?: ChainBuildSpirit | null;
  seconds: number;
}

/** A chain build asked for from the Graph view. `setup` marks an error about Python, exilegen or pobctl not being found. */
export interface ChainJob {
  status: "running" | "done" | "error";
  /** Null builds at the generator's end level. */
  level: number | null;
  excluded: string[];
  /** Spirit gems the build was asked to include. */
  spiritGems?: string[];
  started: number;
  result?: ChainBuildResult;
  error?: string;
  setup?: boolean;
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
  /** Typed reading of the link against game stat ids and PoB tags (exilegen edges.py). */
  relation?: "applies" | "generates" | "spends" | "while" | "scales" | "raises_cap" | "amplifies" | "forbids" | "converts";
  /** Who holds the state the link is about. */
  owner?: "enemy" | "self";
  /** confirmed by structured data, contradicted by it (see `why`), or read from text alone. */
  verdict?: "confirmed" | "contradicted" | "text";
  why?: string;
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
  ladder?: ChainLadder;
  /** Reasons to doubt the chain itself (a link its own text does not bear out). */
  doubts?: string[];
  spirit?: ChainSpirit;
  /** Kind "reaction": the meta gem that casts a second spell from an event of the main skill. */
  reaction?: ChainReaction;
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

/** A meta gem ("Cast on ...") fed by the main skill. The numbers are estimates: PoB calculates no Energy. */
export interface ChainReaction {
  meta: string;
  meta_name: string;
  spells: string[];
  spell_names: string[];
  event: string;
  event_label: string;
  /** Energy against a boss from the gem's own stats: per event, per cast of the spell, events for one cast. */
  energy: { per_event: number; per_cast: number; events_per_cast: number | null; assumes: string; bank?: number; scaled?: string };
  /** exilegen's scoring has an Energy model for this meta gem (Cast on Critical, Cast on Elemental Ailment). */
  modelled: boolean;
  /** False when the meta gem comes from an item or a notable: shown, not built yet. */
  buildable: boolean;
  /** Judged reliability of the event, 0..1. */
  boss: number;
  clearing: number;
  spirit: number;
  source: { kind: "gem" | "unique" | "asc" | "base"; name?: string; ascendancy?: string };
  ladder_builds: number;
  payoff: "state" | "loop" | "damage";
  state?: string;
}

export const DAMAGE_TYPES = ["Physical", "Fire", "Cold", "Lightning", "Chaos"] as const;
export const CHAIN_KINDS = ["self_feed", "enabler", "graft", "bridge", "reaction"] as const;
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
    spirit_source: "fixed",
    spirit_level: 0,
    spirit_gear: 0,
    spirit_gems: true,
    leveling: false,
    online_by: 0,
    leveling_config: {},
    rank_by: "formula",
    exclude_supports: [],
  };
}
