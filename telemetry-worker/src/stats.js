// Read-only telemetry aggregation for the dashboard.
//
// Everything here returns counts/aggregates only, never raw event rows. Metrics
// are organized into tiers (headline / secondary / diagnostic) and tagged with
// importance so the dashboard can present "the one number" prominently while
// still surfacing all available information.
//
// Accuracy rules (mirrors README "Accuracy notes"):
//   - Users are distinct telemetry_id, never event counts.
//   - "meaningful" = real work; see MEANINGFUL_SQL.
//   - Headline numbers exclude CI traffic (is_ci = 1) and non-release channels.
//   - Raw / less-filtered tiers are always reported alongside, never removed.

// Meaningful-activity predicate, shared by every query so all windows agree.
// A row is meaningful if it is a session_end/session_crash that did real work,
// OR a turn_end (which only fires after a completed user turn) that did work.
const MEANINGFUL_SQL = `(
  (event IN ('session_end','session_crash') AND (
    turns > 0 OR had_user_prompt > 0 OR had_assistant_response > 0
    OR assistant_responses > 0 OR tool_calls > 0 OR executed_tool_calls > 0
    OR duration_secs > 0 OR error_provider_timeout > 0 OR error_auth_failed > 0
    OR error_tool_error > 0 OR error_mcp_error > 0 OR error_rate_limited > 0
    OR provider_switches > 0 OR model_switches > 0
  ))
  OR (event = 'turn_end' AND (
    assistant_responses > 0 OR tool_calls > 0 OR executed_tool_calls > 0
    OR file_write_calls > 0 OR tests_run > 0 OR turn_success > 0
  ))
)`;

const LIFECYCLE_EVENTS = "('session_start','turn_end','session_end','session_crash')";

async function one(env, sql) {
  const result = await env.DB.prepare(sql).all();
  return (result.results && result.results[0]) || {};
}

async function many(env, sql) {
  const result = await env.DB.prepare(sql).all();
  return result.results || [];
}

export async function getStats(env) {
  // --- Headline: total users (the one number) -----------------------------
  // A user is a distinct non-CI id that ever installed OR did meaningful work.
  const totals = await one(env, `
    SELECT
      COUNT(DISTINCT CASE WHEN is_ci = 0 AND (event = 'install' OR ${MEANINGFUL_SQL}) THEN telemetry_id END) AS total_users,
      COUNT(DISTINCT CASE WHEN is_ci = 0 AND ${MEANINGFUL_SQL} THEN telemetry_id END) AS core_users,
      COUNT(DISTINCT CASE WHEN is_ci = 0 THEN telemetry_id END) AS reached_users,
      COUNT(DISTINCT CASE WHEN is_ci = 0 AND event = 'install' THEN telemetry_id END) AS installed_users,
      COUNT(DISTINCT telemetry_id) AS all_ids_including_ci,
      COUNT(DISTINCT CASE WHEN is_ci = 1 THEN telemetry_id END) AS ci_ids
    FROM events
  `);

  // --- Active users from the rollup (cheap, ingest-time) -------------------
  // DAU/WAU/MAU as distinct ids, headline = meaningful + release + non-CI.
  const active = await one(env, `
    SELECT
      COUNT(DISTINCT CASE WHEN activity_date = date('now') THEN telemetry_id END) AS dau_raw,
      COUNT(DISTINCT CASE WHEN activity_date = date('now') AND meaningful_active > 0 THEN telemetry_id END) AS dau_meaningful,
      COUNT(DISTINCT CASE WHEN activity_date = date('now') AND meaningful_release_active > 0 AND last_is_ci = 0 THEN telemetry_id END) AS dau,
      COUNT(DISTINCT CASE WHEN activity_date > date('now','-7 days') THEN telemetry_id END) AS wau_raw,
      COUNT(DISTINCT CASE WHEN activity_date > date('now','-7 days') AND meaningful_active > 0 THEN telemetry_id END) AS wau_meaningful,
      COUNT(DISTINCT CASE WHEN activity_date > date('now','-7 days') AND meaningful_release_active > 0 AND last_is_ci = 0 THEN telemetry_id END) AS wau,
      COUNT(DISTINCT CASE WHEN activity_date > date('now','-30 days') THEN telemetry_id END) AS mau_raw,
      COUNT(DISTINCT CASE WHEN activity_date > date('now','-30 days') AND meaningful_active > 0 THEN telemetry_id END) AS mau_meaningful,
      COUNT(DISTINCT CASE WHEN activity_date > date('now','-30 days') AND meaningful_release_active > 0 AND last_is_ci = 0 THEN telemetry_id END) AS mau,
      COUNT(DISTINCT CASE WHEN activity_date > date('now','-30 days') AND last_is_ci = 1 THEN telemetry_id END) AS ci_mau
    FROM daily_active_users
  `);

  // --- Installs and lifecycle totals --------------------------------------
  const lifecycle = await one(env, `
    SELECT
      SUM(CASE WHEN event = 'install' THEN 1 ELSE 0 END) AS install_events,
      SUM(CASE WHEN event = 'upgrade' THEN 1 ELSE 0 END) AS upgrade_events,
      SUM(CASE WHEN event = 'session_start' THEN 1 ELSE 0 END) AS session_starts,
      SUM(CASE WHEN event = 'session_end' THEN 1 ELSE 0 END) AS session_ends,
      SUM(CASE WHEN event = 'session_crash' THEN 1 ELSE 0 END) AS session_crashes,
      SUM(CASE WHEN event = 'turn_end' THEN 1 ELSE 0 END) AS turn_ends,
      COUNT(DISTINCT CASE WHEN event = 'install' THEN telemetry_id END) AS install_ids,
      COUNT(DISTINCT CASE WHEN event = 'install' AND is_ci = 0 THEN telemetry_id END) AS install_ids_noci
    FROM events
    WHERE event IN ('install','upgrade','session_start','turn_end','session_end','session_crash')
  `);
  const lifecycleCompletion =
    (lifecycle.session_starts || 0) > 0
      ? Number(((lifecycle.session_ends + lifecycle.session_crashes) / lifecycle.session_starts).toFixed(3))
      : null;
  const crashRate =
    (lifecycle.session_ends + lifecycle.session_crashes) > 0
      ? Number((lifecycle.session_crashes / (lifecycle.session_ends + lifecycle.session_crashes)).toFixed(4))
      : null;

  // --- New vs returning (last 30d), retention -----------------------------
  const retention = await one(env, `
    WITH cohort AS (
      SELECT DISTINCT telemetry_id FROM events
      WHERE event = 'install' AND is_ci = 0
        AND created_at >= datetime('now','-14 days') AND created_at < datetime('now','-7 days')
    ), retained AS (
      SELECT DISTINCT telemetry_id FROM events
      WHERE event IN ('session_end','session_crash') AND is_ci = 0
        AND created_at >= datetime('now','-7 days')
    )
    SELECT
      (SELECT COUNT(*) FROM cohort) AS d7_cohort,
      (SELECT COUNT(*) FROM cohort WHERE telemetry_id IN retained) AS d7_retained
  `);
  const d7Retention =
    (retention.d7_cohort || 0) > 0
      ? Number((retention.d7_retained / retention.d7_cohort).toFixed(3))
      : null;

  // --- 30d engagement quality ---------------------------------------------
  const quality = await one(env, `
    SELECT
      AVG(duration_mins) AS avg_session_mins,
      AVG(turns) AS avg_turns,
      AVG(CASE WHEN session_success > 0 THEN 1.0 ELSE 0.0 END) AS success_rate,
      AVG(CASE WHEN abandoned_before_response > 0 THEN 1.0 ELSE 0.0 END) AS abandon_rate,
      AVG(first_assistant_response_ms) AS avg_first_response_ms,
      AVG(CASE WHEN executed_tool_calls > 0 THEN CAST(tool_latency_total_ms AS REAL)/executed_tool_calls END) AS avg_tool_latency_ms,
      SUM(input_tokens + output_tokens) AS tokens_30d,
      AVG(CASE WHEN multi_sessioned > 0 THEN 1.0 ELSE 0.0 END) AS multi_session_rate
    FROM events
    WHERE event IN ('session_end','session_crash')
      AND is_ci = 0 AND created_at > datetime('now','-30 days')
  `);

  // --- Per-turn metrics (30d) ---------------------------------------------
  const turns = await one(env, `
    SELECT
      AVG(turn_active_duration_ms) AS avg_turn_ms,
      AVG(CASE WHEN turn_success > 0 THEN 1.0 ELSE 0.0 END) AS turn_success_rate
    FROM events
    WHERE event = 'turn_end' AND is_ci = 0 AND created_at > datetime('now','-30 days')
  `);

  // --- Errors (30d) --------------------------------------------------------
  const errors = await one(env, `
    SELECT
      SUM(error_provider_timeout) AS provider_timeout,
      SUM(error_auth_failed) AS auth_failed,
      SUM(error_tool_error) AS tool_error,
      SUM(error_mcp_error) AS mcp_error,
      SUM(error_rate_limited) AS rate_limited
    FROM events
    WHERE event IN ('session_end','session_crash') AND is_ci = 0
      AND created_at > datetime('now','-30 days')
  `);

  // --- Feature adoption (30d, distinct users) -----------------------------
  const features = await one(env, `
    SELECT
      COUNT(DISTINCT CASE WHEN feature_memory_used > 0 THEN telemetry_id END) AS memory,
      COUNT(DISTINCT CASE WHEN feature_swarm_used > 0 THEN telemetry_id END) AS swarm,
      COUNT(DISTINCT CASE WHEN feature_web_used > 0 THEN telemetry_id END) AS web,
      COUNT(DISTINCT CASE WHEN feature_email_used > 0 THEN telemetry_id END) AS email,
      COUNT(DISTINCT CASE WHEN feature_mcp_used > 0 THEN telemetry_id END) AS mcp,
      COUNT(DISTINCT CASE WHEN feature_side_panel_used > 0 THEN telemetry_id END) AS side_panel,
      COUNT(DISTINCT CASE WHEN feature_goal_used > 0 THEN telemetry_id END) AS goal,
      COUNT(DISTINCT CASE WHEN feature_selfdev_used > 0 THEN telemetry_id END) AS selfdev,
      COUNT(DISTINCT CASE WHEN feature_background_used > 0 THEN telemetry_id END) AS background,
      COUNT(DISTINCT CASE WHEN feature_subagent_used > 0 THEN telemetry_id END) AS subagent
    FROM events
    WHERE event IN ('session_end','session_crash') AND is_ci = 0
      AND created_at > datetime('now','-30 days')
  `);

  // --- Transport mix (30d) -------------------------------------------------
  const transport = await one(env, `
    SELECT
      SUM(transport_https) AS https,
      SUM(transport_persistent_ws_fresh) AS ws_fresh,
      SUM(transport_persistent_ws_reuse) AS ws_reuse,
      SUM(transport_cli_subprocess) AS cli,
      SUM(transport_native_http2) AS native_http2,
      SUM(transport_other) AS other
    FROM events
    WHERE event IN ('session_end','session_crash') AND is_ci = 0
      AND created_at > datetime('now','-30 days')
  `);

  // --- Breakdowns (distinct users) ----------------------------------------
  const versions = await many(env, `
    SELECT version, COUNT(DISTINCT telemetry_id) AS users
    FROM events WHERE is_ci = 0 AND version IS NOT NULL
    GROUP BY version ORDER BY users DESC LIMIT 12
  `);
  const os = await many(env, `
    SELECT os, COUNT(DISTINCT telemetry_id) AS users
    FROM events WHERE is_ci = 0 AND os IS NOT NULL
    GROUP BY os ORDER BY users DESC
  `);
  const channels = await many(env, `
    SELECT COALESCE(build_channel,'unknown') AS build_channel, COUNT(DISTINCT telemetry_id) AS users
    FROM events WHERE event IN ('session_end','session_crash')
    GROUP BY build_channel ORDER BY users DESC
  `);
  const providers = await many(env, `
    SELECT COALESCE(provider_end,'unknown') AS provider, COUNT(DISTINCT telemetry_id) AS users
    FROM events WHERE event IN ('session_end','session_crash') AND is_ci = 0 AND ${MEANINGFUL_SQL}
    GROUP BY provider_end ORDER BY users DESC LIMIT 12
  `);
  const auth = await many(env, `
    SELECT COALESCE(auth_provider,'unknown') AS auth_provider, COUNT(DISTINCT telemetry_id) AS users
    FROM events WHERE event = 'auth_success' AND is_ci = 0
    GROUP BY auth_provider ORDER BY users DESC LIMIT 12
  `);
  const onboarding = await many(env, `
    SELECT step, COUNT(DISTINCT telemetry_id) AS users
    FROM events WHERE event = 'onboarding_step' AND is_ci = 0 AND step IS NOT NULL
    GROUP BY step ORDER BY users DESC
  `);

  // --- Daily timeseries (last 60 days) for charts -------------------------
  const daily = await many(env, `
    SELECT
      activity_date AS date,
      COUNT(DISTINCT telemetry_id) AS raw,
      COUNT(DISTINCT CASE WHEN meaningful_active > 0 THEN telemetry_id END) AS meaningful,
      COUNT(DISTINCT CASE WHEN meaningful_release_active > 0 AND last_is_ci = 0 THEN telemetry_id END) AS headline,
      COUNT(DISTINCT CASE WHEN last_is_ci = 1 THEN telemetry_id END) AS ci
    FROM daily_active_users
    WHERE activity_date > date('now','-60 days')
    GROUP BY activity_date ORDER BY activity_date
  `);
  const dailyInstalls = await many(env, `
    SELECT date(created_at) AS date, COUNT(DISTINCT telemetry_id) AS installs
    FROM events
    WHERE event = 'install' AND is_ci = 0 AND created_at > datetime('now','-60 days')
    GROUP BY date(created_at) ORDER BY date(created_at)
  `);

  // --- Recent feedback (text only, no identifiers) ------------------------
  const feedback = await many(env, `
    SELECT created_at, feedback_text, feedback_rating, feedback_reason, version
    FROM events
    WHERE event = 'feedback' AND feedback_text IS NOT NULL
    ORDER BY created_at DESC LIMIT 25
  `);

  return {
    generated_at: new Date().toISOString(),
    headline: {
      total_users: totals.total_users || 0,
      dau: active.dau || 0,
      wau: active.wau || 0,
      mau: active.mau || 0,
    },
    users: {
      total_users: totals.total_users || 0,
      core_users: totals.core_users || 0,
      installed_users: totals.installed_users || 0,
      reached_users: totals.reached_users || 0,
      all_ids_including_ci: totals.all_ids_including_ci || 0,
      ci_ids: totals.ci_ids || 0,
    },
    active,
    lifecycle: { ...lifecycle, lifecycle_completion_ratio: lifecycleCompletion, crash_rate: crashRate },
    retention: { ...retention, d7_retention: d7Retention },
    quality,
    turns,
    errors,
    features,
    transport,
    breakdowns: { versions, os, channels, providers, auth, onboarding },
    timeseries: { daily, installs: dailyInstalls },
    feedback,
  };
}
