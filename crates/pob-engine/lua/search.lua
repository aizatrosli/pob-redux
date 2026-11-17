-- Build-search methods (search_*), for programs that score many small changes to one build:
-- what-ifs on nodes, gems and attribute choices, cheap snapshot/restore, and evaluation profiles.
--
-- Loaded after bridge.lua with (methods, bridge helpers, profiles JSON); it adds methods to the
-- bridge table. Ported from exilegen's exg.lua; SEARCH_API.md maps its ops to these names.
--
-- Every stat comes from one calculation path: PoB's calcs.initEnv(build, "CALCULATOR", override)
-- then calcs.perform, the pipeline behind PoB's calcFunc and node power, with the override's
-- mainSocketGroup set to the selected main skill. So search_stats, search_whatif and search_apply
-- read the same numbers for the same state. PoB's main output (sidebar, get_stats) is only
-- refreshed by search_export or the bridge's `refresh`.

local M, U, profilesJson = ...
local native = __native
local array, null = native.array, native.null
local dkjson = require("dkjson")
local gemUtil = U.gemUtil
local IS_POE2 = U.IS_POE2

local function B() return launch.main.modes["BUILD"] end
local function isnull(v) return v == nil or v == null end
local function num(v) if type(v) == "number" and v == v and v ~= math.huge and v ~= -math.huge then return v end return nil end
local function n0(v) return num(v) or 0 end
local function opt(v) if v == nil then return null end return v end

local STN = {}
for k, v in pairs(SkillType) do STN[v] = k end
local function typeNames(t)
	local r = array({})
	if t then for k, v in pairs(t) do if v and STN[k] then r[#r + 1] = STN[k] end end end
	table.sort(r)
	return r
end
local function has(t, name) return t and SkillType[name] and t[SkillType[name]] and true or false end

local function copyArr(t)
	local r = array({})
	for i, v in ipairs(t or {}) do r[i] = v end
	return r
end

local function keysSorted(t)
	local r = array({})
	for k, v in pairs(t or {}) do if v then r[#r + 1] = k end end
	table.sort(r)
	return r
end

local CORE_FIELDS = { "mainSkill", "stats", "minionStats", "main", "dps", "offence", "sustain", "movementSpeed",
	"ailmentThreshold", "spirit", "maxHit", "requirements", "keystones", "uniques" }
-- marginals is never implied by "all": it costs one calculation per support of the scored groups
local ALL_FIELDS = { "mainSkill", "stats", "minionStats", "main", "pobMain", "dps", "offence", "sustain", "movementSpeed",
	"ailmentThreshold", "spirit", "maxHit", "requirements", "keystones", "uniques", "unsupported", "skills", "energy" }

local function wantSet(fields, default)
	if isnull(fields) then fields = default end
	if type(fields) == "string" then fields = { fields } end
	local s = {}
	for _, f in ipairs(fields or {}) do
		if f == "core" then for _, g in ipairs(CORE_FIELDS) do s[g] = true end
		elseif f == "all" then for _, g in ipairs(ALL_FIELDS) do s[g] = true end
		else s[f] = true end
	end
	return s
end

-- ---------------------------------------------------------------------------
-- Session state, tied to the open build
-- ---------------------------------------------------------------------------

local shipped = dkjson.decode(profilesJson or "{}", 1, nil)
local PROFILES = shipped and shipped.profiles or {}
local S = {}

local function resetState()
	for k in pairs(S) do S[k] = nil end
	S.generation = launch.main.__reduxBuildGeneration
	S.snapshots, S.nextToken = {}, 1
	S.profiles, S.profileSets = {}, {}
	S.mainSel, S.dirty, S.variants = "pob", false, {}
	local b = B()
	S.playerConfigSetId = b.configTab and b.configTab.activeConfigSetId
	S.pobMain = b.mainSocketGroup
	S.profile = "as_exported"
	local list = b.skillsTab and b.skillsTab.socketGroupList or {}
	local gi = b.mainSocketGroup or 1
	S.main = { group = gi, skill = list[gi] and list[gi].mainActiveSkill or 1 }
end

-- A build opened by any other method starts a fresh session with the exported main skill.
local function session()
	U.ensureBuild()
	if S.generation ~= launch.main.__reduxBuildGeneration then resetState() end
	return S
end

local function profileDef(name)
	return S.profiles and S.profiles[name] or PROFILES[name]
end

-- ---------------------------------------------------------------------------
-- Calculation
-- ---------------------------------------------------------------------------

-- One calculation of the current state plus override. Returns env, output.
local function calcWith(override, skipEHP)
	local b = B()
	local calcs = b.calcsTab.calcs
	-- trigger rates are cached by skill identity, not state: never reuse them across variants
	if GlobalCache and GlobalCache.cachedData and GlobalCache.cachedData.CALCULATOR then wipeTable(GlobalCache.cachedData.CALCULATOR) end
	local env = calcs.initEnv(b, "CALCULATOR", override or {})
	env.override = override
	calcs.perform(env, skipEHP)
	return env, env.player.output
end

-- ---------------------------------------------------------------------------
-- Gems
-- ---------------------------------------------------------------------------

local gemIndex
local function findGem(key)
	if type(key) ~= "string" then return nil end
	if data.gems[key] then return data.gems[key] end
	if not gemIndex then
		gemIndex = { skill = {}, name = {} }
		for _, gd in pairs(data.gems) do
			if gd.grantedEffectId then gemIndex.skill[gd.grantedEffectId] = gemIndex.skill[gd.grantedEffectId] or gd end
			if gd.name then
				local prev = gemIndex.name[gd.name:lower()]
				if not prev or (prev.grantedEffect and prev.grantedEffect.hidden) then gemIndex.name[gd.name:lower()] = gd end
			end
		end
	end
	local lk = key:lower()
	return gemIndex.skill[key] or gemIndex.name[lk] or gemIndex.name[(lk:gsub(" support$", ""))] or gemIndex.name[lk .. " support"]
end

local function gemTier(gd) return gd and gd.Tier and gd.Tier > 0 and gd.Tier or nil end

-- PoE2 meta gems (Cast on X): the energy facts below are about these. PoE1 has none.
local function isMetaGemEffect(ge) return ge and not ge.support and has(ge.skillTypes, "Meta") and has(ge.skillTypes, "Triggers") end

local function groupMeta(g)
	for i, gem in ipairs(g.gemList or {}) do
		local ge = gem.gemData and gem.gemData.grantedEffect
		if isMetaGemEffect(ge) and gem.enabled ~= false then return gem, ge, i end
	end
end

local function newGemInstance(gd, level, quality, enabled)
	return {
		nameSpec = gd.name, gemId = gd.id, skillId = gd.grantedEffectId,
		level = level, quality = quality or 0, enabled = enabled ~= false, count = 1,
		enableGlobal1 = true, enableGlobal2 = true, corrupted = false, corruptLevel = 0,
	}
end

local function copyGem(gem)
	local c = {}
	for k, v in pairs(gem) do c[k] = v end
	return c
end

-- Applies gem_changes / group_enabled to the live skills tab. Gems are never mutated in place
-- (a changed gem is a new table), so the undo record restores the saved arrays.
local function applyGemChanges(changes, groupEnabled)
	local st = B().skillsTab
	local list = st.socketGroupList
	local undo = { groupList = {}, groups = {}, touched = {} }
	for i, g in ipairs(list) do undo.groupList[i] = g end
	local function touch(g)
		if not undo.touched[g] then
			undo.touched[g] = true
			local saved = {}
			for i, gem in ipairs(g.gemList) do saved[i] = gem end
			undo.groups[#undo.groups + 1] = { g = g, gems = saved, enabled = g.enabled, mas = g.mainActiveSkill }
		end
	end
	local removals = {}
	local ok, err = pcall(function()
		for _, c in ipairs(changes or {}) do
			local gi = tonumber(c.group)
			if not gi then error("gem change needs group", 0) end
			local g = list[gi]
			if not g then
				if gi == #list + 1 then
					g = { label = c.label or "search", enabled = true, includeInFullDPS = false, gemList = {}, mainActiveSkill = 1 }
					list[gi] = g
				else
					error("unknown group " .. tostring(c.group), 0)
				end
			end
			local slot = tonumber(c.slot) or (#g.gemList + 1)
			-- An item- or node-granted group keeps its granted skill in slot 1, but takes supports in the
			-- other slots as in PoB's skills tab, unless the grant says noSupports.
			if g.source and (g.noSupports or slot <= 1) then
				error("group " .. gi .. " is granted by an item or node: " ..
					(g.noSupports and "its skill cannot be supported" or "slot 1 (the granted skill) cannot be changed"), 0)
			end
			touch(g)
			local old = g.gemList[slot]
			if c.gem == false or c.remove == true then
				if old then removals[#removals + 1] = { g = g, slot = slot } end
			else
				local gem
				if c.gem ~= nil then
					local gd = findGem(c.gem)
					if not gd then error("unknown gem " .. tostring(c.gem), 0) end
					local level = tonumber(c.level)
					if not level then
						local og = old and old.gemData and old.gemData.grantedEffect
						if og and not og.support and not gd.grantedEffect.support then level = old.level
						else level = st:ProcessGemLevel(gd) end
					end
					gem = newGemInstance(gd, level, tonumber(c.quality) or (old and not gd.grantedEffect.support and old.quality) or 0, c.enabled)
				elseif old then
					gem = copyGem(old)
					if c.level ~= nil then gem.level = tonumber(c.level) end
					if c.quality ~= nil then gem.quality = tonumber(c.quality) end
					if c.enabled ~= nil then gem.enabled = c.enabled == true end
				else
					error("gem change on empty slot " .. slot .. " of group " .. gi .. " needs gem", 0)
				end
				if slot > #g.gemList then g.gemList[#g.gemList + 1] = gem else g.gemList[slot] = gem end
			end
		end
		table.sort(removals, function(a, b) return a.slot > b.slot end)
		for _, r in ipairs(removals) do table.remove(r.g.gemList, r.slot) end
		for k, v in pairs(groupEnabled or {}) do
			local g = list[tonumber(k)]
			if not g then error("unknown group " .. tostring(k), 0) end
			touch(g)
			g.enabled = v == true
		end
		for g in pairs(undo.touched) do st:ProcessSocketGroup(g) end
	end)
	if not ok then
		undo.failed = true
		return undo, err
	end
	return undo
end

local function undoGemChanges(undo)
	if not undo then return end
	local st = B().skillsTab
	local list = st.socketGroupList
	for i = #list, 1, -1 do list[i] = nil end
	for i, g in ipairs(undo.groupList) do list[i] = g end
	for _, r in ipairs(undo.groups) do
		for i = #r.g.gemList, 1, -1 do r.g.gemList[i] = nil end
		for i, gem in ipairs(r.gems) do r.g.gemList[i] = gem end
		r.g.enabled = r.enabled
		r.g.mainActiveSkill = r.mas
		if isValueInArray(list, r.g) then st:ProcessSocketGroup(r.g) end
	end
end

local function gemChangesOf(p)
	local gc = not isnull(p.gem_changes) and p.gem_changes or nil
	local ge = not isnull(p.group_enabled) and p.group_enabled or nil
	return gc, ge
end

-- ---------------------------------------------------------------------------
-- Skill classification
-- ---------------------------------------------------------------------------

local function damagingTypes(t)
	return has(t, "Damage") or has(t, "Attack") or has(t, "DamageOverTime") or has(t, "DegenOnlySpellDamage")
end

-- activation class as in corpus-eval's extract.lua
local function classify(as, inMeta)
	local t = as.skillTypes or {}
	local ge = as.activeEffect.grantedEffect
	local gt = ge.skillTypes or {}
	local f = as.skillFlags or {}
	local function h(n) return has(t, n) or has(gt, n) end
	if h("Triggered") or h("InbuiltTrigger") or (inMeta and not isMetaGemEffect(ge)) then return "triggered" end
	-- Invocations store energy until you press them (wiki Invocation), so they are a button
	if isMetaGemEffect(ge) and h("Invocation") then return "manual" end
	if h("Persistent") or h("HasReservation") then return "persistent" end
	if h("Minion") or h("CreatesMinion") or h("SummonsTotem") or h("Trapped") or h("RemoteMined") or f.totem or f.trap or f.mine or f.minion then return "proxy" end
	return "manual"
end

local function proxyKinds(as)
	local t, gt, f = as.skillTypes or {}, as.activeEffect.grantedEffect.skillTypes or {}, as.skillFlags or {}
	local function h(n) return has(t, n) or has(gt, n) end
	local pk = array({})
	if h("Minion") or h("CreatesMinion") or f.minion then pk[#pk + 1] = "minion" end
	if h("SummonsTotem") or f.totem then pk[#pk + 1] = "totem" end
	if h("Trapped") or f.trap then pk[#pk + 1] = "trap" end
	if h("RemoteMined") or f.mine then pk[#pk + 1] = "mine" end
	return pk
end

-- { [groupIndex] = { activeSkill, ... } } in PoB's mainActiveSkill order
local function skillsByGroup(env)
	local list = B().skillsTab.socketGroupList
	local gidx = {}
	for i, g in ipairs(list) do gidx[g] = i end
	local by = {}
	for _, as in ipairs(env.player.activeSkillList or {}) do
		local gi = as.socketGroup and gidx[as.socketGroup]
		if gi then
			by[gi] = by[gi] or {}
			by[gi][#by[gi] + 1] = as
		end
	end
	return by
end

local function skillName(as)
	local f = B().calcsTab.calcs.getActiveSkillDisplayName
	local ok, nm = false, nil
	if f then ok, nm = pcall(f, as) end
	return (ok and nm) or as.activeEffect.grantedEffect.name
end

local function groupGrantKind(g)
	if not g.source then return nil end
	return gemUtil.grantedBy(g).kind
end

local function groupSupportCount(g)
	local n = 0
	for _, gem in ipairs(g.gemList or {}) do
		local ge = gem.gemData and gem.gemData.grantedEffect
		if ge and ge.support and gem.enabled ~= false then n = n + 1 end
	end
	return n
end

local function skillFacts(as, gi, si)
	local g = B().skillsTab.socketGroupList[gi]
	local ge = as.activeEffect.grantedEffect
	local gd = as.activeEffect.gemData
	local inMeta = groupMeta(g) ~= nil
	local t = as.skillTypes or {}
	local damaging = damagingTypes(t) or damagingTypes(ge.skillTypes) or false
	local movement = has(t, "Travel") or has(t, "Blink") or false
	local src = as.activeEffect.srcInstance
	local gemSlot
	for i, gem in ipairs(g.gemList) do if gem == src then gemSlot = i end end
	return {
		group = gi, index = si, name = skillName(as), id = ge.id, gemSlot = gemSlot,
		types = typeNames(t),
		activation = classify(as, inMeta), proxyKind = proxyKinds(as),
		damaging = damaging, spell = has(t, "Spell"), attack = has(t, "Attack"),
		movement = movement, utility = (not damaging) and not movement,
		meta = isMetaGemEffect(ge) or false,
		itemGranted = g.source ~= nil,
		grantKind = groupGrantKind(g),
		groupSupports = groupSupportCount(g),
		weaponDefault = (gd and gd.Tier == 0 and ((gd.variantId or ""):find("^PlayerDefault") ~= nil or gd.weaponRequirements ~= nil)) or false,
		enabled = g.enabled ~= false and (not src or src.enabled ~= false),
		triggeredBy = as.triggeredBy and as.triggeredBy.grantedEffect and as.triggeredBy.grantedEffect.name or nil,
	}
end

-- ---------------------------------------------------------------------------
-- Stats extraction
-- ---------------------------------------------------------------------------

-- corpus-eval's STAT_KEYS (extract.lua), same names, then search extensions.
local STAT_KEYS = {
	"CombinedDPS", "TotalDPS", "FullDPS", "TotalDot", "TotalDotDPS", "WithDotDPS", "WithBleedDPS", "WithIgniteDPS", "WithPoisonDPS",
	"BleedDPS", "IgniteDPS", "PoisonDPS", "TotalPoisonDPS", "DecayDPS", "ImpaleDPS", "CullingDPS", "ReservationDPS", "MirageDPS",
	"AverageHit", "AverageDamage", "AverageBurstDamage", "Speed", "HitSpeed", "HitChance", "CritChance", "PreEffectiveCritChance", "CritMultiplier",
	"Life", "LifeUnreserved", "EnergyShield", "Mana", "ManaUnreserved", "Spirit", "SpiritReserved", "SpiritUnreserved",
	"Armour", "Evasion", "Ward", "EnergyShieldRecharge", "LifeRegenRecovery", "ManaRegenRecovery",
	"BlockChance", "EffectiveBlockChance", "SpellBlockChance", "EffectiveSpellBlockChance", "SpellSuppressionChance",
	"MeleeEvadeChance", "ProjectileEvadeChance", "EvadeChance", "PhysicalDamageReduction", "DeflectChance", "EffectiveDeflectChance",
	"FireResist", "ColdResist", "LightningResist", "ChaosResist",
	"FireResistOverCap", "ColdResistOverCap", "LightningResistOverCap", "ChaosResistOverCap",
	"TotalEHP", "PhysicalMaximumHitTaken", "FireMaximumHitTaken", "ColdMaximumHitTaken", "LightningMaximumHitTaken", "ChaosMaximumHitTaken",
	"MovementSpeedMod", "Str", "Dex", "Int", "ManaCost", "ManaPerSecondCost", "CharmLimit",
	"PowerChargesMax", "FrenzyChargesMax", "EnduranceChargesMax",
	"FireResistMax", "ColdResistMax", "LightningResistMax", "ChaosResistMax",
	"ReqStr", "ReqDex", "ReqInt", "LifeCost", "LifePerSecondCost", "ESCost", "ESPerSecondCost",
	"LifeLeechRate", "ManaLeechRate", "LifeLeechGainRate", "ManaLeechGainRate", "NetLifeRegen", "NetManaRegen",
	"AilmentThreshold", "StunThreshold", "ShockChance", "IgniteChance", "FreezeChance", "ChillChance", "ElectrocuteChance",
	"BleedChance", "PoisonChance", "EnemyShockChance", "FreezeBuildup", "FreezeDurationMod",
	"SkillTriggerRate", "EffectiveSourceRate", "TriggerRateCap", "ManaRegen", "LifeRegen", "ExtraPoints",
}

local function pickStats(o)
	local s = {}
	for _, k in ipairs(STAT_KEYS) do
		local v = num(o[k])
		if v then s[k] = v end
	end
	local dt = {}
	for _, t in ipairs({ "Physical", "Lightning", "Cold", "Fire", "Chaos" }) do
		local v = o[t .. "HitAverage"]
		if type(v) ~= "number" then
			local mh, oh = o.MainHand and o.MainHand[t .. "HitAverage"], o.OffHand and o.OffHand[t .. "HitAverage"]
			if type(mh) == "number" and type(oh) == "number" then v = (mh + oh) / 2 else v = (type(mh) == "number" and mh) or (type(oh) == "number" and oh) or nil end
		end
		if type(v) == "number" and v > 0 then dt[t] = v end
	end
	s.HitDamageByType = dt
	if s.CombinedDPS and s.TotalDPS then s.NonHitDPS = math.max(0, s.CombinedDPS - s.TotalDPS) end
	return s
end

local function mainSkillInfo(ms)
	if not ms then return nil end
	local ge = ms.activeEffect and ms.activeEffect.grantedEffect
	local flags = array({})
	for k, v in pairs(ms.skillFlags or {}) do if v == true then flags[#flags + 1] = k end end
	table.sort(flags)
	return {
		name = skillName(ms), id = ge and ge.id, types = typeNames(ms.skillTypes), flags = flags,
		minion = ms.minion and ms.minion.minionData and ms.minion.minionData.name or nil,
	}
end

local function dpsOf(o)
	local p, m = n0(o.CombinedDPS), n0(o.Minion and o.Minion.CombinedDPS)
	return math.max(p, m), p, m
end

-- supports limited to one copy across all skills: lineage supports, and supports that reserve Spirit
local function oneCopyOnly(gd)
	local ge = gd and gd.grantedEffect
	if not (ge and ge.support) then return false end
	if gd.tags and gd.tags.lineage then return true end
	for _, lv in pairs(ge.levels or {}) do
		if type(lv) == "table" and (lv.spiritReservationFlat or 0) > 0 then return true end
	end
	return false
end

local HIT_TYPES = { "Physical", "Lightning", "Cold", "Fire", "Chaos" }
local function storedSum(o, key)
	local sum, any = 0, false
	for _, t in ipairs(HIT_TYPES) do
		local v = num(o[t .. key])
		if v then sum, any = sum + v, true end
	end
	return any and sum or nil
end
-- attacks: PoB keeps per-hit facts under MainHand / OffHand (main hand first)
local function hitSrc(o)
	if storedSum(o, "StoredCritAvg") == nil and type(o.MainHand) == "table" and storedSum(o.MainHand, "StoredCritAvg") then return o.MainHand end
	return o
end

local function offenceFacts(o)
	local avgHit = n0(o.AverageHit)
	if avgHit <= 0 and type(o.MainHand) == "table" then avgHit = n0(o.MainHand.AverageHit) end
	local hitChance = num(o.HitChance) or 100
	local hps
	if avgHit > 0 and n0(o.TotalDPS) > 0 then hps = n0(o.TotalDPS) / avgHit else hps = n0(o.Speed) * hitChance / 100 end
	local function pct(k) local v = num(o[k]); return v and math.min(v, 100) or 0 end
	return {
		speed = n0(o.Speed), hitSpeed = num(o.HitSpeed), cooldown = num(o.Cooldown), hitChance = hitChance, hitsPerSec = hps,
		averageHit = avgHit, critChance = pct("CritChance"), critMultiplier = num(o.CritMultiplier),
		shockChance = pct("ShockChance"), igniteChance = pct("IgniteChance"), freezeChance = pct("FreezeChance"),
		chillChance = pct("ChillChance"), electrocuteChance = pct("ElectrocuteChance"),
		bleedChance = pct("BleedChance"), poisonChance = pct("PoisonChance"),
		skillTriggerRate = num(o.SkillTriggerRate), effectiveSourceRate = num(o.EffectiveSourceRate), triggerRateCap = num(o.TriggerRateCap),
		-- energy scaling facts (wiki Cast on Critical): energy per crit = Power x raw crit hit (before mitigation)
		-- / enemy ailment threshold. PoB keeps pre-mitigation per-type averages as <Type>StoredCritAvg (crits) and
		-- <Type>StoredCombinedAvg; attacks keep them under MainHand/OffHand only.
		critHit = storedSum(hitSrc(o), "StoredCritAvg") or null,
		fireHit = num(hitSrc(o).FireStoredCombinedAvg) or null,
		enemyAilmentThreshold = num(o.EnemyAilmentThreshold) or num(hitSrc(o).EnemyAilmentThreshold) or null,
	}
end

local function sustainFacts(o)
	local manaPS, lifePS = n0(o.ManaPerSecondCost), n0(o.LifePerSecondCost)
	-- use per second when the skill is used continuously; PerSecondCost covers channel and upkeep skills
	local speed = n0(o.Speed)
	local manaUse = manaPS > 0 and manaPS or n0(o.ManaCost) * speed
	local lifeUse = lifePS > 0 and lifePS or n0(o.LifeCost) * speed
	local manaIn = n0(o.ManaRegenRecovery) + n0(o.ManaLeechGainRate)
	local lifeIn = n0(o.LifeRegenRecovery) + n0(o.LifeLeechGainRate)
	return {
		manaCost = n0(o.ManaCost), lifeCost = n0(o.LifeCost), esCost = n0(o.ESCost),
		manaPerSecondCost = manaPS, lifePerSecondCost = lifePS,
		manaUsePerSec = manaUse, lifeUsePerSec = lifeUse,
		manaRegen = n0(o.ManaRegenRecovery), lifeRegen = n0(o.LifeRegenRecovery),
		manaLeech = n0(o.ManaLeechGainRate), lifeLeech = n0(o.LifeLeechGainRate),
		manaNet = manaIn - manaUse, lifeNet = lifeIn - lifeUse,
		sustainable = (manaIn - manaUse) >= 0 and (lifeUse <= 0 or (lifeIn - lifeUse) >= 0),
	}
end

local function maxHitFacts(o)
	local r = {}
	local minV, minT
	for _, t in ipairs({ "Physical", "Fire", "Cold", "Lightning", "Chaos" }) do
		local v = num(o[t .. "MaximumHitTaken"])
		if v then
			r[t] = v
			if not minV or v < minV then minV, minT = v, t end
		end
	end
	r.min, r.minType = minV, minT
	return r
end

-- Mod lines PoB does not parse, as the bridge's _nc lists them, for this calculation's env.
local function unsupportedLines(env)
	local count, sample = 0, array({})
	local function add(lines)
		for _, l in ipairs(lines) do
			count = count + 1
			if #sample < 40 then sample[#sample + 1] = l end
		end
	end
	local slots = {}
	for slot in pairs(env.player.itemList or {}) do slots[#slots + 1] = slot end
	table.sort(slots)
	for _, slot in ipairs(slots) do
		local item = env.player.itemList[slot]
		if item then add(M._nc.itemLines(item)) end
	end
	for _, node in pairs(env.allocNodes or {}) do add(M._nc.nodeLines(node)) end
	return { count = count, sample = sample }
end

-- ---------------------------------------------------------------------------
-- Meta-gem energy facts (PoE2). Raw facts only; no correction is applied.
-- ---------------------------------------------------------------------------

local function constStat(ge, name)
	local ss = ge.statSets and ge.statSets[1]
	for _, cs in ipairs(ss and ss.constantStats or {}) do if cs[1] == name then return cs[2] end end
	return 0
end

local function levelStat(ge, name, level)
	local ss = ge.statSets and ge.statSets[1]
	if not ss then return 0 end
	for i, s in ipairs(ss.stats or {}) do
		if s == name then
			local lv = ss.levels and (ss.levels[math.min(level or 1, #ss.levels)] or ss.levels[#ss.levels])
			return lv and lv[i] or 0
		end
	end
	return constStat(ge, name)
end

local function qualStat(ge, name, q)
	for _, qs in ipairs(ge.qualityStats or {}) do if qs[1] == name then return qs[2] * (q or 0) end end
	return 0
end

local function energyStatList(ge, level)
	local out = array({})
	local ss = ge.statSets and ge.statSets[1]
	for _, cs in ipairs(ss and ss.constantStats or {}) do
		if tostring(cs[1]):find("energy") then out[#out + 1] = { stat = cs[1], value = cs[2] } end
	end
	for _, s in ipairs(ss and ss.stats or {}) do
		if tostring(s):find("energy") then out[#out + 1] = { stat = s, value = levelStat(ge, s, level), perLevel = true } end
	end
	return out
end

-- "Socketed Skills deal X% less Damage": the meta gem's support half carries trigger_meta_gem_damage_+%_final,
-- which PoB never maps (Cast on Critical -20). Returns the multiplier for the socketed spells.
local function metaSocketedDamageMore(mg)
	local gd = mg and mg.gemData
	local v = 0
	for _, k in ipairs({ "additionalGrantedEffectId1", "additionalGrantedEffectId2" }) do
		local ge = gd and gd[k] and data.skills[gd[k]]
		if ge then v = v + levelStat(ge, "trigger_meta_gem_damage_+%_final", mg.level) end
	end
	return 1 + v / 100
end

-- energy lines from allocated passives and equipped items, which PoB does not parse:
-- "Meta Skills gain X% increased/more Energy", "X% chance for Trigger skills to refund half of Energy Spent"
local function globalEnergyMods()
	local r = { inc = 0, more = 0, refund = 0, lines = array({}) }
	local function scan(line)
		if type(line) ~= "string" then return end
		local l = line:lower()
		local v = l:match("meta skills gain (%d+)%% increased energy")
		if v then r.inc = r.inc + tonumber(v); r.lines[#r.lines + 1] = line end
		v = l:match("meta skills gain (%d+)%% more energy")
		if v then r.more = r.more + tonumber(v); r.lines[#r.lines + 1] = line end
		v = l:match("(%d+)%% chance for trigger skills to refund half of energy spent")
		if v then r.refund = r.refund + tonumber(v); r.lines[#r.lines + 1] = line end
	end
	local b = B()
	pcall(function()
		for _, node in pairs(b.spec.allocNodes or {}) do for _, line in ipairs(node.sd or {}) do scan(line) end end
	end)
	pcall(function()
		for _, slot in pairs(b.itemsTab.slots or {}) do
			local item = slot.selItemId and b.itemsTab.items[slot.selItemId]
			if item and not slot.inactive then
				for _, key in ipairs({ "implicitModLines", "explicitModLines", "runeModLines", "enchantModLines" }) do
					for _, ml in ipairs(item[key] or {}) do scan(ml.line) end
				end
			end
		end
	end)
	return r
end

local function metaGroupFacts(gi)
	local g = B().skillsTab.socketGroupList[gi]
	local mg, mge = groupMeta(g)
	if not mg then return nil end
	local gen = levelStat(mge, "energy_generated_+%", mg.level) + qualStat(mge, "energy_generated_+%", mg.quality)
	local refund = 0
	local spells, supports = array({}), array({})
	local maxE = 0
	for slot, gem in ipairs(g.gemList) do
		local ge = gem.gemData and gem.gemData.grantedEffect
		if ge and gem.enabled ~= false then
			if ge.support then
				local sg = constStat(ge, "energy_generated_+%") + levelStat(ge, "energy_generated_+%", gem.level)
				local sr = constStat(ge, "trigger_skills_refund_half_energy_spent_chance_%")
				gen, refund = gen + sg, refund + sr
				if sg ~= 0 or sr ~= 0 then supports[#supports + 1] = { slot = slot, id = ge.id, name = gem.gemData.name, gen = sg, refund = sr } end
			elseif not isMetaGemEffect(ge) then
				local isSpell = has(ge.skillTypes, "Spell")
				local ct = ge.castTime or 0
				-- wiki Cast on Critical: 100 x base cast time + 200 x total cast time modifiers (Comet: 100 + 200 = 300)
				local addCt = (levelStat(ge, "total_cast_time_+_ms", gem.level) or 0) / 1000
				if isSpell then maxE = maxE + ct * 100 + addCt * 200 end
				spells[#spells + 1] = { slot = slot, gemId = gem.gemData.id, id = ge.id, name = gem.gemData.name, castTime = ct, addedCastTime = addCt, spell = isSpell, level = gem.level }
			end
		end
	end
	local perEvent = {}
	local ss = mge.statSets and mge.statSets[1]
	for _, cs in ipairs(ss and ss.constantStats or {}) do
		local ev = tostring(cs[1]):match("centienergy_per_monster_power_on_([%a_]+)$")
		if ev then perEvent[ev] = cs[2] / 100 end
	end
	return {
		group = gi, metaGemId = mg.gemData.id, metaSkillId = mge.id, metaName = mg.gemData.name, metaLevel = mg.level, metaQuality = mg.quality,
		spells = spells, energySupports = supports,
		maxEnergy = maxE, genBonus = gen, refund = refund,
		socketedDamageMore = metaSocketedDamageMore(mg), globalEnergy = globalEnergyMods(),
		perEvent = perEvent, perEventUnit = "energy per monster power per event",
		energyStats = energyStatList(mge, mg.level),
	}
end

-- ---------------------------------------------------------------------------
-- Evaluation
-- ---------------------------------------------------------------------------

local function withOverride(base, gi)
	local ov = {}
	for k, v in pairs(base or {}) do ov[k] = v end
	ov.mainSocketGroup = gi
	return ov
end

-- The group's mainActiveSkill set to `si` for one calculation.
local function calcSkill(base, gi, si, skipEHP)
	local g = B().skillsTab.socketGroupList[gi]
	local saved = g and g.mainActiveSkill
	if g and si then g.mainActiveSkill = si end
	local ok, env, o = pcall(calcWith, withOverride(base, gi), skipEHP)
	if g then g.mainActiveSkill = saved end
	return ok, env, o
end

-- One calculation per enabled damaging or proxy skill, each as if it were the main skill.
local function perSkill(baseOverride, env0)
	local by = skillsByGroup(env0)
	local out = array({})
	for gi, skills in pairs(by) do
		for si, as in ipairs(skills) do
			local f = skillFacts(as, gi, si)
			if (f.damaging or #f.proxyKind > 0) and f.enabled and not f.meta then
				local ok, env, o = calcSkill(baseOverride, gi, si, true)
				if ok then
					local d, p, m = dpsOf(o)
					f.dps, f.playerDps, f.minionDps = d, p, m
					f.offence = offenceFacts(o)
					if o.Minion then f.minionOffence = offenceFacts(o.Minion) end
					local s = sustainFacts(o)
					f.manaUsePerSec, f.lifeUsePerSec = s.manaUsePerSec, s.lifeUsePerSec
					if o.Minion then f.minionLife = num(o.Minion.Life) end
					-- corpse explosions: the corpse PoB assumes (config, else monster life at the enemy level)
					local msk = env.player.mainSkill
					if msk and msk.skillData and msk.skillData.explodeCorpse then
						f.corpseLife = num(msk.skillData.corpseLife) or (env.enemyLevel and data.monsterLifeTable and num(data.monsterLifeTable[env.enemyLevel])) or nil
					end
					f.types = typeNames(env.player.mainSkill and env.player.mainSkill.skillTypes)
				else
					f.err = tostring(env)
				end
				out[#out + 1] = f
			end
		end
	end
	table.sort(out, function(a, b) if a.group ~= b.group then return a.group < b.group end return a.index < b.index end)
	return out
end

local function energyFacts(skills, mainSel)
	local list = B().skillsTab.socketGroupList
	local metaGroups = {}
	for gi, g in ipairs(list) do if g.enabled ~= false and groupMeta(g) then metaGroups[gi] = true end end
	-- feeder: the main skill when it is manual and outside meta groups, else the strongest such skill
	local feeder
	for _, s in ipairs(skills) do
		if not metaGroups[s.group] and s.activation == "manual" and not s.err then
			if mainSel and s.group == mainSel.group and s.index == mainSel.skill then feeder = s break end
			if not feeder or (s.dps or 0) > (feeder.dps or 0) then feeder = s end
		end
	end
	local res = array({})
	local gis = {}
	for gi in pairs(metaGroups) do gis[#gis + 1] = gi end
	table.sort(gis)
	for _, gi in ipairs(gis) do
		local e = metaGroupFacts(gi)
		if e then
			local fo = feeder and feeder.offence
			e.feeder = feeder and {
				group = feeder.group, index = feeder.index, name = feeder.name, id = feeder.id, dps = feeder.dps,
				hitsPerSec = fo.hitsPerSec, speed = fo.speed, critChance = fo.critChance, shockChance = fo.shockChance,
				igniteChance = fo.igniteChance, freezeChance = fo.freezeChance, chillChance = fo.chillChance,
				electrocuteChance = fo.electrocuteChance,
				-- hits/s x PoB's per-hit chance; freeze is PoB's FreezeChance, not a buildup model
				critsPerSec = fo.hitsPerSec * fo.critChance / 100,
				shocksPerSec = fo.hitsPerSec * fo.shockChance / 100,
				ignitesPerSec = fo.hitsPerSec * fo.igniteChance / 100,
				freezesPerSec = fo.hitsPerSec * fo.freezeChance / 100,
				critHit = fo.critHit, fireHit = fo.fireHit, enemyAilmentThreshold = fo.enemyAilmentThreshold,
			} or null
			e.triggered = array({})
			for _, s in ipairs(skills) do
				-- skills granted by a support (Fiery Death's explosion, ...) have their own trigger condition
				-- (an enemy dying, ...); the meta gem never fires them, so they are not energy-triggered spells
				local bySupport = false
				for _, tn in ipairs(s.types or {}) do if tn == "SkillGrantedBySupport" then bySupport = true end end
				if s.group == gi and not s.err and not bySupport then
					e.triggered[#e.triggered + 1] = { index = s.index, name = s.name, id = s.id, dps = s.dps,
						pobRate = s.offence.speed, skillTriggerRate = s.offence.skillTriggerRate,
						effectiveSourceRate = s.offence.effectiveSourceRate, triggerRateCap = s.offence.triggerRateCap }
				end
			end
			res[#res + 1] = e
		end
	end
	return res
end

-- Sum of DPS over one group's non-meta damaging or proxy skills.
local function groupDps(gi, override)
	local env0 = calcWith(withOverride(override, gi), true)
	local tot = 0
	for si, as in ipairs(skillsByGroup(env0)[gi] or {}) do
		local f = skillFacts(as, gi, si)
		if (f.damaging or #f.proxyKind > 0) and f.enabled and not f.meta then
			local ok, _, o = calcSkill(override, gi, si, true)
			if ok then tot = tot + dpsOf(o) end
		end
	end
	return tot
end

-- Drop-one support marginals for the main group and every enabled meta (trigger) group.
local function marginals(override, main)
	local list = B().skillsTab.socketGroupList
	local groups = { main.group }
	for gi, g in ipairs(list) do if gi ~= main.group and g.enabled ~= false and groupMeta(g) then groups[#groups + 1] = gi end end
	local function mainDps()
		local ok, _, o = calcSkill(override, main.group, main.skill, true)
		return ok and dpsOf(o) or nil
	end
	local base = mainDps()
	local out = array({})
	for _, gi in ipairs(groups) do
		local g = list[gi]
		if g and not g.noSupports then
			local isMeta = groupMeta(g) ~= nil
			local gBase = isMeta and groupDps(gi, override) or nil
			for slot, gem in ipairs(g.gemList) do
				local ge = gem.gemData and gem.gemData.grantedEffect
				if ge and ge.support and gem.enabled ~= false then
					local undo = applyGemChanges({ { group = gi, slot = slot, enabled = false } })
					local okm, dw = pcall(mainDps)
					local okg, gw = true, nil
					if isMeta then okg, gw = pcall(groupDps, gi, override) end
					undoGemChanges(undo)
					out[#out + 1] = { group = gi, slot = slot, gem_id = ge.id, gemId = gem.gemData.id, name = gem.gemData.name,
						dps_without = okm and dw or null, dps_base = base or null,
						group_dps_without = (isMeta and okg) and gw or nil, group_dps_base = gBase }
				end
			end
		end
	end
	return out
end

-- The current live state (plus a node override), as the fields asked for.
local function evaluate(override, want, opts)
	opts = opts or {}
	local b = B()
	local main = S.main or { group = b.mainSocketGroup or 1, skill = 1 }
	local list = b.skillsTab.socketGroupList
	local gi = math.min(math.max(main.group or 1, 1), math.max(#list, 1))
	local ok, env, o = calcSkill(override, gi, main.skill, opts.skip_ehp == true)
	if not ok then error(env, 0) end
	local r = { profile = S.profile or null }
	local d, p, m = dpsOf(o)
	if want.mainSkill then r.mainSkill = mainSkillInfo(env.player.mainSkill) or null end
	if want.stats then
		r.stats = pickStats(o)
		-- PoB fills CharmLimit only in breakdown (MAIN) passes
		if IS_POE2 and r.stats.CharmLimit == nil and env.player.modDB then
			local mdb = env.player.modDB
			r.stats.CharmLimit = math.min(mdb:Override(nil, "CharmLimit") or mdb:Sum("BASE", nil, "CharmLimit"), 3)
		end
	end
	if want.minionStats then r.minionStats = o.Minion and pickStats(o.Minion) or null end
	if want.main then
		r.main = { group = gi, skill = main.skill or 1, selection = S.mainSel or "pob",
			name = env.player.mainSkill and skillName(env.player.mainSkill) or null }
	end
	if want.pobMain then
		local pg = S.pobMain or b.mainSocketGroup
		local pgr = list[pg or 1]
		r.pobMain = { group = opt(pg), skill = pgr and pgr.mainActiveSkill or null }
	end
	if want.dps then r.dps = { best = d, player = p, minion = m, full = num(o.FullDPS) } end
	if want.offence then
		r.offence = offenceFacts(o)
		if o.Minion then r.offence.minion = offenceFacts(o.Minion) end
	end
	if want.sustain then r.sustain = sustainFacts(o) end
	if want.movementSpeed then r.movementSpeed = num(o.MovementSpeedMod) or null end
	if want.ailmentThreshold then r.ailmentThreshold = { ailment = num(o.AilmentThreshold) or null, stun = num(o.StunThreshold) or null } end
	if want.spirit and IS_POE2 then r.spirit = { total = n0(o.Spirit), reserved = n0(o.SpiritReserved), unreserved = n0(o.SpiritUnreserved) } end
	if want.maxHit then r.maxHit = maxHitFacts(o) end
	if want.requirements then
		local req, deficit = {}, 0
		for _, a in ipairs({ "Str", "Dex", "Int" }) do
			local need, have = n0(o["Req" .. a]), n0(o[a])
			req[a] = { need = need, have = have }
			if need > have then deficit = deficit + need - have end
		end
		req.deficit = deficit
		r.requirements = req
	end
	if want.keystones or want.uniques then
		local ks, us = array({}), array({})
		for id, node in pairs(env.allocNodes or {}) do
			if node.type == "Keystone" then ks[#ks + 1] = { id = id, name = node.dn or node.name, granted = (env.grantedPassives or {})[id] and true or nil } end
		end
		table.sort(ks, function(a, c) return a.name < c.name end)
		local slots = {}
		for slot in pairs(env.player.itemList or {}) do slots[#slots + 1] = slot end
		table.sort(slots)
		for _, slot in ipairs(slots) do
			local item = env.player.itemList[slot]
			if item and (item.rarity == "UNIQUE" or item.rarity == "RELIC") then us[#us + 1] = { name = item.title or item.name, slot = slot, base = item.baseName } end
		end
		if want.keystones then r.keystones = ks end
		if want.uniques then r.uniques = us end
	end
	if want.unsupported then r.unsupported = unsupportedLines(env) end
	if want.marginals then r.marginals = marginals(override, { group = gi, skill = main.skill }) end
	if want.skills or want.energy then
		local skills = perSkill(override, env)
		if want.skills then r.skills = skills end
		if want.energy and IS_POE2 then r.energy = energyFacts(skills, main) end
	end
	return r
end

-- ---------------------------------------------------------------------------
-- Main skill selection
-- ---------------------------------------------------------------------------

local function selectMain(sel)
	local b = B()
	local list = b.skillsTab.socketGroupList
	S.mainSel = sel or S.mainSel or "auto"
	if S.mainSel == "pob" then
		local gi = S.pobMain or b.mainSocketGroup or 1
		S.main = { group = gi, skill = list[gi] and list[gi].mainActiveSkill or 1 }
	elseif type(S.mainSel) == "number" or tonumber(S.mainSel) then
		local gi = tonumber(S.mainSel)
		S.main = { group = gi, skill = list[gi] and list[gi].mainActiveSkill or 1 }
	elseif type(S.mainSel) == "table" then
		S.main = { group = tonumber(S.mainSel.group) or 1, skill = tonumber(S.mainSel.skill) or 1 }
		S.mainSel = "fixed"
	else
		S.mainSel = "auto"
		local env = calcWith({ mainSocketGroup = b.mainSocketGroup }, true)
		-- the strongest skill the player chose: not PoB's synthetic mechanic groups, and a weapon's
		-- default attack only when the player socketed supports into it
		local best, fallback
		for _, s in ipairs(perSkill(nil, env)) do
			if not s.err and (s.dps or 0) > 0 and s.grantKind ~= "mechanic" then
				if s.weaponDefault and s.groupSupports == 0 and s.group ~= S.pobMain then
					if not fallback or s.dps > fallback.dps then fallback = s end
				elseif not best or s.dps > best.dps then best = s end
			end
		end
		best = best or fallback
		if best then S.main = { group = best.group, skill = best.index }
		else
			local gi = S.pobMain or b.mainSocketGroup or 1
			S.main = { group = gi, skill = (list[gi] or {}).mainActiveSkill or 1 }
		end
	end
	-- PoB's own marker follows, so an export opens on the scored skill
	local g = list[S.main.group]
	if g then
		U.calcsSelection.setMainSocketGroup(S.main.group)
		U.calcsSelection.setMainActiveSkill(g, S.main.skill)
	end
	S.dirty = true
	return S.main
end

-- ---------------------------------------------------------------------------
-- Profiles
-- ---------------------------------------------------------------------------

local function applyProfile(name)
	local prof = profileDef(name)
	if not prof then error("unknown profile " .. tostring(name), 0) end
	local ct = B().configTab
	if prof.base == "player" then
		ct:SetActiveConfigSet(S.playerConfigSetId, false, true)
		for k, v in pairs(prof.config or {}) do ct.input[k] = v end
	else
		local id = S.profileSets[name]
		if not (id and ct.configSets[id]) then
			-- a fresh PoB-default set, carrying the player's character-state inputs
			local old = ct.configSets[S.playerConfigSetId]
			local cs = (ct.CreateConfigSet or ct.NewConfigSet)(ct, nil, "search_" .. name)
			for k, v in pairs(prof.default_config or {}) do cs.input[k] = v end
			for k, v in pairs(old and old.input or {}) do
				if type(k) == "string" then
					for _, pre in ipairs(prof.carry_prefixes or {}) do
						if k:sub(1, #pre) == pre then cs.input[k] = v end
					end
				end
			end
			for k, v in pairs(prof.config or {}) do cs.input[k] = v end
			if not isValueInArray(ct.configSetOrderList, cs.id) then table.insert(ct.configSetOrderList, cs.id) end
			S.profileSets[name] = cs.id
			id = cs.id
		end
		ct:SetActiveConfigSet(id, false, true)
	end
	-- twice: list options (enemyIsBoss) set placeholders that later options read
	ct:BuildModList()
	ct:BuildModList()
	S.profile = name
	S.dirty = true
end

local function profileScoring(name)
	local prof = profileDef(name or S.profile or "")
	return prof and prof.scoring or null
end

-- ---------------------------------------------------------------------------
-- Tree
-- ---------------------------------------------------------------------------

local function nodeKind(node)
	if node.type == "ClassStart" then return "start" end
	if node.type == "AscendClassStart" then return "ascendancy_start" end
	if node.ascendancyName then return node.type == "Notable" and "ascendancy_notable" or "ascendancy" end
	if node.type == "Keystone" then return "keystone" end
	if node.type == "Notable" then return "notable" end
	if node.type == "Socket" then return "jewel" end
	if node.type == "Mastery" then return "mastery" end
	if node.isAttribute then return "attribute" end
	return "small"
end

local function attrChoice(spec, id, node)
	local tn = spec.tree.nodes[id]
	if not (tn and tn.isAttribute) then return nil end
	for i, o in ipairs(tn.options or {}) do if o.dn == node.dn then return i end end
	return 0
end

-- Granted by an item (PoE2 "Allocates N Sinister Jewel sockets"): PoB keeps these allocated without a path.
local function isGrantedNode(node)
	return node.isGrantedPassive and node.isFreeAllocate and true or false
end

-- Path-free sources of a node: allocated jewel sockets (Intuitive Leap-like, From Nothing) or keystones
-- (AllocateFromNodeRadius) that PoB lists in node.intuitiveLeapLikesAffecting.
local function leapSources(node)
	local out
	for _, src in ipairs(node.intuitiveLeapLikesAffecting or {}) do
		out = out or array({})
		out[#out + 1] = src.id
	end
	return out
end

local function nodeFacts(spec, id, node, withLinks)
	local f = { id = id, name = node.dn or node.name, kind = nodeKind(node), type = node.type,
		sd = array({}), asc = node.ascendancyName or nil, attribute = node.isAttribute or nil,
		leap = leapSources(node), granted = isGrantedNode(node) or nil }
	for i, l in ipairs(node.sd or {}) do f.sd[i] = l end
	if node.isAttribute then
		local tn = spec.tree.nodes[id]
		f.options = array({})
		for i, o in ipairs(tn and tn.options or {}) do f.options[i] = o.dn end
	end
	if withLinks then
		f.linked = array({})
		for _, o in ipairs(node.linked or {}) do f.linked[#f.linked + 1] = o.id end
	end
	return f
end

local function pointInfo(env)
	local b = B()
	local spec = b.spec
	local used, ascUsed, secAscUsed, sockets, ws1, ws2 = U.countAllocNodes(spec)
	local mo = b.calcsTab.mainOutput or {}
	local extra = env and env.modDB:Sum("BASE", nil, "ExtraPoints") or n0(mo.ExtraPoints)
	local level = b.characterLevel or 1
	local quest = 0
	if IS_POE2 and b.acts then
		for _, a in ipairs(b.acts) do if level >= (a.level or 0) then quest = math.max(quest, a.questPoints or 0) end end
		if level >= 60 then quest = math.max(quest, b.maxWeaponSets or quest) end
	else
		quest = U.questPointsForLevel(level, extra)
	end
	local wsMax = (b.maxWeaponSets or 0) + (env and env.modDB:Sum("BASE", nil, "PassivePointsToWeaponSetPoints") or n0(mo.PassivePointsToWeaponSetPoints))
	return {
		used = used, normalUsed = used - math.min(ws1, ws2), ascUsed = ascUsed, secondaryAscUsed = secAscUsed, sockets = sockets,
		weaponSet1 = ws1, weaponSet2 = ws2, weaponSetMax = wsMax,
		available = (level - 1) + quest + extra, levelPoints = level - 1, questPoints = quest, extraPoints = extra,
		ascMax = 8,
	}
end

local function treeView(p)
	local b = B()
	local spec = b.spec
	local env = calcWith({ mainSocketGroup = (S.main and S.main.group) or b.mainSocketGroup }, true)
	local granted = env.grantedPassives or {}
	local maxDist = tonumber(p.max_dist) or 1000
	local nodes = {}
	local alloc, asc, ws, attrs, jewels = array({}), array({}), {}, {}, array({})
	local pathFree, grantedAlloc = {}, array({})
	for id, node in pairs(spec.allocNodes) do
		if node.type ~= "ClassStart" and node.type ~= "AscendClassStart" then
			if isGrantedNode(node) then grantedAlloc[#grantedAlloc + 1] = id
			elseif node.ascendancyName then asc[#asc + 1] = id
			else alloc[#alloc + 1] = id end
			if (node.allocMode or 0) ~= 0 then ws[tostring(id)] = node.allocMode end
			local c = attrChoice(spec, id, node)
			if c then attrs[tostring(id)] = c end
			local leap = leapSources(node)
			if leap then pathFree[tostring(id)] = leap end
			nodes[tostring(id)] = nodeFacts(spec, id, node, p.graph)
		end
		if node.type == "Socket" then
			local itemId = spec.jewels and spec.jewels[id]
			local item = itemId and itemId ~= 0 and b.itemsTab.items[itemId]
			jewels[#jewels + 1] = { id = id, item = item and (item.title or item.name) or null, granted = isGrantedNode(node) or nil,
				pathFree = item and item.jewelData and (item.jewelData.intuitiveLeapLike or item.jewelData.fromNothingKeystone) and true or nil }
		end
	end
	table.sort(alloc); table.sort(asc); table.sort(grantedAlloc)
	local grantedIds = array({})
	for id, v in pairs(granted) do
		if v then
			grantedIds[#grantedIds + 1] = id
			local node = spec.nodes[id]
			if node and not nodes[tostring(id)] then nodes[tostring(id)] = nodeFacts(spec, id, node, p.graph) end
		end
	end
	table.sort(grantedIds)
	local frontier = array({})
	if p.frontier ~= false then
		for id, node in pairs(spec.nodes) do
			if not node.alloc and node.modKey ~= nil and node.type ~= "ClassStart" and node.type ~= "AscendClassStart"
				and not node.isProxy and (node.pathDist or 1000) <= maxDist and (node.pathDist or 1000) < 1000
				and (p.ascendancy ~= false or not node.ascendancyName) then
				local path = array({})
				for _, pn in ipairs(node.path or {}) do path[#path + 1] = pn.id end
				local w = node.pathRoot and (node.pathRoot.allocMode or 0) or 0
				frontier[#frontier + 1] = { id = id, dist = node.pathDist, path = path, granted = granted[id] and true or nil,
					ws = w ~= 0 and w or nil, leap = leapSources(node) }
				nodes[tostring(id)] = nodes[tostring(id)] or nodeFacts(spec, id, node, p.graph)
			end
		end
		table.sort(frontier, function(a, c) if a.dist ~= c.dist then return a.dist < c.dist end return a.id < c.id end)
	end
	if p.graph then
		-- the whole tree with adjacency, for graph solvers
		for id, node in pairs(spec.nodes) do
			if (not node.ascendancyName or p.ascendancy ~= false) and not node.isProxy and (node.group ~= nil or node.type == "ClassStart") and not nodes[tostring(id)] then
				nodes[tostring(id)] = nodeFacts(spec, id, node, true)
			end
		end
	end
	return {
		classId = spec.curClassId, className = spec.curClassName, ascendClassId = spec.curAscendClassId,
		ascendancy = opt(spec.curAscendClassName), startNode = spec.curClass and spec.curClass.startNodeId or null,
		treeVersion = spec.treeVersion,
		allocated = alloc, ascendancyNodes = asc, weaponSetNodes = ws, attributeChoices = attrs,
		grantedNodes = grantedIds, grantedAllocated = grantedAlloc, pathFree = pathFree,
		jewelSockets = jewels, frontier = frontier, points = pointInfo(env), nodes = nodes,
	}
end

local function attrVariant(id, choice)
	local key = id * 10 + choice
	if S.variants[key] then return S.variants[key] end
	local spec = B().spec
	local tn = spec.tree.nodes[id]
	if not (tn and tn.isAttribute and tn.options and tn.options[choice]) then error("node " .. id .. " has no attribute option " .. tostring(choice), 0) end
	local nn = copyTableSafe(tn, false, true)
	spec:ReplaceNode(nn, nn.options[choice])
	spec.tree:ProcessStats(nn)
	nn.id = id
	S.variants[key] = nn
	return nn
end

local function parseAdd(e)
	if type(e) == "table" then return tonumber(e.id), tonumber(e.ws or e.weapon_set) end
	return tonumber(e), nil
end

local function isStart(node) return node.type == "ClassStart" or node.type == "AscendClassStart" end

-- The main-tree allocation after removing and adding nodes, by PoB's connectivity rules
-- (BuildAllDependsAndPaths prunes nodes that lose their path to the class start; a normal node
-- paths through normal nodes only, a weapon-set node through normal nodes and its own set).
-- An added node without ws gets the first mode that connects it: 0, then 1, then 2.
-- A node with a path-free source (PoB's intuitiveLeapLikesAffecting) is kept while a source is kept,
-- and item-granted nodes are left to PoB. Ascendancy nodes pass through unchecked.
-- Returns kept {id -> mode}, current {id -> mode}, pruned {ids}.
local function resolveAlloc(addList, removeSet)
	local spec = B().spec
	local mode, cur = {}, {}
	local fixed = {}
	for id, node in pairs(spec.allocNodes) do
		if isGrantedNode(node) or node.ascendancyName or isStart(node) then
			if not removeSet[id] then fixed[id] = true end
		else
			cur[id] = node.allocMode or 0
			if not removeSet[id] then mode[id] = node.allocMode or 0 end
		end
	end
	local asked = {}
	for _, e in ipairs(addList or {}) do
		local id, w = parseAdd(e)
		local node = id and spec.nodes[id]
		if not node then error("unknown node " .. tostring(id), 0) end
		if node.ascendancyName or isStart(node) then
			fixed[id] = true
		elseif not isGrantedNode(node) then
			asked[id] = true
			if w then mode[id] = w elseif mode[id] == nil then mode[id] = false end
		end
	end
	local starts = {}
	local startId = spec.curClass and spec.curClass.startNodeId
	if startId and spec.nodes[startId] then starts[#starts + 1] = spec.nodes[startId] end
	local startMap = spec.tree.classStartNodeNameMap
	for socketId, itemId in pairs(spec.jewels or {}) do
		if mode[socketId] ~= nil and startMap and spec.GetJewel then
			local item = spec:GetJewel(itemId)
			local cn = item and item.jewelData and item.jewelData.alternateClassStart
			local sid = cn and startMap[cn]
			if sid and spec.nodes[sid] then starts[#starts + 1] = spec.nodes[sid] end
		end
	end
	local kept = {}
	for _, m in ipairs({ 0, 1, 2 }) do
		local seen, q, qi = {}, {}, 1
		for _, sn in ipairs(starts) do q[#q + 1] = sn; seen[sn.id] = true end
		while qi <= #q do
			local u = q[qi]; qi = qi + 1
			if u.type ~= "Mastery" then
				for _, v in ipairs(u.linked or {}) do
					local id = v.id
					local mv = mode[id]
					if mv ~= nil and not seen[id] and not v.ascendancyName then
						local ok
						if mv == false then ok = kept[id] == nil
						elseif mv == 0 then ok = (m == 0) or kept[id] == 0
						else ok = (mv == m) end
						if ok then seen[id] = true; q[#q + 1] = v end
					end
				end
			end
		end
		for id in pairs(seen) do
			if (mode[id] == false or mode[id] == m) and kept[id] == nil then kept[id] = m end
		end
	end
	-- path-free nodes: kept while any of their sources is kept, which can chain through sockets
	local changed = true
	while changed do
		changed = false
		for id, mv in pairs(mode) do
			if kept[id] == nil then
				for _, src in ipairs(spec.nodes[id].intuitiveLeapLikesAffecting or {}) do
					if kept[src.id] ~= nil or fixed[src.id] then
						kept[id] = mv or 0
						changed = true
						break
					end
				end
			end
		end
	end
	local pruned = array({})
	for id in pairs(mode) do if kept[id] == nil and (cur[id] ~= nil or asked[id]) then pruned[#pruned + 1] = id end end
	table.sort(pruned)
	return kept, cur, pruned
end

local function attrChanges(p)
	local out = {}
	for k, c in pairs(not isnull(p.attr_choices) and p.attr_choices or {}) do
		local id, ch = tonumber(k), tonumber(c)
		if id and ch then out[id] = ch end
	end
	return out
end

local function treeChange(p)
	local addL = not isnull(p.add_nodes) and p.add_nodes or {}
	local remL = not isnull(p.remove_nodes) and p.remove_nodes or {}
	local attrs = attrChanges(p)
	if #addL == 0 and #remL == 0 and next(attrs) == nil then return nil end
	local removeSet = {}
	for _, e in ipairs(remL) do removeSet[tonumber(e)] = true end
	return addL, remL, attrs, removeSet
end

-- Calc override for a tree what-if: override, cleanup(), pruned.
local function nodeOverride(p)
	local spec = B().spec
	local addL, remL, attrs, removeSet = treeChange(p)
	if not addL then return {}, function() end, nil end
	local kept, cur, pruned = resolveAlloc(addL, removeSet)
	local add, rem = nil, nil
	local restoreModes = {}
	local function addNode(node, m)
		if (node.allocMode or 0) ~= m then restoreModes[node] = node.allocMode; node.allocMode = m end
		add = add or {}; add[node] = true
	end
	for id, m in pairs(kept) do
		local node = spec.nodes[id]
		local c = attrs[id]
		local changeAttr = node.isAttribute and c and (cur[id] == nil or attrChoice(spec, id, node) ~= c)
		if cur[id] == nil then
			addNode(changeAttr and attrVariant(id, c) or node, m)
		elseif changeAttr or cur[id] ~= m then
			rem = rem or {}; rem[node] = true
			addNode(changeAttr and attrVariant(id, c) or copyTableSafe(node, false, true), m)
		end
	end
	for id in pairs(cur) do
		if kept[id] == nil then rem = rem or {}; rem[spec.nodes[id]] = true end
	end
	for _, e in ipairs(addL) do
		local node = spec.nodes[parseAdd(e)]
		if node and node.ascendancyName and not node.alloc then add = add or {}; add[node] = true end
	end
	for _, e in ipairs(remL) do
		local node = spec.nodes[tonumber(e)]
		if node and node.ascendancyName and node.alloc then rem = rem or {}; rem[node] = true end
	end
	return { addNodes = add, removeNodes = rem }, function()
		for node, m in pairs(restoreModes) do node.allocMode = m end
	end, pruned
end

-- Restores a spec undo state through PoB's RestoreUndoState. Item-granted nodes are left out of the
-- node list and handed back to SetGrantedPassiveNodes, as PoB's own calc pass grants them.
local function restoreTree(st)
	local spec = B().spec
	for id, node in pairs(spec.nodes) do
		local tn = spec.tree.nodes[id]
		if tn and tn.isAttribute and not st.hashOverrides[id] and node.dn ~= tn.dn then spec:ReplaceNode(node, tn) end
	end
	local granted, list = {}, {}
	for _, id in ipairs(st.hashList) do
		local node = spec.nodes[id]
		if node and (st.granted and st.granted[id]) then granted[id] = node else list[#list + 1] = id end
	end
	local full = {}
	for k, v in pairs(st) do full[k] = v end
	full.hashList = list
	spec:RestoreUndoState(full)
	if next(granted) and spec.SetGrantedPassiveNodes then spec:SetGrantedPassiveNodes(granted) end
end

-- spec:CreateUndoState plus the set of item-granted nodes in it.
local function treeState()
	local spec = B().spec
	local st = spec:CreateUndoState()
	st.granted = {}
	for id, node in pairs(spec.allocNodes) do if isGrantedNode(node) then st.granted[id] = true end end
	return st
end

local function applyTree(p)
	local spec = B().spec
	local addL, _, attrs, removeSet = treeChange(p)
	if not addL then return nil end
	local kept, _, pruned = resolveAlloc(addL, removeSet)
	local st = treeState()
	local list, modes = {}, {}
	for _, id in ipairs(st.hashList) do
		local node = spec.nodes[id]
		if node and (node.ascendancyName or isStart(node) or st.granted[id]) and not removeSet[id] then
			list[#list + 1] = id
			if (node.allocMode or 0) ~= 0 then modes[id] = node.allocMode end
		end
	end
	for _, e in ipairs(addL) do
		local id = parseAdd(e)
		local node = spec.nodes[id]
		if node and node.ascendancyName and not node.alloc then list[#list + 1] = id end
	end
	for id, m in pairs(kept) do
		list[#list + 1] = id
		if m ~= 0 then modes[id] = m end
	end
	for id in pairs(st.hashOverrides) do
		local tn = spec.tree.nodes[id]
		if tn and tn.isAttribute and kept[id] == nil then st.hashOverrides[id] = nil end
	end
	for id, c in pairs(attrs) do
		local tn = spec.tree.nodes[id]
		if tn and tn.isAttribute and kept[id] ~= nil then
			if not (tn.options and tn.options[c]) then error("node " .. id .. " has no attribute option " .. c, 0) end
			local nn = copyTableSafe(tn, false, true)
			spec:ReplaceNode(nn, nn.options[c])
			spec.tree:ProcessStats(nn)
			st.hashOverrides[id] = nn
		end
	end
	st.hashList, st.weaponSets = list, modes
	restoreTree(st)
	return pruned
end

-- ---------------------------------------------------------------------------
-- Skills view and valid supports
-- ---------------------------------------------------------------------------

local function skillsView()
	local b = B()
	local list = b.skillsTab.socketGroupList
	local env = calcWith({ mainSocketGroup = (S.main and S.main.group) or b.mainSocketGroup }, true)
	local by = skillsByGroup(env)
	local groups = array({})
	for gi, g in ipairs(list) do
		local gems = array({})
		for slot, gem in ipairs(g.gemList) do
			local gd = gem.gemData
			local ge = gem.grantedEffect or (gd and gd.grantedEffect)
			local tier = gemTier(gd)
			gems[#gems + 1] = {
				slot = slot, id = gd and gd.id or null, skillId = gem.skillId or (ge and ge.id) or null,
				name = (gd and gd.name) or gem.nameSpec or "?", level = gem.level, quality = gem.quality,
				enabled = gem.enabled ~= false, support = (ge and ge.support) and true or false, known = gd ~= nil,
				tags = keysSorted(gd and gd.tags), gemTypes = typeNames(ge and ge.skillTypes),
				attr = opt(gd and gemUtil.gemAttr(gd)), tier = opt(tier), reqLevel = opt(gd and gemUtil.gemReqLevel(tier, gd)),
				family = copyArr(ge and ge.gemFamily),
				meta = isMetaGemEffect(ge) or false,
			}
		end
		local skills = array({})
		for si, as in ipairs(by[gi] or {}) do skills[#skills + 1] = skillFacts(as, gi, si) end
		local src = g.sourceItem
		groups[#groups + 1] = {
			index = gi, label = (g.label and g.label ~= "") and g.label or null, slot = opt(g.slot),
			enabled = g.enabled ~= false, main = S.main and S.main.group == gi or false, pobMain = S.pobMain == gi,
			mainActiveSkill = g.mainActiveSkill or 1, includeInFullDPS = g.includeInFullDPS == true,
			grantedBy = g.source and ((src and (src.title or src.name)) or tostring(g.source)) or null,
			noSupports = g.noSupports and true or false,
			metaGroup = groupMeta(g) ~= nil, gems = gems, skills = skills,
		}
	end
	return { groups = groups, main = S.main or null, characterLevel = b.characterLevel }
end

local function validSupports(p)
	local b = B()
	local list = b.skillsTab.socketGroupList
	local gi = tonumber(p.group)
	local g = gi and list[gi]
	if not g then error("unknown group " .. tostring(p.group), 0) end
	local env = calcWith({ mainSocketGroup = gi }, true)
	local skills = skillsByGroup(env)[gi] or {}
	local si = tonumber(p.skill) or g.mainActiveSkill or 1
	-- a meta group's supports apply to its socketed skills
	local active = skills[si]
	if active and isMetaGemEffect(active.activeEffect.grantedEffect) then
		for _, as in ipairs(skills) do if not isMetaGemEffect(as.activeEffect.grantedEffect) then active = as break end end
	end
	if not active then return { group = gi, supports = array({}), err = "no active skill in group" } end
	local level = b.characterLevel or 100
	local usedElsewhere = {}
	for j, g2 in ipairs(list) do
		if j ~= gi and g2.enabled ~= false then
			for _, gem in ipairs(g2.gemList or {}) do
				-- PoE2 0.3.0+: any support can be used on many skills; only lineage supports and supports that
				-- reserve Spirit are limited to one copy (per rank) across all skills (wiki: Gem#Support gems)
				if gem.gemData and gem.enabled ~= false and oneCopyOnly(gem.gemData) then usedElsewhere[gem.gemData.id] = j end
			end
		end
	end
	local cur = {}
	for slot, gem in ipairs(g.gemList) do
		local ge = gem.gemData and gem.gemData.grantedEffect
		if ge and ge.support and gem.enabled ~= false then cur[#cur + 1] = { slot = slot, gd = gem.gemData } end
	end
	local function conflicts(ga, gb)
		if ga.id == gb.id then return "same" end
		for _, x in ipairs(ga.gemFamily or {}) do for _, y in ipairs(gb.gemFamily or {}) do if x == y then return "family" end end end
		if ga.plusVersionOf == gb.id or gb.plusVersionOf == ga.id then return "lineage" end
		return nil
	end
	local out = array({})
	for id, gd in pairs(data.gems) do
		local ge = gd.grantedEffect
		local legacyHidden = not IS_POE2 and ge and ge.legacy and not b.skillsTab.showLegacyGems
		if ge and ge.support and not ge.hidden and not legacyHidden then
			local tier = gemTier(gd)
			local req = gemUtil.gemReqLevel(tier, gd)
			if not (req and req > level) and (not usedElsewhere[id] or p.include_used == true) then
				local ok, s = pcall(calcLib.canGrantedEffectSupportActiveSkill, ge, active)
				if ok and s then
					local cf = array({})
					local inGroup = false
					for _, c in ipairs(cur) do
						local why = conflicts(ge, c.gd.grantedEffect)
						if why then cf[#cf + 1] = { slot = c.slot, why = why } end
						if c.gd.id == id then inGroup = true end
					end
					out[#out + 1] = {
						id = id, skillId = gd.grantedEffectId, name = gd.name, tier = opt(tier), reqLevel = opt(req),
						attr = opt(gemUtil.gemAttr(gd)), attrCost = IS_POE2 and 5 or 0, socketed = inGroup,
						usedInGroup = opt(usedElsewhere[id]), conflicts = cf,
						family = copyArr(ge.gemFamily),
						lineage = (gd.name or ""):find("'") ~= nil,
						tags = keysSorted(gd.tags),
					}
				end
			end
		end
	end
	table.sort(out, function(a, c) return a.name < c.name end)
	local slots = array({})
	for _, c in ipairs(cur) do slots[#slots + 1] = c.slot end
	return { group = gi, skill = si, activeSkill = active.activeEffect.grantedEffect.name, characterLevel = level, slots = slots, supports = out }
end

-- ---------------------------------------------------------------------------
-- Snapshots
-- ---------------------------------------------------------------------------

local function copyItemState(st)
	local c = {}
	for k, v in pairs(st) do c[k] = v end
	c.items = {}
	for k, v in pairs(st.items) do c.items[k] = copyTableSafe(v, true, true) end
	c.itemOrderList = copyTable(st.itemOrderList)
	c.itemSets = copyTableSafe(st.itemSets)
	c.itemSetOrderList = copyTable(st.itemSetOrderList)
	return c
end

local function snapshot(withItems)
	local b = B()
	local list = b.skillsTab.socketGroupList
	local snap = { tree = treeState(), jewels = copyTable(b.spec.jewels or {}), groupList = {}, groups = {},
		mainSocketGroup = b.mainSocketGroup, main = S.main and { group = S.main.group, skill = S.main.skill } or nil,
		mainSel = S.mainSel, profile = S.profile, configSetId = b.configTab.activeConfigSetId, level = b.characterLevel }
	-- PoB's own items undo state: every item, slot selection (jewel sockets included) and item set
	if withItems then snap.items = b.itemsTab:CreateUndoState() end
	for i, g in ipairs(list) do
		snap.groupList[i] = g
		local gems = {}
		for j, gem in ipairs(g.gemList) do gems[j] = gem end
		snap.groups[i] = { gems = gems, enabled = g.enabled, mas = g.mainActiveSkill }
	end
	local token = "s" .. S.nextToken
	S.nextToken = S.nextToken + 1
	S.snapshots[token] = snap
	return token
end

local function restore(token)
	local snap = S.snapshots[token]
	if not snap then error("unknown snapshot token " .. tostring(token), 0) end
	local b = B()
	if snap.items then
		b.itemsTab:RestoreUndoState(copyItemState(snap.items))
		b.spec.jewels = b.spec.jewels or {}
		for k in pairs(b.spec.jewels) do b.spec.jewels[k] = nil end
		for k, v in pairs(snap.jewels) do b.spec.jewels[k] = v end
	end
	local st = b.skillsTab
	local list = st.socketGroupList
	for i = #list, 1, -1 do list[i] = nil end
	for i, g in ipairs(snap.groupList) do
		list[i] = g
		local s = snap.groups[i]
		for j = #g.gemList, 1, -1 do g.gemList[j] = nil end
		for j, gem in ipairs(s.gems) do g.gemList[j] = gem end
		g.enabled, g.mainActiveSkill = s.enabled, s.mas
		if not g.source then st:ProcessSocketGroup(g) end
	end
	b.mainSocketGroup = snap.mainSocketGroup
	S.main, S.mainSel = snap.main, snap.mainSel
	if b.characterLevel ~= snap.level then b.characterLevel = snap.level end
	restoreTree(snap.tree)
	if snap.configSetId ~= b.configTab.activeConfigSetId then
		b.configTab:SetActiveConfigSet(snap.configSetId, false, true)
		b.configTab:BuildModList()
		b.configTab:BuildModList()
	end
	S.profile = snap.profile
	S.dirty = true
	return true
end

-- ---------------------------------------------------------------------------
-- Methods
-- ---------------------------------------------------------------------------

local function summary()
	local b = B()
	local spec = b.spec
	local list = b.skillsTab.socketGroupList
	local mg = S.main and list[S.main.group]
	return {
		game = U.GAME,
		className = spec.curClassName, ascendancy = opt(spec.curAscendClassName), level = b.characterLevel,
		treeVersion = spec.treeVersion, targetVersion = opt(b.targetVersion),
		profile = opt(S.profile), main = opt(S.main), mainSelection = S.mainSel,
		pobMainGroup = opt(S.pobMain), groups = #list,
		mainLabel = mg and (mg.displayLabel or mg.label) or null,
		points = pointInfo(),
	}
end

local function whatif(p, defaultFields)
	local want = wantSet(p.fields, defaultFields or { "core" })
	if p.marginals == true then want.marginals = true end
	local gc, ge = gemChangesOf(p)
	local undo
	if gc or ge then
		local u, err = applyGemChanges(gc, ge)
		if err then undoGemChanges(u); error(err, 0) end
		undo = u
	end
	local okN, ov, cleanup, pruned = pcall(nodeOverride, p)
	if not okN then undoGemChanges(undo); error(ov, 0) end
	local ok, r = pcall(evaluate, ov, want, p)
	cleanup()
	undoGemChanges(undo)
	if not ok then error(r, 0) end
	if pruned and #pruned > 0 then r.pruned = pruned end
	return r
end

local function mainParam(p)
	if not isnull(p.main) then return p.main end
	return nil
end

-- Open a build for a search session: load_build_code / load_build_xml, then a profile and a main skill.
-- params: { code | xml, name?, profile? (name, default as_exported), main? ("auto" default | "pob" | group | {group, skill}), gc? ("auto" | "full" | "none") }
-- returns: the search_summary of the loaded build
M.search_load = function(p)
	p = p or {}
	if p.gc ~= "none" then
		if p.gc == "full" or collectgarbage("count") / 1024 > 600 then collectgarbage("collect") else collectgarbage("step", 0) end
	end
	if type(p.code) == "string" and p.code ~= "" then
		M.load_build_code({ code = p.code, name = p.name })
	elseif type(p.xml) == "string" and p.xml ~= "" then
		M.load_build_xml({ xml = p.xml, name = p.name })
	else
		error("params.code or params.xml is required", 0)
	end
	if wipeGlobalCache then wipeGlobalCache() end
	resetState()
	if type(p.profile) == "string" and p.profile ~= "as_exported" then applyProfile(p.profile) end
	selectMain(mainParam(p) or "auto")
	return summary()
end

-- Class, level, points, profile and main skill of the session.
M.search_summary = function()
	session()
	return summary()
end

-- The shipped profiles, any the caller defined with search_set_profile, and the active one.
M.search_profiles = function()
	session()
	local all = {}
	for k, v in pairs(PROFILES) do all[k] = v end
	for k, v in pairs(S.profiles) do all[k] = v end
	return { profiles = all, current = opt(S.profile) }
end

-- Switch the evaluation profile. A fresh profile gets its own config set (titled search_<name>), made once.
-- params: { name, profile? (a profile table to define or replace `name`), main? }
-- returns: { profile, scoring, main }
M.search_set_profile = function(p)
	session()
	p = p or {}
	local name = p.name or (type(p.profile) == "string" and p.profile) or nil
	if not name then error("params.name is required", 0) end
	if type(p.profile) == "table" then
		S.profiles[name] = p.profile
		S.profileSets[name] = nil
	end
	applyProfile(name)
	if mainParam(p) then selectMain(p.main)
	elseif S.mainSel == "auto" then selectMain("auto") end
	return { profile = S.profile, scoring = profileScoring(S.profile), main = S.main }
end

-- Choose the scored skill and move PoB's main-skill marker to it.
-- params: { main: "auto" (highest DPS) | "pob" (the exported marker) | group | {group, skill} }
M.search_select_main = function(p)
	session()
	local m = selectMain(mainParam(p or {}) or "auto")
	return { main = m, selection = S.mainSel }
end

-- Stats of the current state. params: { fields? (default "all"; "core", "all" or top-level keys), skip_ehp?, marginals? }
-- returns: the stats record (SEARCH_API.md) plus the profile's `scoring`
M.search_stats = function(p)
	session()
	p = p or {}
	local want = wantSet(p.fields, { "all" })
	if p.marginals == true then want.marginals = true end
	local r = evaluate(nil, want, p)
	r.scoring = profileScoring(S.profile)
	return r
end

-- Every socket group with its gems and calculated skills (activation class, proxy kind, grant kind).
M.search_skills = function()
	session()
	return skillsView()
end

-- Supports PoB lets the group's active skill take, with conflicts against what is socketed.
-- params: { group, skill?, include_used? }
M.search_valid_supports = function(p)
	session()
	return validSupports(p or {})
end

-- Allocation, frontier (unallocated nodes with their PoB path), path-free sources and points.
-- params: { frontier? (true), max_dist?, graph? (false), ascendancy? (true) }
M.search_tree = function(p)
	session()
	return treeView(p or {})
end

-- Evaluate a change without keeping it. params: change (SEARCH_API.md) plus { fields? ("core"), skip_ehp?, marginals? }
-- returns: the stats record, with `pruned` when the tree change drops nodes
M.search_whatif = function(p)
	session()
	return whatif(p or {})
end

-- Several what-ifs from the same state. params: { variants: [change], fields?, skip_ehp? }
-- returns: one stats record per variant; a failed variant gives { err }
M.search_whatif_many = function(p)
	session()
	p = p or {}
	local res = array({})
	for i, v in ipairs(p.variants or {}) do
		if isnull(v.fields) and not isnull(p.fields) then v.fields = p.fields end
		if v.skip_ehp == nil and p.skip_ehp ~= nil then v.skip_ehp = p.skip_ehp end
		local ok, r = pcall(whatif, v)
		res[i] = ok and r or { err = tostring(r) }
	end
	return res
end

-- Apply a change and keep it. The tree goes through PoB's RestoreUndoState, gems through ProcessSocketGroup.
-- params: change plus { fields? ("core"; false returns { applied, pruned }), skip_ehp? }
M.search_apply = function(p)
	session()
	p = p or {}
	local gc, ge = gemChangesOf(p)
	if gc or ge then
		local u, err = applyGemChanges(gc, ge)
		if err then undoGemChanges(u); error(err, 0) end
	end
	local pruned = applyTree(p)
	S.dirty = true
	if p.fields == false then return { applied = true, pruned = pruned or array({}) } end
	local r = evaluate(nil, wantSet(p.fields, { "core" }), p)
	if pruned and #pruned > 0 then r.pruned = pruned end
	return r
end

-- Keep the tree, gems, main skill, config set and (unless items = false) items, to come back to.
-- params: { items? (true) } returns: { token }
M.search_snapshot = function(p)
	session()
	return { token = snapshot(not (p and p.items == false)) }
end

-- Return to a snapshot. Tokens last until the build changes. params: { token }
M.search_restore = function(p)
	session()
	return { restored = restore(p and p.token) }
end

M.search_drop_snapshot = function(p)
	session()
	S.snapshots[p and p.token or ""] = nil
	return { dropped = true }
end

-- A share code of the current state, after one PoB refresh when the session changed anything.
M.search_export = function()
	session()
	if S.dirty then U.refresh(); S.dirty = false end
	return { code = U.encodeCode(B():SaveDB("code")) }
end

M.search_export_xml = function()
	session()
	if S.dirty then U.refresh(); S.dirty = false end
	return { xml = B():SaveDB("code") }
end
