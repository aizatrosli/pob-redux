import type { ChainLeveling, LevelConfig, LevelStep } from "./types";

/** Defaults for everything in a leveling path that is a choice or an estimate. Mirrors exilegen's `leveling.DEFAULT_CONFIG`. */
export function defaultLevelConfig(): LevelConfig {
  return {
    trial_levels: [22, 33, 60, 75],
    quest_passive_points: { "10": 2, "12": 4, "22": 6, "26": 8, "30": 10, "39": 12, "47": 14, "48": 16, "52": 18, "54": 20, "56": 22, "58": 24 },
    quest_spirit: { "11": 30, "33": 60, "55": 100 },
    boss_level: 75,
    early_level: 30,
    late_level: 90,
    late_factor: 0.4,
    boss_factor: 0.8,
    farm_floor: 0.5,
    max_level: 100,
  };
}

export function levelConfig(overrides: Partial<LevelConfig> | undefined): LevelConfig {
  return { ...defaultLevelConfig(), ...(overrides ?? {}) };
}

const table = (d: Record<string, number>): [number, number][] =>
  Object.entries(d)
    .map(([k, v]): [number, number] => [Number(k), v])
    .sort((a, b) => a[0] - b[0]);

function levelForPassives(points: number, cfg: LevelConfig): number {
  const quests = table(cfg.quest_passive_points);
  for (let lvl = 1; lvl <= cfg.max_level; lvl++) {
    const quest = Math.max(0, ...quests.filter(([at]) => at <= lvl).map(([, n]) => n));
    if (lvl - 1 + quest >= points) return lvl;
  }
  return cfg.max_level;
}

function levelForAscendancy(points: number, cfg: LevelConfig): number {
  const n = Math.max(Math.floor((points + 1) / 2), 1);
  return n <= cfg.trial_levels.length ? cfg.trial_levels[n - 1] : cfg.max_level;
}

export function levelFactor(online: number, bosses: number, unobtainable: boolean, cfg: LevelConfig): number {
  if (unobtainable) return 0;
  const width = cfg.late_level - cfg.early_level;
  const span = width > 0 ? Math.min(Math.max((online - cfg.early_level) / width, 0), 1) : online > cfg.early_level ? 1 : 0;
  return (1 - (1 - cfg.late_factor) * span) * Math.max(cfg.boss_factor ** bosses, cfg.farm_floor);
}

/** The same path under another config. Steps carry the raw facts, so no game data is needed. */
export function settle(path: ChainLeveling, cfg: LevelConfig): ChainLeveling {
  const spirit = table(cfg.quest_spirit);
  const steps = path.steps.map((s): LevelStep => {
    const points = s.points ?? null;
    switch (s.gate) {
      case "boss":
        return { ...s, level: Math.max(s.item_level ?? 1, cfg.boss_level) };
      case "passives":
        return points == null ? s : { ...s, level: levelForPassives(points, cfg) };
      case "ascendancy":
        return { ...s, level: levelForAscendancy(points ?? 2, cfg) };
      case "spirit":
      case "spirit_gear": {
        const hit = spirit.find(([, have]) => (points ?? 0) <= have);
        const last = spirit.length ? spirit[spirit.length - 1] : ([1, 0] as [number, number]);
        return { ...s, level: hit ? hit[0] : last[0], gate: hit ? "spirit" : "spirit_gear", quest: last[1] };
      }
      default:
        return s;
    }
  });
  steps.sort((a, b) => a.level - b.level || a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));
  const online = Math.max(1, ...steps.map((s) => s.level));
  const bosses = steps.filter((s) => s.gate === "boss");
  return {
    ...path,
    steps,
    start_level: steps.find((s) => s.main)?.level ?? 1,
    online_level: online,
    farm: [...new Set(bosses.map((s) => s.boss ?? ""))].sort(),
    factor: levelFactor(online, bosses.length, path.unobtainable, cfg),
  };
}

export function sameLevelConfig(a: Partial<LevelConfig> | undefined, b: Partial<LevelConfig> | undefined): boolean {
  return JSON.stringify(levelConfig(a)) === JSON.stringify(levelConfig(b));
}
