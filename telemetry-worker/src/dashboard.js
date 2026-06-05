// Self-contained dashboard page (HTML + CSS + JS, no external dependencies so it
// works under Cloudflare with no CDN/CSP issues). Charts are drawn as inline SVG.
//
// The page fetches /v1/stats with the dashboard token (entered once, stored in
// localStorage) and renders tiered metrics: a hero "total users" number, the
// active-user funnel, then secondary KPIs and diagnostic breakdowns. Every
// metric the API returns is shown; importance is conveyed visually (hero /
// primary cards / muted diagnostic tables) and via short "why it matters" notes.

export const DASHBOARD_HTML = `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>jcode telemetry</title>
<style>
  :root {
    --bg: #0b0e14;
    --bg-soft: #11151f;
    --panel: #151a26;
    --panel-2: #1b2231;
    --line: #232c3d;
    --text: #e6edf6;
    --muted: #8a97ac;
    --muted-2: #5d6982;
    --accent: #5b9dff;
    --accent-2: #7c5cff;
    --good: #3fb950;
    --warn: #d29922;
    --bad: #f85149;
    --radius: 14px;
    --shadow: 0 1px 0 rgba(255,255,255,0.03) inset, 0 10px 30px rgba(0,0,0,0.35);
  }
  * { box-sizing: border-box; }
  html, body { margin: 0; padding: 0; }
  body {
    background: radial-gradient(1200px 600px at 80% -10%, #16203a 0%, var(--bg) 55%) fixed;
    color: var(--text);
    font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, Roboto, Helvetica, Arial, sans-serif;
    -webkit-font-smoothing: antialiased;
    min-height: 100vh;
  }
  a { color: var(--accent); text-decoration: none; }
  .wrap { max-width: 1180px; margin: 0 auto; padding: 28px 22px 80px; }

  header.top { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 22px; flex-wrap: wrap; }
  .brand { display: flex; align-items: center; gap: 12px; }
  .logo { width: 34px; height: 34px; border-radius: 9px; background: linear-gradient(135deg, var(--accent), var(--accent-2)); display: grid; place-items: center; font-weight: 800; color: #fff; box-shadow: var(--shadow); }
  .brand h1 { font-size: 17px; margin: 0; font-weight: 650; letter-spacing: .2px; }
  .brand .sub { color: var(--muted); font-size: 12px; }
  .top-actions { display: flex; align-items: center; gap: 10px; }
  .pill { font-size: 12px; color: var(--muted); background: var(--panel); border: 1px solid var(--line); padding: 6px 11px; border-radius: 999px; }
  button.btn { cursor: pointer; font: inherit; color: var(--text); background: var(--panel-2); border: 1px solid var(--line); padding: 7px 13px; border-radius: 9px; }
  button.btn:hover { border-color: var(--accent); }

  /* Hero */
  .hero { display: grid; grid-template-columns: 1.15fr 1fr; gap: 18px; margin-bottom: 18px; }
  @media (max-width: 860px) { .hero { grid-template-columns: 1fr; } }
  .card { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius); box-shadow: var(--shadow); }
  .hero-main { padding: 26px 28px; position: relative; overflow: hidden; }
  .hero-main:before { content:""; position:absolute; right:-40px; top:-60px; width:240px; height:240px; background: radial-gradient(circle, rgba(91,157,255,.20), transparent 60%); }
  .eyebrow { text-transform: uppercase; letter-spacing: 1.4px; font-size: 11px; color: var(--muted); font-weight: 600; }
  .hero-number { font-size: 68px; font-weight: 750; line-height: 1.02; margin: 6px 0 2px; letter-spacing: -1.5px; background: linear-gradient(180deg, #fff, #b9c6dd); -webkit-background-clip: text; background-clip: text; color: transparent; }
  .hero-note { color: var(--muted); font-size: 13px; max-width: 46ch; }
  .hero-sub { display: flex; gap: 26px; margin-top: 18px; flex-wrap: wrap; }
  .hero-sub .k { font-size: 11px; color: var(--muted); text-transform: uppercase; letter-spacing: .6px; }
  .hero-sub .v { font-size: 22px; font-weight: 650; }

  .hero-side { padding: 18px 20px; display: grid; grid-template-rows: auto 1fr; }
  .hero-side h3 { margin: 2px 0 10px; font-size: 13px; color: var(--muted); font-weight: 600; }

  /* Section + grid */
  .section-title { display: flex; align-items: baseline; gap: 10px; margin: 26px 2px 12px; }
  .section-title h2 { font-size: 14px; margin: 0; font-weight: 650; letter-spacing: .3px; }
  .section-title .hint { color: var(--muted-2); font-size: 12px; }
  .grid { display: grid; gap: 14px; }
  .g4 { grid-template-columns: repeat(4, 1fr); }
  .g3 { grid-template-columns: repeat(3, 1fr); }
  .g2 { grid-template-columns: repeat(2, 1fr); }
  .hero-kpis { grid-template-columns: repeat(3, 1fr); }
  .hero-kpis .kpi { padding: 12px 12px; }
  .hero-kpis .num { font-size: 23px; }
  .hero-kpis .meta { font-size: 11px; }
  @media (max-width: 980px) { .g4 { grid-template-columns: repeat(2, 1fr); } .g3 { grid-template-columns: repeat(2, 1fr); } }
  @media (max-width: 620px) { .g4, .g3, .g2 { grid-template-columns: 1fr; } }

  .kpi { padding: 16px 17px; }
  .kpi .label { color: var(--muted); font-size: 12px; display: flex; align-items: center; gap: 7px; }
  .kpi .num { font-size: 28px; font-weight: 700; margin-top: 7px; letter-spacing: -.5px; }
  .kpi .meta { color: var(--muted-2); font-size: 12px; margin-top: 3px; }
  .dot { width: 7px; height: 7px; border-radius: 50%; display: inline-block; }
  .dot.p1 { background: var(--accent); } .dot.p2 { background: var(--muted); } .dot.p3 { background: var(--muted-2); }
  .tag { font-size: 10px; text-transform: uppercase; letter-spacing: .5px; padding: 2px 7px; border-radius: 6px; border: 1px solid var(--line); color: var(--muted); }
  .tag.key { color: var(--accent); border-color: rgba(91,157,255,.35); background: rgba(91,157,255,.08); }

  .panel { padding: 18px 18px 8px; }
  .panel h3 { margin: 0 0 4px; font-size: 13px; font-weight: 650; }
  .panel .desc { color: var(--muted-2); font-size: 12px; margin: 0 0 12px; }

  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th, td { text-align: left; padding: 7px 6px; border-bottom: 1px solid var(--line); }
  th { color: var(--muted); font-weight: 600; font-size: 11px; text-transform: uppercase; letter-spacing: .5px; }
  td.num, th.num { text-align: right; font-variant-numeric: tabular-nums; }
  .bar { height: 7px; border-radius: 4px; background: linear-gradient(90deg, var(--accent), var(--accent-2)); }
  .bar-track { background: var(--panel-2); border-radius: 4px; overflow: hidden; }

  .muted { color: var(--muted); } .small { font-size: 12px; }
  .legend { display: flex; gap: 14px; align-items: center; font-size: 12px; color: var(--muted); margin-bottom: 8px; flex-wrap: wrap; }
  .legend i { width: 10px; height: 10px; border-radius: 3px; display: inline-block; margin-right: 5px; vertical-align: -1px; }

  .feedback-item { padding: 11px 0; border-bottom: 1px solid var(--line); }
  .feedback-item .q { color: var(--text); }
  .feedback-item .m { color: var(--muted-2); font-size: 11px; margin-top: 3px; }

  /* token gate */
  .gate { max-width: 420px; margin: 12vh auto 0; text-align: center; }
  .gate .card { padding: 28px 26px; }
  .gate input { width: 100%; margin: 14px 0; padding: 11px 13px; border-radius: 10px; border: 1px solid var(--line); background: var(--bg-soft); color: var(--text); font: inherit; }
  .gate button { width: 100%; padding: 11px; font-weight: 600; }
  .err { color: var(--bad); font-size: 13px; min-height: 18px; }
  .hidden { display: none !important; }
  .foot { color: var(--muted-2); font-size: 12px; margin-top: 30px; text-align: center; }
  .spin { display:inline-block; width:14px; height:14px; border:2px solid var(--line); border-top-color: var(--accent); border-radius:50%; animation: sp 0.8s linear infinite; vertical-align:-2px; }
  @keyframes sp { to { transform: rotate(360deg); } }
</style>
</head>
<body>
<div class="wrap">
  <!-- token gate -->
  <div id="gate" class="gate hidden">
    <div class="card">
      <div class="logo" style="margin:0 auto 14px">jc</div>
      <h1 style="margin:0 0 4px;font-size:18px">jcode telemetry</h1>
      <div class="muted small">Enter the dashboard token to view stats.</div>
      <input id="token" type="password" placeholder="dashboard token" autocomplete="off" />
      <button class="btn" id="unlock">Unlock</button>
      <div class="err" id="gate-err"></div>
    </div>
  </div>

  <!-- dashboard -->
  <div id="app" class="hidden">
    <header class="top">
      <div class="brand">
        <div class="logo">jc</div>
        <div>
          <h1>jcode telemetry</h1>
          <div class="sub" id="generated">live product analytics</div>
        </div>
      </div>
      <div class="top-actions">
        <span class="pill" id="freshness">—</span>
        <button class="btn" id="refresh">Refresh</button>
        <button class="btn" id="logout">Lock</button>
      </div>
    </header>

    <div id="content"></div>

    <div class="foot">
      Users are distinct anonymous <code>telemetry_id</code>s. Headline numbers exclude CI runners and dev/non-release builds.
      Raw and CI-inclusive figures are shown in the diagnostic tiers so nothing is hidden.
    </div>
  </div>
</div>

<script>
const fmt = (n) => (n == null ? "—" : Number(n).toLocaleString());
const pct = (x) => (x == null ? "—" : (x * 100).toFixed(1) + "%");
const ms = (x) => (x == null ? "—" : x >= 1000 ? (x/1000).toFixed(1) + "s" : Math.round(x) + "ms");
const dec = (x, d=2) => (x == null ? "—" : Number(x).toFixed(d));
const el = (h) => { const t = document.createElement("template"); t.innerHTML = h.trim(); return t.content.firstChild; };
const esc = (s) => String(s == null ? "" : s).replace(/[&<>"]/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"}[c]));

let TOKEN = localStorage.getItem("jcode_dash_token") || "";

function showGate(msg) {
  document.getElementById("app").classList.add("hidden");
  document.getElementById("gate").classList.remove("hidden");
  document.getElementById("gate-err").textContent = msg || "";
}
function showApp() {
  document.getElementById("gate").classList.add("hidden");
  document.getElementById("app").classList.remove("hidden");
}

async function load() {
  if (!TOKEN) { showGate(""); return; }
  document.getElementById("content").innerHTML = '<div class="muted" style="padding:40px 0"><span class="spin"></span> loading…</div>';
  showApp();
  let res;
  try {
    res = await fetch("/v1/stats?token=" + encodeURIComponent(TOKEN), { headers: { "Authorization": "Bearer " + TOKEN } });
  } catch (e) { showGate("Network error."); return; }
  if (res.status === 401) { localStorage.removeItem("jcode_dash_token"); TOKEN = ""; showGate("Invalid token."); return; }
  if (!res.ok) { document.getElementById("content").innerHTML = '<div class="err">Failed to load stats ('+res.status+').</div>'; return; }
  const data = await res.json();
  render(data);
}

function kpi(label, value, meta, tier, isKey) {
  return \`<div class="card kpi">
    <div class="label"><span class="dot p\${tier||1}"></span>\${esc(label)} \${isKey?'<span class="tag key">key</span>':''}</div>
    <div class="num">\${value}</div>
    <div class="meta">\${meta||""}</div>
  </div>\`;
}

function barTable(title, desc, rows, keyName, valName, total) {
  const max = Math.max(1, ...rows.map(r => r.value));
  const body = rows.map(r => \`<tr>
      <td>\${esc(r.label)}</td>
      <td style="width:45%"><div class="bar-track"><div class="bar" style="width:\${Math.max(2,(r.value/max)*100)}%"></div></div></td>
      <td class="num">\${fmt(r.value)}</td>
    </tr>\`).join("");
  return \`<div class="card panel">
    <h3>\${esc(title)}</h3><p class="desc">\${esc(desc)}</p>
    <table><thead><tr><th>\${esc(keyName)}</th><th>share</th><th class="num">\${esc(valName)}</th></tr></thead><tbody>\${body || '<tr><td class="muted" colspan="3">no data</td></tr>'}</tbody></table>
  </div>\`;
}

function lineChart(series, opts) {
  // series: [{name,color,points:[{date,value}]}]
  const W = 760, H = 220, padL = 36, padR = 12, padT = 14, padB = 26;
  const dates = series[0] ? series[0].points.map(p => p.date) : [];
  if (!dates.length) return '<div class="muted small" style="padding:18px">no timeseries yet</div>';
  const maxV = Math.max(1, ...series.flatMap(s => s.points.map(p => p.value)));
  const x = (i) => padL + (i/(Math.max(1,dates.length-1)))*(W-padL-padR);
  const y = (v) => padT + (1 - v/maxV)*(H-padT-padB);
  const grid = [0,0.25,0.5,0.75,1].map(f => {
    const gy = padT + f*(H-padT-padB); const val = Math.round(maxV*(1-f));
    return \`<line x1="\${padL}" y1="\${gy}" x2="\${W-padR}" y2="\${gy}" stroke="#232c3d" stroke-width="1"/><text x="4" y="\${gy+3}" fill="#5d6982" font-size="10">\${val}</text>\`;
  }).join("");
  const paths = series.map(s => {
    const d = s.points.map((p,i) => (i?'L':'M')+x(i).toFixed(1)+' '+y(p.value).toFixed(1)).join(' ');
    return \`<path d="\${d}" fill="none" stroke="\${s.color}" stroke-width="2" stroke-linejoin="round"/>\`;
  }).join("");
  const lbl = (i) => \`<text x="\${x(i)}" y="\${H-8}" fill="#5d6982" font-size="10" text-anchor="middle">\${dates[i].slice(5)}</text>\`;
  const ticks = dates.length>1 ? [0, Math.floor(dates.length/2), dates.length-1].map(lbl).join("") : "";
  const legend = series.map(s => \`<span><i style="background:\${s.color}"></i>\${esc(s.name)}</span>\`).join("");
  return \`<div class="legend">\${legend}</div><svg viewBox="0 0 \${W} \${H}" width="100%" preserveAspectRatio="xMidYMid meet">\${grid}\${paths}\${ticks}</svg>\`;
}

function render(d) {
  document.getElementById("generated").textContent = "updated " + new Date(d.generated_at).toLocaleString();
  document.getElementById("freshness").textContent = "as of " + new Date(d.generated_at).toLocaleTimeString();
  const c = document.getElementById("content");
  const u = d.users, a = d.active, lc = d.lifecycle, q = d.quality, ret = d.retention;

  // hero + active funnel timeseries
  const ts = (d.timeseries.daily || []);
  const headlineSeries = [
    { name: "headline DAU", color: "#5b9dff", points: ts.map(r => ({date:r.date, value:r.headline})) },
    { name: "meaningful", color: "#7c5cff", points: ts.map(r => ({date:r.date, value:r.meaningful})) },
    { name: "raw", color: "#39507a", points: ts.map(r => ({date:r.date, value:r.raw})) },
  ];

  let html = "";

  // ---- HERO ----
  html += \`<div class="hero">
    <div class="card hero-main">
      <div class="eyebrow">Total users</div>
      <div class="hero-number">\${fmt(u.total_users)}</div>
      <div class="hero-note">Distinct real people who installed or did meaningful work in jcode. Excludes CI runners and counts each anonymous machine id once. This is the headline number.</div>
      <div class="hero-sub">
        <div><div class="k">Core (did work)</div><div class="v">\${fmt(u.core_users)}</div></div>
        <div><div class="k">Installed</div><div class="v">\${fmt(u.installed_users)}</div></div>
        <div><div class="k">Reached (ran it)</div><div class="v">\${fmt(u.reached_users)}</div></div>
      </div>
    </div>
    <div class="card hero-side">
      <h3>Active users (distinct, headline definition)</h3>
      <div class="grid hero-kpis" style="gap:10px">
        \${kpi("DAU", fmt(a.dau), "today, meaningful + release", 1, true)}
        \${kpi("WAU", fmt(a.wau), "last 7 days", 1, true)}
        \${kpi("MAU", fmt(a.mau), "last 30 days", 1, true)}
      </div>
      <div style="margin-top:12px">\${lineChart(headlineSeries, {})}</div>
    </div>
  </div>\`;

  // ---- Why these differ (transparency band) ----
  html += \`<div class="section-title"><h2>How the user number is built</h2><span class="hint">each tier is broader than the one below it; nothing is dropped</span></div>\`;
  html += \`<div class="grid g4">
    \${kpi("Reached", fmt(u.reached_users), "ran jcode at least once (non-CI)", 2)}
    \${kpi("Total users", fmt(u.total_users), "installed OR did meaningful work", 1, true)}
    \${kpi("Core users", fmt(u.core_users), "did meaningful work", 2)}
    \${kpi("CI ids (excluded)", fmt(u.ci_ids), "ephemeral runners, filtered out", 3)}
  </div>
  <div class="grid g2" style="margin-top:14px">
    \${kpi("All ids incl. CI + dev", fmt(u.all_ids_including_ci), "raw upper bound, never used as headline", 3)}
    \${kpi("Installed users", fmt(u.installed_users), "distinct non-CI install events", 2)}
  </div>\`;

  // ---- Acquisition & retention ----
  html += \`<div class="section-title"><h2>Acquisition &amp; retention</h2><span class="hint">important: are new users sticking?</span></div>\`;
  html += \`<div class="grid g4">
    \${kpi("Install events", fmt(lc.install_events), fmt(lc.install_ids_noci)+" distinct (non-CI)", 2)}
    \${kpi("Upgrades", fmt(lc.upgrade_events), "version bumps observed", 3)}
    \${kpi("D7 retention", pct(ret.d7_retention), (ret.d7_retained||0)+" of "+(ret.d7_cohort||0)+" returned", 1, true)}
    \${kpi("Multi-session rate", pct(q.multi_session_rate), "users running >1 session at once", 3)}
  </div>\`;
  html += \`<div class="grid g2" style="margin-top:14px">
    <div class="card panel"><h3>Daily active users (60d)</h3><p class="desc">headline = meaningful work on a release build, excluding CI. raw = anyone who launched.</p>\${lineChart(headlineSeries, {})}</div>
    <div class="card panel"><h3>New installs / day (60d, non-CI)</h3><p class="desc">distinct ids whose first install event landed that day.</p>\${lineChart([{name:"installs",color:"#3fb950",points:(d.timeseries.installs||[]).map(r=>({date:r.date,value:r.installs}))}], {})}</div>
  </div>\`;

  // ---- Engagement quality ----
  html += \`<div class="section-title"><h2>Engagement quality</h2><span class="hint">30-day, non-CI sessions</span></div>\`;
  html += \`<div class="grid g4">
    \${kpi("Avg session length", dec(q.avg_session_mins,1)+" min", "per meaningful session", 2)}
    \${kpi("Avg turns / session", dec(q.avg_turns,1), "user prompts per session", 2)}
    \${kpi("Session success rate", pct(q.success_rate), "ended in a successful state", 1, true)}
    \${kpi("Abandon rate", pct(q.abandon_rate), "left before first response", 2)}
  </div>
  <div class="grid g4" style="margin-top:14px">
    \${kpi("Turn success rate", pct(d.turns.turn_success_rate), "per-turn, 30d", 2)}
    \${kpi("Avg turn time", ms(d.turns.avg_turn_ms), "active duration per turn", 3)}
    \${kpi("Time to first response", ms(q.avg_first_response_ms), "agent responsiveness", 2)}
    \${kpi("Avg tool latency", ms(q.avg_tool_latency_ms), "per executed tool call", 3)}
  </div>
  <div class="grid g2" style="margin-top:14px">
    \${kpi("Tokens (30d)", fmt(q.tokens_30d), "input + output across sessions", 3)}
    \${kpi("Crash rate", pct(lc.crash_rate)+"  ·  completion "+(lc.lifecycle_completion_ratio==null?"—":lc.lifecycle_completion_ratio), "session_crash share / (ends+crashes)/starts", 1, true)}
  </div>\`;

  // ---- Reliability / errors ----
  const e = d.errors;
  html += \`<div class="section-title"><h2>Reliability</h2><span class="hint">error counts, 30d non-CI — watch for spikes</span></div>\`;
  html += \`<div class="grid g4">
    \${kpi("Provider timeouts", fmt(e.provider_timeout), "", (e.provider_timeout>0?1:3))}
    \${kpi("Rate limited", fmt(e.rate_limited), "", (e.rate_limited>0?2:3))}
    \${kpi("Auth failures", fmt(e.auth_failed), "", (e.auth_failed>0?1:3))}
    \${kpi("Tool / MCP errors", fmt((e.tool_error||0)+(e.mcp_error||0)), fmt(e.tool_error)+" tool · "+fmt(e.mcp_error)+" mcp", 3)}
  </div>\`;

  // ---- Breakdowns ----
  const b = d.breakdowns;
  const rows = (arr, k) => (arr||[]).map(r => ({ label: r[k] ?? "unknown", value: r.users }));
  html += \`<div class="section-title"><h2>Who &amp; what</h2><span class="hint">distinct users per bucket</span></div>\`;
  html += \`<div class="grid g2">
    \${barTable("Versions", "adoption by release (non-CI users)", rows(b.versions,"version"), "version", "users")}
    \${barTable("Operating system", "OS split", rows(b.os,"os"), "os", "users")}
  </div>
  <div class="grid g2" style="margin-top:14px">
    \${barTable("Providers", "meaningful sessions by provider", rows(b.providers,"provider"), "provider", "users")}
    \${barTable("Auth method", "successful auth by provider", rows(b.auth,"auth_provider"), "provider", "users")}
  </div>
  <div class="grid g2" style="margin-top:14px">
    \${barTable("Build channel", "incl. dev/local; release is the headline channel", rows(b.channels,"build_channel"), "channel", "users")}
    \${barTable("Onboarding funnel", "distinct users reaching each step", rows(b.onboarding,"step"), "step", "users")}
  </div>\`;

  // ---- Feature adoption ----
  const f = d.features;
  const featRows = Object.entries(f||{}).map(([k,v]) => ({label:k.replace(/_/g,' '), value:v})).sort((a,b)=>b.value-a.value);
  html += \`<div class="section-title"><h2>Feature adoption</h2><span class="hint">distinct users using each feature, 30d</span></div>\`;
  html += \`<div class="grid g2">
    \${barTable("Features", "how many users touched each capability", featRows, "feature", "users")}
    \${transportPanel(d.transport)}
  </div>\`;

  // ---- Feedback ----
  if ((d.feedback||[]).length) {
    html += \`<div class="section-title"><h2>Recent feedback</h2><span class="hint">explicit user submissions</span></div>\`;
    html += \`<div class="card panel">\` + d.feedback.map(fb => \`
      <div class="feedback-item">
        <div class="q">\${esc(fb.feedback_text)}</div>
        <div class="m">\${esc(new Date(fb.created_at+'Z').toLocaleString())} · v\${esc(fb.version||'?')}\${fb.feedback_rating?' · '+esc(fb.feedback_rating):''}\${fb.feedback_reason?' · '+esc(fb.feedback_reason):''}</div>
      </div>\`).join("") + \`</div>\`;
  }

  c.innerHTML = html;
}

function transportPanel(t) {
  const rows = [
    ["https", t.https], ["ws fresh", t.ws_fresh], ["ws reuse", t.ws_reuse],
    ["cli subprocess", t.cli], ["native http2", t.native_http2], ["other", t.other],
  ].map(([label,value]) => ({label, value: value||0})).sort((a,b)=>b.value-a.value);
  return barTable("Transport mix", "request transport counts (30d non-CI)", rows, "transport", "count");
}

// events
document.getElementById("unlock").addEventListener("click", () => {
  const v = document.getElementById("token").value.trim();
  if (!v) { document.getElementById("gate-err").textContent = "Enter a token."; return; }
  TOKEN = v; localStorage.setItem("jcode_dash_token", v); load();
});
document.getElementById("token") && document.getElementById("token").addEventListener("keydown", (e)=>{ if(e.key==="Enter") document.getElementById("unlock").click(); });
document.getElementById("refresh").addEventListener("click", load);
document.getElementById("logout").addEventListener("click", () => { localStorage.removeItem("jcode_dash_token"); TOKEN=""; showGate(""); });

load();
</script>
</body>
</html>`;
