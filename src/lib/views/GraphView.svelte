<script lang="ts">
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import GraphCanvas, { type DrawEdge, type DrawNode } from "$lib/graph/GraphCanvas.svelte";
  import { exportCommand, gate, gateOpen, matchingChains, type GateCheck } from "$lib/graph/requirements";
  import { CHAIN_KINDS, CONSTRAINT_CHOICES, DAMAGE_TYPES, type CheckStatus, type GraphChain, type GraphGap, type GraphKit } from "$lib/graph/types";
  import { graph } from "$lib/state/graph.svelte";
  import { build } from "$lib/state/build.svelte";
  import { game } from "$lib/state/game.svelte";
  import { m } from "$lib/paraglide/messages";

  const req = $derived(graph.req);
  const doc = $derived(graph.doc);
  const checks = $derived(gate(doc, req, game.current));
  const open = $derived(gateOpen(checks));
  const chains = $derived(matchingChains(doc, req));

  // ---- requirements form
  const ascendancies = $derived(doc?.catalog?.ascendancies ?? []);
  const classes = $derived([...new Set(ascendancies.map((a) => a.class))].sort());
  const uniqueNames = $derived((doc?.mechanics?.nodes ?? []).filter((n) => n.kind === "unique").map((n) => n.name).sort());
  const keystoneNames = $derived((doc?.mechanics?.nodes ?? []).filter((n) => n.kind === "keystone").map((n) => n.name).sort());
  let dragOver = $state(false);
  let copied = $state<"" | "json" | "cmd">("");

  function toggle<T>(list: T[], v: T): T[] {
    return list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
  }

  async function readFile(f: File | undefined | null) {
    if (f) graph.load(await f.text(), f.name);
  }

  function useBuild() {
    const i = build.info;
    if (i) graph.setReq({ cls: i.className || null, ascendancy: i.ascendClassName || null });
  }

  async function copy(what: "json" | "cmd") {
    const text = what === "json" ? JSON.stringify(req, null, 2) : exportCommand();
    await writeText(text).catch(() => navigator.clipboard?.writeText(text).catch(() => {}));
    copied = what;
    setTimeout(() => (copied = ""), 1500);
  }

  const constraintLabels = $derived<Record<string, string>>({
    one_button: m.graph_constraint_one_button(),
    no_cast: m.graph_constraint_no_cast(),
    "no_cast:strict": m.graph_constraint_no_cast_strict(),
    no_minions: m.graph_constraint_no_minions(),
  });
  const kindLabels = $derived<Record<string, string>>({
    self_feed: m.graph_kind_self_feed(),
    enabler: m.graph_kind_enabler(),
    graft: m.graph_kind_graft(),
    bridge: m.graph_kind_bridge(),
  });

  function gateText(c: GateCheck): { label: string; detail: string } {
    const v = c.values ?? {};
    const ok = c.status === "ok";
    switch (c.id) {
      case "file":
        return { label: m.graph_gate_file(), detail: ok ? (graph.fileName ?? "") : m.graph_gate_file_missing() };
      case "format":
        return { label: m.graph_gate_format(), detail: ok ? `${v.format} v${v.version}` : m.graph_gate_format_bad({ format: String(v.format), version: String(v.version) }) };
      case "game":
        return { label: m.graph_gate_game(), detail: ok ? String(v.file) : m.graph_gate_game_bad({ file: String(v.file), app: String(v.app) }) };
      case "export":
        return { label: m.graph_gate_export(), detail: c.status === "fail" ? m.graph_gate_export_failed() : c.status === "warn" ? m.graph_gate_export_warn() : m.graph_gate_export_ok() };
      case "age":
        return { label: m.graph_gate_age(), detail: ok ? m.graph_gate_age_ok({ days: v.days }) : m.graph_gate_age_old({ days: v.days }) };
      case "ladder":
        return { label: m.graph_gate_ladder(), detail: ok ? m.graph_gate_ladder_ok() : m.graph_gate_ladder_missing() };
      case "scope_ascendancy":
        return { label: m.graph_gate_scope_ascendancy(), detail: ok ? m.graph_gate_scope_ok() : m.graph_gate_scope_bad({ file: String(v.file) }) };
      case "scope_damage":
        return { label: m.graph_gate_scope_damage(), detail: ok ? m.graph_gate_scope_ok() : m.graph_gate_scope_bad({ file: String(v.file) }) };
      case "scope_constraints":
        return { label: m.graph_gate_scope_constraints(), detail: ok ? m.graph_gate_scope_ok() : m.graph_gate_scope_bad({ file: String(v.file) }) };
      case "scope_uniques":
        return { label: m.graph_gate_scope_uniques(), detail: ok ? m.graph_gate_scope_ok() : m.graph_gate_scope_uniques_bad({ missing: String(v.missing) }) };
      case "matches":
        return { label: m.graph_gate_matches(), detail: ok ? m.graph_gate_matches_ok({ n: v.n }) : m.graph_gate_matches_none() };
    }
  }
  const mark: Record<CheckStatus, string> = { ok: "✓", warn: "!", fail: "✕" };
  const failing = $derived(checks.filter((c) => c.status === "fail").length);

  // ---- results
  let mode = $state<"mechanics" | "ladder">("mechanics");
  let ladderTab = $state<"kits" | "gaps">("kits");
  let query = $state("");
  let pickedChain = $state<string | null>(null);
  let pickedNode = $state<string | null>(null);
  let pickedKit = $state<number | null>(null);
  let pickedGap = $state<GraphGap | null>(null);
  let canvasRef = $state<ReturnType<typeof GraphCanvas>>();

  function clearPicks() {
    pickedChain = null;
    pickedNode = null;
    pickedKit = null;
    pickedGap = null;
  }
  $effect(() => {
    void mode;
    void graph.started;
    clearPicks();
    query = "";
  });

  const KIND_COLOUR: Record<string, string> = {
    unique: "--c-unique",
    keystone: "--c-rare",
    asc: "--c-class",
    skill: "--c-gem",
    util: "--c-magic",
    token: "--fg-2",
  };
  const kitColour = (k: number) => `oklch(72% 0.13 ${(k * 137.5) % 360})`;
  const legend = $derived([
    { colour: "var(--c-gem)", label: m.graph_legend_skill() },
    { colour: "var(--c-unique)", label: m.graph_legend_unique() },
    { colour: "var(--c-rare)", label: m.graph_legend_keystone() },
    { colour: "var(--c-class)", label: m.graph_legend_ascendancy() },
    { colour: "var(--c-magic)", label: m.graph_legend_utility() },
    { colour: "var(--fg-2)", label: m.graph_legend_token(), diamond: true },
  ]);

  const mech = $derived.by(() => {
    const ids = new Set<string>();
    const uses = new Map<string, number>();
    for (const c of chains) for (const id of [...c.path, ...c.path_tokens]) {
      ids.add(id);
      uses.set(id, (uses.get(id) ?? 0) + 1);
    }
    const nodes: DrawNode[] = (doc?.mechanics?.nodes ?? [])
      .filter((n) => ids.has(n.id))
      .map((n) => ({
        id: n.id,
        label: n.name,
        colour: KIND_COLOUR[n.kind] ?? "--fg-2",
        r: Math.min(13, 4.5 + Math.sqrt(uses.get(n.id) ?? 1) * 1.6),
        shape: n.kind === "token" ? "diamond" : "circle",
      }));
    const edges: DrawEdge[] = (doc?.mechanics?.edges ?? []).filter((e) => ids.has(e.s) && ids.has(e.t)).map((e) => ({ s: e.s, t: e.t, w: e.w, directed: true }));
    return { nodes, edges };
  });

  const lad = $derived.by(() => {
    const l = doc?.ladder;
    if (!l) return { nodes: [] as DrawNode[], edges: [] as DrawEdge[] };
    const nodes: DrawNode[] = l.nodes.map((n) => ({
      id: n.id,
      label: n.name,
      colour: kitColour(n.kit ?? 0),
      r: Math.min(12, 3 + Math.sqrt(n.n ?? 1) * 0.45),
      shape: "circle",
      cluster: n.kit,
    }));
    return { nodes, edges: l.edges.map((e) => ({ s: e.s, t: e.t, w: e.w })) };
  });

  const shown = $derived(mode === "mechanics" ? mech : lad);
  const chainById = $derived(new Map(chains.map((c) => [c.id, c])));
  const nodeInfo = $derived(new Map([...(doc?.mechanics?.nodes ?? []), ...(doc?.ladder?.nodes ?? [])].map((n) => [n.id, n])));

  const listedChains = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return chains.filter((c) => (!pickedNode || c.path.includes(pickedNode) || c.path_tokens.includes(pickedNode)) && (!q || `${c.main_name} ${c.thesis}`.toLowerCase().includes(q)));
  });

  const highlight = $derived.by((): Set<string> | null => {
    const q = query.trim().toLowerCase();
    if (mode === "mechanics") {
      const c = pickedChain ? chainById.get(pickedChain) : null;
      if (c) return new Set([...c.path, ...c.path_tokens]);
      if (pickedNode) {
        const s = new Set([pickedNode]);
        for (const e of mech.edges) {
          if (e.s === pickedNode) s.add(e.t);
          if (e.t === pickedNode) s.add(e.s);
        }
        return s;
      }
      if (q) return new Set(mech.nodes.filter((n) => n.label.toLowerCase().includes(q)).map((n) => n.id));
      return null;
    }
    if (pickedGap) {
      const want = new Set([`M:${pickedGap.skill}`, `${pickedGap.kind}:${pickedGap.candidate}`, ...pickedGap.driven_by]);
      return new Set(lad.nodes.filter((n) => want.has(n.id)).map((n) => n.id));
    }
    if (pickedKit != null) return new Set((doc?.ladder?.nodes ?? []).filter((n) => n.kit === pickedKit).map((n) => n.id));
    if (pickedNode) {
      const s = new Set([pickedNode]);
      for (const e of lad.edges) {
        if (e.s === pickedNode) s.add(e.t);
        if (e.t === pickedNode) s.add(e.s);
      }
      return s;
    }
    if (q) return new Set(lad.nodes.filter((n) => n.label.toLowerCase().includes(q)).map((n) => n.id));
    return null;
  });

  function pickNode(id: string | null) {
    pickedChain = null;
    pickedGap = null;
    pickedKit = null;
    pickedNode = id === pickedNode ? null : id;
  }
  function pickChain(c: GraphChain) {
    pickedNode = null;
    pickedChain = pickedChain === c.id ? null : c.id;
  }
  function pickKit(k: GraphKit) {
    pickedNode = null;
    pickedGap = null;
    pickedKit = pickedKit === k.id ? null : k.id;
  }
  function pickGap(g: GraphGap) {
    pickedNode = null;
    pickedKit = null;
    pickedGap = pickedGap === g ? null : g;
  }

  const nodeLines = $derived.by(() => {
    if (!pickedNode || mode !== "mechanics") return [];
    return (doc?.mechanics?.edges ?? []).filter((e) => e.s === pickedNode || e.t === pickedNode).map((e) => ({ type: e.type, other: nodeInfo.get(e.s === pickedNode ? e.t : e.s)?.name ?? "", line: e.line ?? "" }));
  });
  const pct = (x: number) => `${Math.round(x * 100)}%`;
</script>

{#if !graph.started}
  <div class="setup">
    <div class="col">
      <section class="panel block">
        <div class="label">{m.graph_step_data()}</div>
        <label
          class="drop"
          class:over={dragOver}
          ondragover={(e) => {
            e.preventDefault();
            dragOver = true;
          }}
          ondragleave={() => (dragOver = false)}
          ondrop={(e) => {
            e.preventDefault();
            dragOver = false;
            void readFile(e.dataTransfer?.files[0]);
          }}
        >
          <input type="file" accept=".json,application/json" onchange={(e) => readFile(e.currentTarget.files?.[0])} />
          {#if doc && graph.fileName}
            <span class="fname">{graph.fileName}</span>
            <span class="dim small">{m.graph_file_replace()}</span>
          {:else}
            <span>{m.graph_file_open()}</span>
            <span class="dim small">{m.graph_file_hint()}</span>
          {/if}
        </label>
        {#if graph.loadError}<div class="bad small selectable">{m.graph_file_error({ error: graph.loadError })}</div>{/if}
        {#if doc?.mechanics}
          <div class="dim small">
            {m.graph_file_summary({ chains: doc.mechanics.chains.length, nodes: doc.mechanics.nodes.length, kits: doc.ladder?.kits.length ?? 0 })}
          </div>
        {/if}
        <div class="row">
          <button class="btn sm" onclick={() => copy("cmd")}>{copied === "cmd" ? m.graph_copied() : m.graph_copy_command()}</button>
          {#if doc}<button class="btn sm ghost" onclick={() => graph.clear()}>{m.graph_file_close()}</button>{/if}
        </div>
      </section>

      <section class="panel block">
        <div class="label">{m.graph_step_request()}</div>

        <div class="field">
          <span class="name">{m.graph_req_constraints()}</span>
          <div class="chips">
            {#each CONSTRAINT_CHOICES as c (c)}
              <button class="btn sm" class:on={req.constraints.includes(c)} aria-pressed={req.constraints.includes(c)} onclick={() => graph.setReq({ constraints: toggle(req.constraints, c) })}>
                {constraintLabels[c]}
              </button>
            {/each}
          </div>
        </div>

        <div class="field">
          <span class="name">{m.graph_req_class()}</span>
          <div class="chips">
            <select
              class="select"
              value={req.cls ?? ""}
              onchange={(e) => {
                const cls = e.currentTarget.value || null;
                const keep = req.ascendancy && ascendancies.find((a) => a.name === req.ascendancy)?.class === cls;
                graph.setReq({ cls, ascendancy: keep ? req.ascendancy : null });
              }}
            >
              <option value="">{m.graph_req_any_class()}</option>
              {#if req.cls && !classes.includes(req.cls)}<option value={req.cls}>{req.cls}</option>{/if}
              {#each classes as c (c)}<option value={c}>{c}</option>{/each}
            </select>
            <select
              class="select"
              value={req.ascendancy ?? ""}
              onchange={(e) => {
                const name = e.currentTarget.value || null;
                graph.setReq({ ascendancy: name, cls: name ? (ascendancies.find((a) => a.name === name)?.class ?? req.cls) : req.cls });
              }}
            >
              <option value="">{m.graph_req_any_ascendancy()}</option>
              {#if req.ascendancy && !ascendancies.some((a) => a.name === req.ascendancy)}<option value={req.ascendancy}>{req.ascendancy}</option>{/if}
              {#each ascendancies.filter((a) => !req.cls || a.class === req.cls) as a (a.name)}<option value={a.name}>{a.name}</option>{/each}
            </select>
            {#if build.info}<button class="btn sm ghost" onclick={useBuild}>{m.graph_req_from_build()}</button>{/if}
          </div>
        </div>

        <div class="field">
          <span class="name">{m.graph_req_damage()}</span>
          <div class="chips">
            {#each DAMAGE_TYPES as d (d)}
              <button
                class="btn sm"
                class:on={req.damage_types.includes(d)}
                aria-pressed={req.damage_types.includes(d)}
                style:--dot="var(--c-{d.toLowerCase()})"
                onclick={() => graph.setReq({ damage_types: toggle(req.damage_types, d) })}
              >
                <span class="dot"></span>{d}
              </button>
            {/each}
          </div>
        </div>

        <div class="field">
          <span class="name">{m.graph_req_uniques()}</span>
          <div class="chips">
            {#each req.uniques as u (u.name)}
              <button class="btn sm on" title={m.graph_req_remove()} onclick={() => graph.setReq({ uniques: req.uniques.filter((x) => x.name !== u.name) })}>{u.name} ×</button>
            {/each}
            <select
              class="select"
              disabled={!uniqueNames.length}
              value=""
              onchange={(e) => {
                const name = e.currentTarget.value;
                e.currentTarget.value = "";
                if (name && !req.uniques.some((u) => u.name === name)) graph.setReq({ uniques: [...req.uniques, { name }], forbid_uniques: req.forbid_uniques.filter((x) => x !== name) });
              }}
            >
              <option value="">{m.graph_req_add_unique()}</option>
              {#each uniqueNames as u (u)}<option value={u}>{u}</option>{/each}
            </select>
          </div>
        </div>

        <div class="field">
          <span class="name">{m.graph_req_forbid()}</span>
          <div class="chips">
            {#each req.forbid_uniques as u (u)}
              <button class="btn sm on" title={m.graph_req_remove()} onclick={() => graph.setReq({ forbid_uniques: req.forbid_uniques.filter((x) => x !== u) })}>{u} ×</button>
            {/each}
            <select
              class="select"
              disabled={!uniqueNames.length}
              value=""
              onchange={(e) => {
                const name = e.currentTarget.value;
                e.currentTarget.value = "";
                if (name && !req.forbid_uniques.includes(name)) graph.setReq({ forbid_uniques: [...req.forbid_uniques, name], uniques: req.uniques.filter((x) => x.name !== name) });
              }}
            >
              <option value="">{m.graph_req_add_unique()}</option>
              {#each uniqueNames as u (u)}<option value={u}>{u}</option>{/each}
            </select>
          </div>
        </div>

        <div class="field">
          <span class="name">{m.graph_req_keystones()}</span>
          <div class="chips">
            {#each req.keystones as k (k)}
              <button class="btn sm on" title={m.graph_req_remove()} onclick={() => graph.setReq({ keystones: req.keystones.filter((x) => x !== k) })}>{k} ×</button>
            {/each}
            <select
              class="select"
              disabled={!keystoneNames.length}
              value=""
              onchange={(e) => {
                const name = e.currentTarget.value;
                e.currentTarget.value = "";
                if (name && !req.keystones.includes(name)) graph.setReq({ keystones: [...req.keystones, name] });
              }}
            >
              <option value="">{m.graph_req_add_keystone()}</option>
              {#each keystoneNames as k (k)}<option value={k}>{k}</option>{/each}
            </select>
          </div>
        </div>

        <div class="field">
          <span class="name">{m.graph_req_kinds()}</span>
          <div class="chips">
            {#each CHAIN_KINDS as k (k)}
              <button class="btn sm" class:on={req.chain_kinds.includes(k)} aria-pressed={req.chain_kinds.includes(k)} onclick={() => graph.setReq({ chain_kinds: toggle(req.chain_kinds, k) })}>{kindLabels[k]}</button>
            {/each}
            <span class="dim small">{m.graph_req_kinds_hint()}</span>
          </div>
        </div>

        <div class="field">
          <span class="name">{m.graph_req_novelty()}</span>
          <div class="chips">
            <input type="range" min="0" max="1" step="0.05" value={req.min_novelty} oninput={(e) => graph.setReq({ min_novelty: Number(e.currentTarget.value) })} />
            <span class="mono small">{pct(req.min_novelty)}</span>
            <span class="dim small">{m.graph_req_novelty_hint()}</span>
          </div>
        </div>

        <div class="field">
          <span class="name">{m.graph_req_budget()}</span>
          <div class="chips">
            {#each ["low", "mid", "high"] as const as t (t)}
              <button class="btn sm" class:on={req.budget_tier === t} aria-pressed={req.budget_tier === t} onclick={() => graph.setReq({ budget_tier: t })}>
                {t === "low" ? m.graph_budget_low() : t === "mid" ? m.graph_budget_mid() : m.graph_budget_high()}
              </button>
            {/each}
            <span class="dim small">{m.graph_req_builder_hint()}</span>
          </div>
        </div>

        <div class="row">
          <button class="btn sm" onclick={() => copy("json")}>{copied === "json" ? m.graph_copied() : m.graph_copy_requirements()}</button>
          <span class="dim small">{m.graph_copy_requirements_hint()}</span>
        </div>
      </section>
    </div>

    <div class="col narrow">
      <section class="panel block">
        <div class="label">{m.graph_step_check()}</div>
        <ul class="checks">
          {#each checks as c (c.id)}
            {@const t = gateText(c)}
            <li class={c.status}>
              <span class="mark">{mark[c.status]}</span>
              <div>
                <div>{t.label}</div>
                <div class="dim small selectable">{t.detail}</div>
                {#each c.notes ?? [] as n (n.text)}
                  <div class="note small selectable {n.status}">{n.text}</div>
                {/each}
              </div>
            </li>
          {/each}
        </ul>
        <div class="row">
          <button class="btn primary" disabled={!open} onclick={() => (graph.started = true)}>{m.graph_start()}</button>
          <span class="dim small">{open ? m.graph_start_ready({ n: chains.length }) : m.graph_start_blocked({ n: failing })}</span>
        </div>
      </section>
    </div>
  </div>
{:else}
  <div class="page">
    <div class="toolbar">
      <button class="btn sm" onclick={() => (graph.started = false)}>{m.graph_back()}</button>
      <span class="vr"></span>
      <button class="btn sm" class:on={mode === "mechanics"} aria-pressed={mode === "mechanics"} onclick={() => (mode = "mechanics")}>{m.graph_mode_mechanics()}</button>
      <button class="btn sm" class:on={mode === "ladder"} aria-pressed={mode === "ladder"} disabled={!doc?.ladder} title={doc?.ladder ? undefined : m.graph_gate_ladder_missing()} onclick={() => (mode = "ladder")}>
        {m.graph_mode_ladder()}
      </button>
      <input class="input search" placeholder={m.graph_search()} bind:value={query} />
      <span class="dim small">{m.graph_counts({ nodes: shown.nodes.length, edges: shown.edges.length })}</span>
      <span class="grow"></span>
      <button class="btn sm ghost" onclick={() => canvasRef?.reset()}>{m.graph_fit()}</button>
    </div>
    <div class="main">
      <div class="stage">
        {#key mode}
          <GraphCanvas bind:this={canvasRef} nodes={shown.nodes} edges={shown.edges} {highlight} selected={pickedNode} onpick={pickNode} />
        {/key}
        {#if mode === "mechanics"}
          <div class="legend">
            {#each legend as l (l.label)}
              <span><i class:diamond={l.diamond} style:background={l.colour}></i>{l.label}</span>
            {/each}
          </div>
        {/if}
      </div>
      <aside class="side">
        {#if pickedNode}
          {@const n = nodeInfo.get(pickedNode)}
          <div class="detail">
            <div class="head">
              <strong>{n?.name}</strong>
              <button class="btn sm ghost" onclick={() => (pickedNode = null)}>×</button>
            </div>
            <div class="dim small">{[n?.kind, n?.ascendancy, n?.slot, n?.n != null ? m.graph_node_builds({ n: n.n }) : ""].filter(Boolean).join(" · ")}</div>
            {#each nodeLines as l (l.type + l.other + l.line)}
              <div class="line small selectable">
                <span class="tag">{l.type === "produces" ? m.graph_edge_produces() : m.graph_edge_consumes()}</span>
                {l.other}
                <div class="dim">{l.line}</div>
              </div>
            {/each}
          </div>
        {/if}
        {#if mode === "mechanics"}
          <div class="label pad">{m.graph_chains({ n: listedChains.length })}</div>
          <div class="list">
            {#each listedChains as c (c.id)}
              <button class="item" class:on={pickedChain === c.id} onclick={() => pickChain(c)}>
                <div class="top">
                  <strong>{c.main_name}</strong>
                  <span class="tag">{kindLabels[c.kind] ?? c.kind}</span>
                  <span class="grow"></span>
                  <span class="mono small" title={m.graph_chain_score_title()}>{c.score.toFixed(2)}</span>
                </div>
                <div class="small thesis">{c.thesis}</div>
                {#if pickedChain === c.id}
                  <div class="small dim">{m.graph_chain_numbers({ fit: pct(c.fit), novelty: pct(c.novelty) })}</div>
                  {#each c.evidence as ev (ev)}<div class="small dim selectable ev">{ev}</div>{/each}
                {/if}
              </button>
            {:else}
              <div class="dim small pad">{m.graph_none()}</div>
            {/each}
          </div>
        {:else}
          <div class="tabs pad">
            <button class="btn sm" class:on={ladderTab === "kits"} onclick={() => (ladderTab = "kits")}>{m.graph_kits({ n: doc?.ladder?.kits.length ?? 0 })}</button>
            <button class="btn sm" class:on={ladderTab === "gaps"} onclick={() => (ladderTab = "gaps")}>{m.graph_gaps({ n: doc?.ladder?.gaps.length ?? 0 })}</button>
          </div>
          <div class="list">
            {#if ladderTab === "kits"}
              {#each doc?.ladder?.kits ?? [] as k (k.id)}
                <button class="item" class:on={pickedKit === k.id} onclick={() => pickKit(k)}>
                  <div class="top">
                    <i class="swatch" style:background={kitColour(k.id)}></i>
                    <strong>{k.label}</strong>
                    <span class="grow"></span>
                    <span class="mono small">{m.graph_node_builds({ n: k.builds })}</span>
                  </div>
                  <div class="small dim">{k.skills.map((s) => `${s.name} (${s.n})`).join(", ")}</div>
                  {#if pickedKit === k.id}
                    {#if k.mean_resid != null}<div class="small">{m.graph_kit_resid({ value: (k.mean_resid >= 0 ? "+" : "") + k.mean_resid.toFixed(2) })}</div>{/if}
                    <div class="small dim selectable">{k.core.map((c) => c.split(":").slice(1).join(":")).join(", ")}</div>
                  {/if}
                </button>
              {/each}
            {:else}
              <div class="dim small pad">{m.graph_gaps_hint()}</div>
              {#each doc?.ladder?.gaps ?? [] as g (g.skill + g.kind + g.candidate)}
                <button class="item" class:on={pickedGap === g} onclick={() => pickGap(g)}>
                  <div class="top">
                    <strong>{g.skill}</strong>
                    <span>+ {g.candidate}</span>
                    <span class="grow"></span>
                    <span class="mono small">{g.score.toFixed(0)}</span>
                  </div>
                  <div class="small dim">{m.graph_gap_driven({ by: g.driven_by.map((c) => c.split(":").slice(1).join(":")).join(", ") })}</div>
                </button>
              {/each}
            {/if}
          </div>
        {/if}
      </aside>
    </div>
  </div>
{/if}

<style>
  .setup {
    flex: 1;
    display: flex;
    gap: 12px;
    padding: 12px;
    overflow: auto;
    align-items: flex-start;
    flex-wrap: wrap;
  }
  .col {
    flex: 2 1 420px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
  }
  .col.narrow {
    flex: 1 1 320px;
    position: sticky;
    top: 0;
  }
  .block {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .small {
    font-size: var(--fs-xs);
  }
  .bad {
    color: var(--bad);
  }
  .drop {
    display: flex;
    flex-direction: column;
    gap: 2px;
    align-items: center;
    padding: 14px;
    border: 1px dashed var(--line-2);
    border-radius: var(--r-2);
    cursor: var(--cursor-pointer, pointer);
    text-align: center;
  }
  .drop:hover,
  .drop.over {
    background: var(--bg-hover);
    border-color: var(--focus);
  }
  .drop input {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
  }
  .drop:focus-within {
    border-color: var(--focus);
  }
  .fname {
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }
  .row,
  .chips {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }
  .field {
    display: grid;
    grid-template-columns: 130px 1fr;
    gap: 8px;
    align-items: start;
  }
  .field .name {
    color: var(--fg-1);
    font-size: var(--fs-sm);
    padding-top: 3px;
  }
  .select {
    height: 22px;
    font-size: var(--fs-xs);
    max-width: 200px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--dot);
  }
  .checks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .checks li {
    display: grid;
    grid-template-columns: 18px 1fr;
    gap: 8px;
    font-size: var(--fs-sm);
  }
  .mark {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-size: var(--fs-2xs);
    font-weight: 700;
    border: 1px solid currentColor;
  }
  .ok .mark {
    color: var(--ok);
  }
  .warn .mark,
  .note.warn {
    color: var(--warn);
  }
  .fail .mark,
  .note.fail {
    color: var(--bad);
  }
  .page {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--line-0);
    background: var(--bg-1);
  }
  .vr {
    width: 1px;
    height: 16px;
    background: var(--line-1);
  }
  .grow {
    flex: 1;
  }
  .search {
    height: 22px;
    width: 180px;
    font-size: var(--fs-xs);
  }
  .main {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .stage {
    position: relative;
    flex: 1;
    display: flex;
    min-width: 0;
  }
  .legend {
    position: absolute;
    left: 10px;
    bottom: 10px;
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    padding: 5px 8px;
    border: 1px solid var(--line-0);
    border-radius: var(--r-1);
    background: color-mix(in oklab, var(--bg-1) 88%, transparent);
    font-size: var(--fs-xs);
    color: var(--fg-1);
    pointer-events: none;
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .legend i,
  .swatch {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
  }
  .legend i.diamond {
    border-radius: 1px;
    transform: rotate(45deg) scale(0.85);
  }
  .side {
    width: 340px;
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-left: 1px solid var(--line-0);
    background: var(--bg-1);
  }
  .pad {
    padding: 8px 10px 4px;
  }
  .tabs {
    display: flex;
    gap: 6px;
  }
  .list {
    flex: 1;
    overflow: auto;
    padding: 4px 6px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .item {
    appearance: none;
    text-align: left;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--r-1);
    padding: 6px 8px;
    color: var(--fg-0);
    font-size: var(--fs-sm);
    display: flex;
    flex-direction: column;
    gap: 3px;
    cursor: var(--cursor-pointer, pointer);
  }
  .item:hover {
    background: var(--bg-hover);
  }
  .item.on {
    background: var(--bg-active);
    border-color: color-mix(in oklab, var(--accent) 45%, var(--line-2));
  }
  .top {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .tag {
    font-size: var(--fs-2xs);
    padding: 0 5px;
    border: 1px solid var(--line-2);
    border-radius: 999px;
    color: var(--fg-2);
    white-space: nowrap;
  }
  .thesis {
    color: var(--fg-1);
  }
  .ev {
    padding-left: 8px;
    border-left: 2px solid var(--line-2);
  }
  .detail {
    padding: 10px;
    border-bottom: 1px solid var(--line-0);
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 45%;
    overflow: auto;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .line .tag {
    margin-right: 4px;
  }
</style>
