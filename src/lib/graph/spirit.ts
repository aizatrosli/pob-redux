import type { ChainSpirit, GraphDoc, GraphRequirements, LevelConfig, SpiritGem, SpiritInfo } from "./types";

/** A Spirit limit and where it comes from. `level`, `quest` and `gear` are set when the source is "level". */
export interface SpiritBudget {
  budget: number;
  /** The level a suggested gem must be usable by. Null when the limit is fixed or taken at the final level. */
  cap: number | null;
  source: "fixed" | "level";
  level?: number;
  quest?: number;
  gear?: number;
}

/** The fitted set for one chain. `approx` marks a set fitted here and not by the export. */
export interface SpiritFit {
  budget: number;
  own: number;
  free: number;
  picks: SpiritGem[];
  more: SpiritGem[];
  used: number;
  approx: boolean;
}

/** Spirit from campaign quests held at a character level. */
export function questSpirit(level: number, cfg: LevelConfig): number {
  return Math.max(0, ...Object.entries(cfg.quest_spirit).map(([at, n]) => (Number(at) <= level ? n : 0)));
}

/** The level the Spirit limit is taken at: 0 or nothing means the final level. */
export function spiritLevel(level: number | undefined, cfg: LevelConfig): number {
  const l = Math.round(Number(level) || 0);
  return l > 0 ? Math.min(l, cfg.max_level) : cfg.max_level;
}

/** The request's Spirit limit: the fixed number, or quest Spirit at the level plus Spirit from equipment. */
export function spiritBudget(req: Pick<GraphRequirements, "spirit" | "spirit_source" | "spirit_level" | "spirit_gear">, cfg: LevelConfig): SpiritBudget {
  if (req.spirit_source !== "level") return { budget: Math.max(0, Number(req.spirit) || 0), cap: null, source: "fixed" };
  const level = spiritLevel(req.spirit_level, cfg);
  const quest = questSpirit(level, cfg);
  const gear = Math.max(0, Number(req.spirit_gear) || 0);
  return { budget: quest + gear, cap: level < cfg.max_level ? level : null, source: "level", level, quest, gear };
}

export function spiritInfo(doc: GraphDoc | null): SpiritInfo | null {
  const s = doc?.mechanics?.info?.spirit;
  return s && typeof s === "object" ? (s as SpiritInfo) : null;
}

/** The limit the file was exported with. A file from before Spirit gems only has the fixed number. */
export function fileSpiritBudget(doc: GraphDoc, cfg: LevelConfig): SpiritBudget {
  const info = spiritInfo(doc);
  if (!info) return spiritBudget({ ...doc.requirements, spirit_source: "fixed" }, cfg);
  if (info.source !== "level") return { budget: info.budget, cap: null, source: "fixed" };
  const level = spiritLevel(info.level, cfg);
  return { budget: info.budget, cap: level < cfg.max_level ? level : null, source: "level", level, quest: info.quest, gear: info.gear };
}

/** Whether two limits give the same set: the same amount and the same level cap on gems. */
export function sameSpiritBudget(a: SpiritBudget, b: SpiritBudget): boolean {
  return a.budget === b.budget && a.cap === b.cap;
}

/**
 * The Spirit gems of a chain under a limit. With the file's own limit this is the file's set.
 * Otherwise the set is fitted again from the file's candidates: best score first, each one taken
 * when it is usable by the level and still fits the Spirit left after the chain's own reservation.
 * exilegen may pack the set better, so a set fitted here is marked `approx`.
 */
export function fitSpirit(sp: ChainSpirit, want: SpiritBudget, file: SpiritBudget): SpiritFit {
  if (sp.budget === want.budget && want.cap === file.cap) return { ...sp, approx: false };
  const level = want.cap;
  const seen = new Set<string>();
  const all = [...sp.picks, ...sp.more]
    .filter((g) => !seen.has(g.name) && !!seen.add(g.name))
    .map((g, i) => ({ g, i }))
    .sort((a, b) => b.g.score - a.g.score || a.i - b.i)
    .map(({ g }) => g);
  const free = want.budget - sp.own;
  let left = free;
  const picks: SpiritGem[] = [];
  const more: SpiritGem[] = [];
  for (const { blocked: _, ...g } of all) {
    if (level != null && g.level != null && g.level > level) more.push({ ...g, blocked: "level" });
    else if (g.spirit <= left) {
      picks.push(g);
      left -= g.spirit;
    } else more.push({ ...g, blocked: "budget" });
  }
  return { budget: want.budget, own: sp.own, free, picks, more, used: free - left, approx: true };
}
