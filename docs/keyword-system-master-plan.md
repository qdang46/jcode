# Magic Keyword System — Master Plan
> Issue #391 | Full keyword detection + workflow handlers
> Research: oh-my-codex, oh-my-openagent, oh-my-claudecode, claude-code, codebuff, oh-my-pi

## Context

jcode has `$<name>` skill invocation but no natural language keyword detection or workflow orchestration. This plan adds:
1. **Keyword detection** — NL triggers with sanitization, intent disambiguation, multilingual
2. **Workflow handlers** — full execution logic for each keyword (spawn agents, enforce rules, aggregate results)
3. **Mode state** — persistent across turns and session restarts
4. **Visual effects** — rainbow highlighting, shimmer, toasts
5. **Task size classification** — suppress heavy modes for simple tasks
6. **Cancel system** — stopjcode/canceljcode

**Removed:** ralph loop, autopilot pipeline, Oracle verification.

---

## Architecture

### Crate: `jcode-keywords`

```
crates/jcode-keywords/
├── Cargo.toml
└── src/
    ├── lib.rs                  # public API
    ├── registry.rs             # KEYWORD_REGISTRY static
    ├── detector.rs             # detection engine
    ├── sanitizer.rs            # strip code blocks, URLs, quotes, etc.
    ├── intent.rs               # informational vs activation intent
    ├── task_size.rs            # small/medium/large classification
    ├── conflict.rs             # priority resolution
    ├── state.rs                # TOML mode state persistence
    ├── prompt_builder.rs       # build prompt injections per mode
    ├── visual.rs               # visual effect types
    ├── workflow/
    │   ├── mod.rs              # WorkflowHandler trait + dispatch
    │   ├── ultrawork.rs        # parallel agent orchestration
    │   ├── ultragoal.rs        # durable goal tracking
    │   ├── ultraqa.rs          # QA cycling
    │   ├── ralplan.rs          # consensus planning
    │   ├── deep_interview.rs   # requirements gathering
    │   ├── tdd.rs              # test-driven development
    │   ├── code_review.rs      # code review agent
    │   ├── security_review.rs  # security review agent
    │   ├── ultrathink.rs       # extended thinking
    │   ├── deepsearch.rs       # thorough codebase search
    │   ├── analyze.rs          # deep analysis
    │   ├── wiki.rs             # documentation lookup
    │   └── ai_slop_cleaner.rs  # fix AI-generated code
    └── tests.rs
```

### Integration

```
User types message
  → TUI input.rs: detect keywords, highlight rainbow, show toast
  → Send to agent
  → prompting.rs: build_system_prompt_split()
      → keyword detector runs on latest user message
      → for each matched keyword:
          → activate mode (persist to .jcode/state/modes.toml)
          → build mode-specific prompt injection
          → if workflow handler needed → spawn workflow
      → append to dynamic_part
  → Agent processes with enhanced prompt
  → Workflow handlers manage sub-agents, state, termination
```

---

## Part 1: Keyword Detection

### 1.1 Registry (`registry.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordEntry {
    pub keyword: &'static str,
    pub skill: &'static str,
    pub priority: u8,                    // 5-11
    pub guidance: &'static str,          // prompt injection text
    pub aliases: &'static [&'static str],
    pub requires_explicit: bool,         // $prefix or activation verbs
    pub is_heavy: bool,                  // suppress for small tasks
    pub visual_effect: VisualEffect,     // Rainbow, Shimmer, Toast, None
    pub workflow_type: WorkflowType,     // determines handler
}
```

**Complete registry:**

| Pri | Skill | Keyword | Aliases | Heavy | Visual | Workflow |
|-----|-------|---------|---------|-------|--------|----------|
| 11 | ralplan | `$ralplan` | "ralplan", "consensus plan" | yes | Toast | ConsensusPlanning |
| 10 | ultrawork | `$ultrawork` | "ulw", "uw", "parallel", "don't stop", "must complete", "keep going" | yes | Rainbow+Shimmer | ParallelExecution |
| 10 | ultragoal | `$ultragoal` | "ultragoal" | yes | Toast | GoalTracking |
| 8 | ultraqa | `$ultraqa` | "ultraqa", "qa cycle" | yes | Toast | QACycling |
| 8 | deep-interview | `$deep-interview` | "ouroboros", "interview me", "gather requirements", "ask me" | no | Toast | RequirementsGathering |
| 7 | ultrathink | `$ultrathink` | "think hard", "think deeply", "think carefully" | no | Rainbow+Shimmer | ExtendedThinking |
| 7 | deepsearch | `$deepsearch` | "search the codebase", "find in codebase", "thorough search" | no | Toast | CodebaseSearch |
| 7 | tdd | `$tdd` | "test first", "red green", "test-driven" | no | Toast | TestDrivenDev |
| 6 | code-review | `$code-review` | "code review", "review code", "review this" | no | Toast | CodeReview |
| 6 | security-review | `$security-review` | "security review", "review security", "audit security" | no | Toast | SecurityReview |
| 6 | analyze | `$analyze` | "deep-analyze", "deepanalyze", "deep analysis" | no | Toast | DeepAnalysis |
| 5 | wiki | `$wiki` | "wiki this", "look up docs" | no | Toast | DocLookup |
| 5 | ai-slop-cleaner | — | compound: action + smell word | no | Toast | SlopCleanup |
| 9 | cancel | `canceljcode` | "stopjcode" | no | Toast | CancelAll |

**Multilingual triggers** (from oh-my-openagent):
- Search: 64 triggers (EN: search/find/locate, KO: 검색/찾아, JA: 検索/探して, ZH: 搜索/查找, VI: tìm kiếm/tra cứu)
- Analyze: 64 triggers (EN: analyze/investigate, KO: 분석/조사, JA: 分析/調査, ZH: 分析/调查, VI: phân tích/điều tra)

### 1.2 Sanitizer (`sanitizer.rs`)

```rust
pub fn sanitize(input: &str) -> String {
    // 1. Strip fenced code blocks ```...```
    // 2. Strip inline code `...`
    // 3. Strip URLs https?://...
    // 4. Strip XML tags <system-reminder>...</system-reminder>
    // 5. Strip HTML comments <!--...-->
    // 6. Strip block quotes > ...
    // 7. Strip quoted spans "..." and '...'
    // 8. Strip system echo blocks (previously injected mode banners)
}
```

### 1.3 Intent Disambiguation (`intent.rs`)

```rust
// "what is ultrawork?" → Informational (DON'T activate)
// "run ultrawork" → Activation (DO activate)
// "ultrawork keeps failing" → Diagnostic (DON'T activate)
// "use ultrawork to fix this" → Activation (DO activate)

pub fn classify_intent(text: &str, keyword: &str) -> Intent;
pub fn is_informational_context(text: &str) -> bool;
pub fn has_activation_intent(text: &str, keyword: &str) -> bool;
pub fn is_diagnostic_context(text: &str) -> bool;
```

### 1.4 Task Size (`task_size.rs`)

```rust
// Escape hatches: "quick:", "simple:", "tiny:", "minor:", "small:", "just:", "only:"
// Small signals: typo, rename, single file, one-liner, bump version
// Large signals: architecture, refactor, from scratch, migration, full-stack
// Word count: <50 = small bias, >200 = large bias

pub fn classify(input: &str) -> TaskSize; // Small | Medium | Large
```

### 1.5 Conflict Resolution (`conflict.rs`)

```rust
// When multiple keywords match:
// 1. Cancel always wins (priority 9, exclusive)
// 2. Highest priority wins per skill
// 3. Combo types (hyperplan-ultrawork) suppress standalone variants
// 4. Already-active modes are filtered out

pub fn resolve(matches: &mut Vec<DetectedKeyword>);
```

---

## Part 2: Workflow Handlers

### 2.1 WorkflowHandler Trait (`workflow/mod.rs`)

```rust
#[async_trait]
pub trait WorkflowHandler: Send + Sync {
    /// Name of this workflow
    fn name(&self) -> &str;

    /// Is this workflow heavy? (suppress for small tasks)
    fn is_heavy(&self) -> bool;

    /// Build prompt injection for system prompt
    fn build_prompt(&self, entry: &KeywordEntry, config: &KeywordConfig) -> String;

    /// Execute the workflow (called after prompt injection)
    /// Returns workflow state for tracking
    async fn execute(&self, ctx: &WorkflowContext) -> Result<WorkflowResult>;

    /// Check if workflow should continue or terminate
    fn should_continue(&self, state: &WorkflowState) -> bool;

    /// Build continuation prompt for next turn
    fn build_continuation(&self, state: &WorkflowState) -> String;

    /// Cleanup on cancel/terminate
    async fn on_cancel(&self, state: &WorkflowState) -> Result<()>;
}

pub struct WorkflowContext {
    pub session_id: String,
    pub user_message: String,
    pub working_dir: PathBuf,
    pub config: KeywordConfig,
    pub state_store: ModeStateStore,
    pub agent_tx: AgentEventSender,
}

pub enum WorkflowResult {
    /// Single-turn workflow (no persistence needed)
    SingleTurn { response: String },
    /// Multi-turn workflow (persist state, continue next turn)
    MultiTurn { state: WorkflowState },
    /// Spawned sub-agents (track their completion)
    Spawned { agent_ids: Vec<String> },
}

pub struct WorkflowState {
    pub workflow_type: WorkflowType,
    pub iteration: u32,
    pub max_iterations: u32,
    pub started_at: DateTime<Utc>,
    pub session_id: String,
    pub data: serde_json::Value, // workflow-specific state
}
```

### 2.2 Ultrawork — Parallel Execution (`workflow/ultrawork.rs`)

**Source:** oh-my-openagent (ultrawork mode), codebuff (4-agent pipeline)

**Behavior:**
1. Analyze user task → decompose into N parallel subtasks
2. Spawn up to 4 sub-agents (configurable `max_concurrency`)
3. Each sub-agent works on one subtask independently
4. Coordinator monitors progress, aggregates results
5. If subtask fails → retry or redistribute

**State:**
```rust
struct UltraworkState {
    subtasks: Vec<Subtask>,
    agent_handles: Vec<AgentHandle>,
    completed: usize,
    failed: usize,
    max_concurrency: usize, // default 4
}

struct Subtask {
    id: String,
    description: String,
    agent_id: Option<String>,
    status: SubtaskStatus, // Pending | Running | Completed | Failed
    result: Option<String>,
}
```

**Prompt injection:**
```
<keyword-mode skill="ultrawork" priority="10">
**ULTRAWORK MODE ENABLED!**

Execute with maximum parallelism:
1. Decompose the task into independent subtasks
2. Spawn up to {max_concurrency} concurrent sub-agents
3. Each sub-agent works independently on its subtask
4. Aggregate results when all complete
5. If a subtask fails, retry or redistribute

Use `subagent` tool with `run_in_background=true` for parallel execution.
Use `task_create`/`task_update` to track subtask progress.
</keyword-mode>
```

**Termination:** All subtasks completed OR max_iterations (10) reached.

**Per-model variants:**
- Claude: detailed decomposition instructions
- GPT: structured task breakdown with explicit tool calls
- Gemini: visual-oriented decomposition

### 2.3 Ultragoal — Durable Goal Tracking (`workflow/ultragoal.rs`)

**Source:** codex (goals system), oh-my-codex (ultragoal)

**Behavior:**
1. Create a structured goal with success criteria
2. Set token budget for the goal
3. Track progress across turns
4. Inject remaining budget and progress into each turn
5. Auto-continue until goal complete or budget exhausted

**State:**
```rust
struct UltragoalState {
    goal: String,
    success_criteria: Vec<String>,
    token_budget: usize,
    tokens_used: usize,
    progress: Vec<ProgressEntry>,
    status: GoalStatus, // Active | Complete | Blocked | Exhausted
}
```

**Prompt injection:**
```
<keyword-mode skill="ultragoal" priority="10">
**ULTRAGOAL MODE ENABLED!**

Active goal: {goal}
Success criteria:
{criteria_list}

Progress: {completed}/{total} criteria met
Token budget: {remaining}/{total} remaining

Continue working toward the goal. Update progress after each step.
Output <goal-complete/> when all criteria are met.
</keyword-mode>
```

**Termination:** All criteria met OR budget exhausted OR user cancels.

### 2.4 Ultraqa — QA Cycling (`workflow/ultraqa.rs`)

**Source:** oh-my-codex (ultraqa), codebuff (reviewer agent)

**Behavior:**
1. Implement the requested change
2. Run tests (cargo test, or project-specific)
3. If tests fail → analyze failures → fix → repeat
4. If tests pass → run additional checks (clippy, format)
5. Max cycles: 10

**State:**
```rust
struct UltraqaState {
    cycle: u32,
    max_cycles: u32, // default 10
    test_results: Vec<TestRun>,
    status: QaStatus, // Implementing | Testing | Fixing | Passed | Failed
}

struct TestRun {
    cycle: u32,
    passed: usize,
    failed: usize,
    errors: Vec<String>,
}
```

**Prompt injection:**
```
<keyword-mode skill="ultraqa" priority="8">
**ULTRAQA MODE ENABLED!**

QA Cycle {cycle}/{max_cycles}:
1. Implement the requested change
2. Run tests: cargo test
3. If failures: analyze, fix, repeat
4. If pass: run cargo clippy and cargo fmt
5. Continue until all tests pass

Previous run: {passed} passed, {failed} failed
{failure_details}
</keyword-mode>
```

**Termination:** All tests pass OR max_cycles reached.

### 2.5 Ralplan — Consensus Planning (`workflow/ralplan.rs`)

**Source:** oh-my-codex (ralplan), oh-my-openagent (hyperplan)

**Behavior:**
1. Generate implementation plan
2. Spawn adversarial critic agent (Metis/Momus pattern)
3. Critic reviews plan, finds gaps
4. Revise plan based on feedback
5. Repeat until critic approves (consensus)
6. Present final plan to user

**State:**
```rust
struct RalplanState {
    plan: String,
    revision: u32,
    max_revisions: u32, // default 5
    critic_feedback: Vec<CriticReview>,
    status: PlanStatus, // Drafting | Reviewing | Revising | Approved | Rejected
}

struct CriticReview {
    revision: u32,
    gaps: Vec<String>,
    approved: bool,
    rationale: String,
}
```

**Prompt injection:**
```
<keyword-mode skill="ralplan" priority="11">
**RALPLAN MODE ENABLED!**

Consensus planning workflow:
1. Draft implementation plan
2. Self-review for gaps and risks
3. Revise until plan is solid
4. Present final plan before execution

Plan revision: {revision}/{max_revisions}
Previous feedback: {feedback_summary}
</keyword-mode>
```

**Termination:** Critic approves OR max_revisions reached.

### 2.6 Deep-Interview — Requirements Gathering (`workflow/deep_interview.rs`)

**Source:** oh-my-codex (deep-interview), oh-my-openagent (ouroboros)

**Behavior:**
1. Analyze user request for ambiguities
2. Ask clarifying questions (max 5 per round)
3. Score ambiguity level (0-100)
4. If ambiguity > threshold → ask more questions
5. If ambiguity ≤ threshold → summarize requirements → proceed
6. Max rounds: 3

**State:**
```rust
struct DeepInterviewState {
    round: u32,
    max_rounds: u32, // default 3
    questions_asked: Vec<Question>,
    answers: Vec<Answer>,
    ambiguity_score: u32, // 0-100
    requirements: Option<String>,
    status: InterviewStatus, // Analyzing | Questioning | Summarizing | Complete
}
```

**Prompt injection:**
```
<keyword-mode skill="deep-interview" priority="8">
**DEEP INTERVIEW MODE ENABLED!**

Before implementing, gather requirements:
1. Identify ambiguities in the request
2. Ask clarifying questions (max 5 per round)
3. Score ambiguity level
4. Continue until ambiguity ≤ 20

Round {round}/{max_rounds}
Ambiguity score: {ambiguity_score}/100
Questions asked: {questions_count}
</keyword-mode>
```

**Termination:** Ambiguity ≤ 20 OR max_rounds reached.

### 2.7 TDD — Test-Driven Development (`workflow/tdd.rs`)

**Source:** oh-my-codex (tdd), oh-my-pi (edit benchmark)

**Behavior:**
1. Write test first (expect fail)
2. Run test → verify it fails
3. Implement minimal code to pass
4. Run test → verify it passes
5. Refactor if needed
6. Repeat for next test case

**State:**
```rust
struct TddState {
    phase: TddPhase, // WriteTest | VerifyFail | Implement | VerifyPass | Refactor
    tests_written: usize,
    tests_passed: usize,
    current_test: Option<String>,
    cycle: u32,
}
```

**Prompt injection:**
```
<keyword-mode skill="tdd" priority="7">
**TDD MODE ENABLED!**

Test-Driven Development workflow:
1. Write test FIRST (before implementation)
2. Run test → verify it FAILS (red)
3. Implement minimal code to pass
4. Run test → verify it PASSES (green)
5. Refactor if needed
6. Repeat for next test

Phase: {phase}
Tests written: {tests_written}, passed: {tests_passed}
</keyword-mode>
```

**Termination:** All tests pass.

### 2.8 Code Review (`workflow/code_review.rs`)

**Source:** codebuff (reviewer agent), oh-my-claudecode

**Behavior:**
1. Identify changed files (git diff)
2. Spawn reviewer agent with review prompt
3. Reviewer analyzes: correctness, style, performance, security
4. Generate structured review report
5. Present findings to user

**State:**
```rust
struct CodeReviewState {
    files_changed: Vec<String>,
    findings: Vec<Finding>,
    status: ReviewStatus, // Scanning | Reviewing | Complete
}

struct Finding {
    file: String,
    line: Option<usize>,
    severity: Severity, // Critical | Warning | Info
    category: Category, // Correctness | Style | Performance | Security
    description: String,
    suggestion: String,
}
```

**Prompt injection:**
```
<keyword-mode skill="code-review" priority="6">
**CODE REVIEW MODE ENABLED!**

Review the recent changes:
1. Check correctness (logic errors, edge cases)
2. Check style (consistency, readability)
3. Check performance (inefficiencies, allocations)
4. Check security (input validation, secrets)

Files to review: {file_list}
</keyword-mode>
```

**Termination:** Review complete.

### 2.9 Security Review (`workflow/security_review.rs`)

**Source:** codex (guardian), oh-my-claudecode

**Behavior:**
1. Scan for common vulnerabilities (OWASP Top 10)
2. Check for secrets in code
3. Validate input handling
4. Check authentication/authorization patterns
5. Generate security report with severity levels

**State:**
```rust
struct SecurityReviewState {
    vulnerabilities: Vec<Vulnerability>,
    secrets_found: Vec<Secret>,
    status: ReviewStatus,
}

struct Vulnerability {
    file: String,
    line: Option<usize>,
    severity: Severity,
    category: String, // SQLi, XSS, CSRF, etc.
    description: String,
    fix_suggestion: String,
}
```

**Prompt injection:**
```
<keyword-mode skill="security-review" priority="6">
**SECURITY REVIEW MODE ENABLED!**

Security audit:
1. Check OWASP Top 10 vulnerabilities
2. Scan for hardcoded secrets/credentials
3. Validate input sanitization
4. Check auth/authz patterns
5. Review dependency vulnerabilities

Generate structured security report.
</keyword-mode>
```

**Termination:** Review complete.

### 2.10 Ultrathink — Extended Thinking (`workflow/ultrathink.rs`)

**Source:** claude-code (rainbow on ultrathink)

**Behavior:**
1. Inject "think deeply" directive into system prompt
2. Enable extended thinking/reasoning mode (if provider supports)
3. No sub-agents needed — single-turn enhancement
4. Visual: rainbow highlighting on keyword in input

**State:** None (single-turn).

**Prompt injection:**
```
<keyword-mode skill="ultrathink" priority="7">
**ULTRATHINK MODE ENABLED!**

Think deeply and thoroughly before acting:
- Consider all edge cases
- Analyze trade-offs
- Think step-by-step
- Consider alternative approaches
- Verify your reasoning before executing
</keyword-mode>
```

**Termination:** Single turn (no persistence).

### 2.11 Deepsearch — Codebase Search (`workflow/deepsearch.rs`)

**Source:** oh-my-codex (deepsearch), codebuff (file-picker)

**Behavior:**
1. Analyze what the user is looking for
2. Search across codebase using multiple strategies:
   - Grep for text patterns
   - AST search for structural matches
   - File name matching
   - Symbol search via LSP
3. Build context map of relevant files
4. Present organized findings

**State:**
```rust
struct DeepsearchState {
    query: String,
    strategies_used: Vec<SearchStrategy>,
    files_found: Vec<SearchResult>,
    status: SearchStatus, // Analyzing | Searching | Organizing | Complete
}
```

**Prompt injection:**
```
<keyword-mode skill="deepsearch" priority="7">
**DEEPSEARCH MODE ENABLED!**

Thorough codebase search:
1. Use grep, glob, lsp tools to search
2. Search for: {query}
3. Check multiple strategies (text, structure, symbols)
4. Build context map of relevant files
5. Present organized findings
</keyword-mode>
```

**Termination:** Search complete.

### 2.12 Analyze — Deep Analysis (`workflow/analyze.rs`)

**Source:** oh-my-codex (analyze)

**Behavior:**
1. Identify what to analyze
2. Gather relevant code/context
3. Perform structured analysis
4. Generate report with findings and recommendations

**State:** Single-turn or multi-turn depending on scope.

**Prompt injection:**
```
<keyword-mode skill="analyze" priority="6">
**ANALYZE MODE ENABLED!**

Deep analysis workflow:
1. Identify the subject of analysis
2. Gather all relevant context
3. Analyze systematically (structure, patterns, issues)
4. Generate structured report with findings and recommendations
</keyword-mode>
```

### 2.13 Wiki — Documentation Lookup (`workflow/wiki.rs`)

**Source:** oh-my-codex (wiki)

**Behavior:**
1. Identify what documentation is needed
2. Search local docs (README, docs/, etc.)
3. Search web if needed (websearch, webfetch)
4. Generate summary

**State:** Single-turn.

**Prompt injection:**
```
<keyword-mode skill="wiki" priority="5">
**WIKI MODE ENABLED!**

Documentation lookup:
1. Search local documentation first
2. Check README, docs/, comments
3. If needed, search the web
4. Provide clear, cited summary
</keyword-mode>
```

### 2.14 AI Slop Cleaner (`workflow/ai_slop_cleaner.rs`)

**Source:** oh-my-claudecode (ai-slop-cleaner)

**Behavior:**
1. Detect AI-generated low-quality patterns:
   - Excessive comments explaining obvious code
   - Overly defensive error handling
   - Unnecessary abstractions
   - Verbose variable names
   - Redundant code patterns
2. Clean up detected slop
3. Improve code quality

**State:**
```rust
struct SlopCleanerState {
    files_scanned: usize,
    slop_found: Vec<SlopPattern>,
    fixes_applied: usize,
    status: CleanupStatus,
}
```

**Prompt injection:**
```
<keyword-mode skill="ai-slop-cleaner" priority="5">
**AI SLOP CLEANER MODE ENABLED!**

Detect and fix AI-generated low-quality code:
1. Look for excessive/obvious comments
2. Check for over-engineering
3. Find redundant patterns
4. Simplify verbose code
5. Maintain functionality while improving clarity
</keyword-mode>
```

### 2.15 Cancel (`workflow/cancel.rs`)

**Behavior:**
1. Clear all active mode states
2. Cancel running background tasks
3. Show toast notification
4. No prompt injection needed

```rust
fn handle_cancel(state: &mut ModeStateStore, session_id: &str) {
    state.clear_all();
    // Cancel background tasks
    event_tx.send(AppEvent::CancelBackgroundTasks { session_id });
    event_tx.send(AppEvent::ShowToast {
        message: "All modes cancelled".to_string(),
        style: ToastStyle::Info,
    });
}
```

---

## Part 3: Mode State Persistence

### File: `.jcode/state/modes.toml`

```toml
[active_modes.ultrawork]
active = true
started_at = "2026-06-06T00:00:00Z"
session_id = "ses_abc123"
iteration = 3
max_iterations = 10
workflow_state = '{ "subtasks": [...], "completed": 2 }'

[active_modes.deep-interview]
active = true
started_at = "2026-06-06T00:01:00Z"
session_id = "ses_abc123"
iteration = 1
max_iterations = 3
workflow_state = '{ "round": 1, "ambiguity_score": 45 }'
```

### State Operations

```rust
pub struct ModeStateStore { ... }

impl ModeStateStore {
    pub fn load() -> Result<Self>;           // Load from .jcode/state/modes.toml
    pub fn save(&self) -> Result<()>;        // Save to .jcode/state/modes.toml
    pub fn activate(&mut self, skill: &str, ctx: &WorkflowContext);
    pub fn deactivate(&mut self, skill: &str);
    pub fn clear_all(&mut self);
    pub fn is_active(&self, skill: &str) -> bool;
    pub fn active_modes(&self) -> Vec<String>;
    pub fn update_workflow_state(&mut self, skill: &str, state: serde_json::Value);
    pub fn get_workflow_state(&self, skill: &str) -> Option<&serde_json::Value>;
    pub fn is_stale(&self, skill: &str) -> bool; // 2-hour TTL
}
```

---

## Part 4: TUI Visual Effects

### 4.1 Rainbow Highlighting (from claude-code)

```rust
const RAINBOW_COLORS: [Color; 7] = [
    Color::Rgb(235, 95, 87),   // red
    Color::Rgb(245, 139, 87),  // orange
    Color::Rgb(250, 195, 95),  // yellow
    Color::Rgb(145, 200, 130), // green
    Color::Rgb(130, 170, 220), // blue
    Color::Rgb(155, 130, 200), // indigo
    Color::Rgb(200, 130, 180), // violet
];

fn highlight_keyword_rainbow(input: &str, keyword_positions: &[Range<usize>]) -> Vec<StyledChar> {
    // Each character of the keyword gets RAINBOW_COLORS[char_index % 7]
}
```

### 4.2 Shimmer Animation (from claude-code)

```rust
// 20fps animation loop
struct ShimmerState {
    glimmer_index: usize,
    direction: Direction, // LeftToRight
}

// Characters within ±1 of glimmer_index get brighter shimmer color
fn update_shimmer(state: &mut ShimmerState, keyword_len: usize) {
    state.glimmer_index = (state.glimmer_index + 1) % keyword_len;
}
```

### 4.3 Toast Notifications

```rust
pub enum ToastStyle {
    Success,  // green
    Info,     // blue
    Warning,  // yellow
    Error,    // red
}

// On mode activation:
event_tx.send(AppEvent::ShowToast {
    message: format!("{} Mode Activated", skill_name.to_uppercase()),
    duration: Duration::from_secs(3),
    style: ToastStyle::Success,
});
```

### 4.4 Mode Indicator in Info Widget

Show active modes in the TUI info widget:
```
[ultrawork: cycle 3/10] [deep-interview: round 1/3]
```

---

## Part 5: Config

### `.jcode/config.json`

```json
{
  "keywords": {
    "enabled": true,
    "disabled_keywords": [],
    "max_concurrency": 4,
    "task_size": {
      "small_word_limit": 50,
      "large_word_limit": 200
    },
    "visual_effects": {
      "rainbow": true,
      "shimmer": true,
      "toast": true
    }
  }
}
```

---

## Part 6: Implementation Order

### Phase 1: Detection Engine
1. Create `jcode-keywords` crate
2. `registry.rs` — all 50+ keyword entries
3. `sanitizer.rs` — code blocks, URLs, quotes, system echoes
4. `detector.rs` — matching engine
5. `intent.rs` — informational vs activation
6. `task_size.rs` — classification
7. `conflict.rs` — priority resolution
8. Unit tests

### Phase 2: State + Prompt Injection
9. `state.rs` — TOML persistence
10. `prompt_builder.rs` — build injections per mode
11. Integrate with `prompting.rs` (Point C)
12. Integration tests

### Phase 3: Workflow Handlers (Core)
13. `workflow/mod.rs` — trait + dispatch
14. `workflow/ultrawork.rs` — parallel execution
15. `workflow/deep_interview.rs` — requirements gathering
16. `workflow/tdd.rs` — test-driven dev
17. `workflow/code_review.rs` — code review
18. `workflow/ultrathink.rs` — extended thinking

### Phase 4: Workflow Handlers (Extended)
19. `workflow/ultragoal.rs` — goal tracking
20. `workflow/ultraqa.rs` — QA cycling
21. `workflow/ralplan.rs` — consensus planning
22. `workflow/security_review.rs` — security review
23. `workflow/deepsearch.rs` — codebase search
24. `workflow/analyze.rs` — deep analysis
25. `workflow/wiki.rs` — doc lookup
26. `workflow/ai_slop_cleaner.rs` — slop cleanup
27. `workflow/cancel.rs` — cancel all

### Phase 5: TUI Effects
28. Rainbow highlighting in input
29. Shimmer animation
30. Toast notifications
31. Mode indicator in info widget

### Phase 6: Config + Polish
32. Config integration
33. E2E tests
34. Documentation

---

## Files Modified/Created

| File | Change |
|------|--------|
| `Cargo.toml` | Add `jcode-keywords` workspace member |
| `crates/jcode-keywords/` | **NEW** — 20+ files |
| `crates/jcode-app-core/src/agent/prompting.rs` | Keyword detection + prompt injection |
| `crates/jcode-app-core/src/agent/turn_loops.rs` | Pass user message to detector |
| `crates/jcode-tui/src/tui/app/input.rs` | Rainbow + shimmer effects |
| `crates/jcode-tui/src/tui/app/state_ui_input_helpers.rs` | Cancel command |
| `crates/jcode-tui/src/tui/info_widget.rs` | Active modes display |
| `crates/jcode-config-types/src/lib.rs` | KeywordsConfig |
| `crates/jcode-base/src/config.rs` | Load keywords config |

## Verification

1. `cargo check -p jcode-keywords` — compiles
2. `cargo test -p jcode-keywords` — unit tests pass
3. `cargo test -p jcode-app-core` — integration tests pass
4. Manual: "run ultrawork" → mode activates, parallel agents spawn
5. Manual: "what is ultrawork?" → no activation (informational)
6. Manual: "fix typo" → heavy modes suppressed (small task)
7. Manual: "canceljcode" → all modes cleared
8. Visual: "ultrathink" → rainbow highlighting in input
9. Workflow: ultrawork → 4 agents spawn → results aggregate
10. Workflow: deep-interview → questions asked → requirements summarized
11. Workflow: tdd → test written → fails → implements → passes

---

## Part 7: Detailed Detection Logic Per Keyword

### 7.1 Keyword Detection Regex (Exact)

Each keyword has primary match + aliases. Detection uses word-boundary matching to prevent false positives.

```rust
// Example: ultrawork
fn match_ultrawork(text: &str) -> bool {
    // Primary: $ultrawork (explicit, always match)
    if text.contains("$ultrawork") || text.contains("$ulw") || text.contains("$uw") {
        return true;
    }
    // Word-boundary matches
    let re = Regex::new(r"(?i)\b(ultrawork|ulw|uw)\b").unwrap();
    if re.is_match(text) {
        // Check it's not in a code block or URL (already sanitized)
        // Check it's not informational ("what is ultrawork?")
        return !is_informational_context(text, "ultrawork");
    }
    // Aliases with context
    let alias_re = Regex::new(r"(?i)\b(don't stop|must complete|keep going|until done)\b").unwrap();
    if alias_re.is_match(text) {
        // These are general phrases — need activation intent
        return has_activation_intent(text, "ultrawork");
    }
    false
}
```

### 7.2 Full Keyword Match Table

| # | Keyword | Primary Match | Aliases (word-boundary) | Requires Explicit | Activation Verbs |
|---|---------|--------------|------------------------|-------------------|-----------------|
| 1 | ultrawork | `$ultrawork`, `$ulw`, `$uw` | `ultrawork`, `ulw`, `uw` | No | — |
| 2 | ultrawork (NL) | — | `don't stop`, `must complete`, `keep going`, `until done` | Yes | `run`, `start`, `use`, `enable` |
| 3 | ultragoal | `$ultragoal` | `ultragoal` | Yes | `run`, `start`, `use` |
| 4 | ultraqa | `$ultraqa` | `ultraqa`, `qa cycle` | No | — |
| 5 | ralplan | `$ralplan` | `ralplan`, `consensus plan` | Yes | `run`, `start`, `use` |
| 6 | deep-interview | `$deep-interview` | `ouroboros`, `deep interview` | No | — |
| 7 | deep-interview (NL) | — | `interview me`, `gather requirements`, `ask me questions` | Yes | `run`, `start`, `use` |
| 8 | ultrathink | `$ultrathink` | `ultrathink`, `ultra think` | No | — |
| 9 | ultrathink (NL) | — | `think hard`, `think deeply`, `think carefully`, `think step by step` | Yes | `run`, `start`, `use`, `please` |
| 10 | deepsearch | `$deepsearch` | `deepsearch`, `deep search` | No | — |
| 11 | deepsearch (NL) | — | `search the codebase`, `find in codebase`, `thorough search` | Yes | `run`, `start`, `use` |
| 12 | tdd | `$tdd` | `tdd`, `test driven`, `test-driven` | No | — |
| 13 | tdd (NL) | — | `test first`, `red green`, `write test first` | Yes | `run`, `start`, `use` |
| 14 | code-review | `$code-review` | `code review`, `review code` | No | — |
| 15 | code-review (NL) | — | `review this`, `review my changes`, `check my code` | Yes | `run`, `start`, `use` |
| 16 | security-review | `$security-review` | `security review`, `review security` | No | — |
| 17 | security-review (NL) | — | `audit security`, `check vulnerabilities`, `security audit` | Yes | `run`, `start`, `use` |
| 18 | analyze | `$analyze` | `analyze`, `analyse` | No | — |
| 19 | analyze (NL) | — | `deep analyze`, `deep analysis`, `deepanalyze` | Yes | `run`, `start`, `use` |
| 20 | wiki | `$wiki` | `wiki`, `wiki this` | No | — |
| 21 | wiki (NL) | — | `look up docs`, `find documentation`, `check docs` | Yes | `run`, `start`, `use` |
| 22 | cancel | `canceljcode` | `stopjcode` | No | — |
| 23 | ai-slop-cleaner | — | compound only | No | — |

### 7.3 Multilingual Detection Patterns

```rust
// Search triggers (64 total)
const SEARCH_PATTERN_EN: &[&str] = &[
    "search", "find", "locate", "lookup", "look up", "explore",
    "discover", "scan", "grep", "query", "browse", "detect",
    "trace", "seek", "track", "pinpoint", "hunt",
];
const SEARCH_PATTERN_KO: &[&str] = &[
    "검색", "찾아", "탐색", "조회", "스캔", "서치", "뒤져",
    "찾기", "어디", "추적", "탐지", "찾아봐", "찾아내", "보여줘", "목록",
];
const SEARCH_PATTERN_JA: &[&str] = &[
    "検索", "探して", "見つけて", "サーチ", "探索", "スキャン",
    "どこ", "発見", "捜索", "見つけ出す", "一覧",
];
const SEARCH_PATTERN_ZH: &[&str] = &[
    "搜索", "查找", "寻找", "查询", "检索", "定位", "扫描",
    "发现", "在哪里", "找出来", "列出",
];
const SEARCH_PATTERN_VI: &[&str] = &[
    "tìm kiếm", "tra cứu", "định vị", "quét", "phát hiện",
    "truy tìm", "tìm ra", "ở đâu", "liệt kê",
];

// Analyze triggers (64 total)
const ANALYZE_PATTERN_EN: &[&str] = &[
    "analyze", "analyse", "investigate", "examine", "research",
    "study", "deep dive", "inspect", "audit", "evaluate", "assess",
    "review", "diagnose", "scrutinize", "dissect", "debug",
    "comprehend", "interpret", "breakdown", "understand",
];
const ANALYZE_PATTERN_KO: &[&str] = &[
    "분석", "조사", "파악", "연구", "검토", "진단", "이해",
    "설명", "원인", "이유", "뜯어봐", "따져봐", "평가", "해석",
    "디버깅", "디버그", "어떻게", "왜", "살펴",
];
const ANALYZE_PATTERN_JA: &[&str] = &[
    "分析", "調査", "解析", "検討", "研究", "診断", "理解",
    "説明", "検証", "精査", "究明", "デバッグ", "なぜ", "どう", "仕組み",
];
const ANALYZE_PATTERN_ZH: &[&str] = &[
    "分析", "调查", "检查", "剖析", "深入", "诊断", "解释",
    "调试", "为什么", "原理", "搞清楚", "弄明白",
];
const ANALYZE_PATTERN_VI: &[&str] = &[
    "phân tích", "điều tra", "nghiên cứu", "kiểm tra", "xem xét",
    "chẩn đoán", "giải thích", "tìm hiểu", "gỡ lỗi", "tại sao",
];
```

### 7.4 Compound Keyword Detection (ai-slop-cleaner)

```rust
// AI Slop Cleaner requires BOTH an action word AND a smell word
const SLOP_ACTION_WORDS: &[&str] = &[
    "clean", "fix", "refactor", "improve", "remove", "simplify",
    "cleanup", "clean up", "tidy", "polish",
];
const SLOP_SMELL_WORDS: &[&str] = &[
    "slop", "ai slop", "ai-generated", "low quality", "verbose",
    "redundant", "over-engineered", "obvious comments",
    "unnecessary", "boilerplate", "copy-paste",
];

fn match_ai_slop_cleaner(text: &str) -> bool {
    let has_action = SLOP_ACTION_WORDS.iter().any(|w| text.to_lowercase().contains(w));
    let has_smell = SLOP_SMELL_WORDS.iter().any(|w| text.to_lowercase().contains(w));
    has_action && has_smell
}
```

### 7.5 False Positive Prevention

```rust
// System echo stripping — prevent self-reinforcing loops
fn strip_system_echoes(text: &str) -> String {
    // Remove previously injected mode banners
    // Pattern: [SYSTEM DIRECTIVE: JCODE - MODE_NAME ...]
    let re = Regex::new(r"\[SYSTEM DIRECTIVE: JCODE[^\]]*\]").unwrap();
    re.replace_all(text, "").to_string()
}

// Quoted span exclusion
fn is_within_quoted_span(text: &str, pos: usize) -> bool {
    // Check if position is inside "..." or '...' or `...`
    let before = &text[..pos];
    let double_quotes = before.matches('"').count();
    let single_quotes = before.matches('\'').count();
    let backticks = before.matches('`').count();
    double_quotes % 2 == 1 || single_quotes % 2 == 1 || backticks % 2 == 1
}

// Informational context detection
fn is_informational_context(text: &str, keyword: &str) -> bool {
    let lower = text.to_lowercase();
    // Question patterns
    let question_patterns = [
        format!("what is {}", keyword),
        format!("what's {}", keyword),
        format!("how does {} work", keyword),
        format!("explain {}", keyword),
        format!("tell me about {}", keyword),
        format!("what does {} do", keyword),
        format!("{} là gì", keyword),      // Vietnamese
        format!("{}이 뭐야", keyword),      // Korean
        format!("{}とは", keyword),          // Japanese
        format!("{}是什么", keyword),        // Chinese
    ];
    question_patterns.iter().any(|p| lower.contains(p))
}

// Diagnostic context detection
fn is_diagnostic_context(text: &str, keyword: &str) -> bool {
    let lower = text.to_lowercase();
    let patterns = [
        format!("{} keeps", keyword),
        format!("{} is broken", keyword),
        format!("{} failed", keyword),
        format!("{} not working", keyword),
        format!("{} keeps looping", keyword),
        format!("{} error", keyword),
    ];
    patterns.iter().any(|p| lower.contains(p))
}

// Review seed context — prevent re-triggering from echoed review text
fn is_review_seed_context(text: &str) -> bool {
    let patterns = [
        "CRITICAL:", "WARNING:", "INFO:",
        "## Finding", "### Severity",
        "Review complete", "Security audit",
    ];
    patterns.iter().any(|p| text.contains(p))
}
```

---

## Part 8: Detailed Prompt Injections Per Keyword

### 8.1 Ultrawork — Per-Model Prompts

**Claude (default):**
```
<keyword-mode skill="ultrawork" priority="10">
**ULTRAWORK MODE ENABLED!**

You are in ULTRAWORK mode. Execute with maximum parallelism and efficiency.

## Rules:
1. Decompose the task into independent subtasks
2. Spawn up to {max_concurrency} concurrent sub-agents using `subagent` tool with `run_in_background=true`
3. Each sub-agent works independently on its assigned subtask
4. Use `task_create`/`task_update` to track progress
5. Monitor sub-agents and aggregate results when complete
6. If a subtask fails, retry or redistribute the work
7. Do NOT stop until all subtasks are complete

## Current State:
- Iteration: {iteration}/{max_iterations}
- Subtasks completed: {completed}/{total}
- Active agents: {active_count}

Output <ultrawork-complete/> when ALL subtasks are finished.
</keyword-mode>
```

**GPT variant:**
```
<keyword-mode skill="ultrawork" priority="10">
ULTRAWORK MODE ACTIVE.

Instructions:
- Break this task into parallelizable subtasks
- For each subtask, call subagent tool with run_in_background=true
- Maximum concurrent agents: {max_concurrency}
- Track each subtask with task_create tool
- When all agents finish, collect and merge results
- Do not stop prematurely

Status: {iteration}/{max_iterations} iterations, {completed}/{total} done
</keyword-mode>
```

**Gemini variant:**
```
<keyword-mode skill="ultrawork" priority="10">
ULTRAWORK MODE. Maximum parallelism.

1. Split task into {max_concurrency} parallel pieces
2. Spawn sub-agents (background=true)
3. Collect results
4. Merge and verify

Progress: {completed}/{total}
</keyword-mode>
```

### 8.2 Deep-Interview — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="deep-interview" priority="8">
**DEEP INTERVIEW MODE ENABLED!**

Before implementing ANYTHING, you MUST gather requirements through structured questioning.

## Rules:
1. Analyze the user's request for ambiguities and missing details
2. Ask clarifying questions (max 5 per round)
3. Categorize questions: scope, constraints, design preferences, edge cases
4. After each round, re-evaluate ambiguity score (0-100)
5. Continue until ambiguity score ≤ 20 OR max {max_rounds} rounds reached
6. Summarize confirmed requirements before proceeding

## Question Categories:
- **Scope**: What exactly is included/excluded?
- **Constraints**: Performance, security, compatibility requirements?
- **Design**: Architecture preferences, patterns to follow?
- **Edge Cases**: What should happen in error scenarios?
- **Testing**: How will we verify this works?

## Current State:
- Round: {round}/{max_rounds}
- Ambiguity score: {ambiguity_score}/100
- Questions asked: {questions_count}
- Confirmed requirements: {confirmed_count}

Output <interview-complete/> when ambiguity ≤ 20 and requirements are confirmed.
</keyword-mode>
```

### 8.3 TDD — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="tdd" priority="7">
**TDD MODE ENABLED!**

You MUST follow Test-Driven Development strictly.

## Rules:
1. Write the test FIRST — before any implementation code
2. Run the test — it MUST fail (red)
3. Write the MINIMUM implementation to make the test pass
4. Run the test — it MUST pass (green)
5. Refactor if needed (keep tests green)
6. Repeat for next test case

## Current State:
- Phase: {phase} (WriteTest | VerifyFail | Implement | VerifyPass | Refactor)
- Tests written: {tests_written}
- Tests passed: {tests_passed}
- Current test: {current_test}

## Commands:
- Run tests: `cargo test` (or project-specific test command)
- Run single test: `cargo test {test_name}`

Output <tdd-complete/> when all planned tests pass.
</keyword-mode>
```

### 8.4 Ralplan — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="ralplan" priority="11">
**RALPLAN MODE ENABLED!**

You MUST create a consensus-approved plan before implementing ANYTHING.

## Rules:
1. Draft a detailed implementation plan
2. Self-review the plan for gaps, risks, and missing steps
3. Identify potential failure modes and mitigation strategies
4. Revise the plan until it is comprehensive and solid
5. Present the final plan for user approval
6. ONLY implement after the plan is approved

## Plan Structure:
- **Goal**: What we're building and why
- **Steps**: Ordered list of implementation steps
- **Dependencies**: What each step depends on
- **Risks**: Potential issues and mitigations
- **Verification**: How we'll verify each step works

## Current State:
- Revision: {revision}/{max_revisions}
- Previous feedback: {feedback_summary}
- Status: {status} (Drafting | Reviewing | Revising | Approved)

## Adversarial Self-Review:
After drafting, ask yourself:
- What could go wrong?
- What am I missing?
- Are there simpler approaches?
- What are the edge cases?

Output <ralplan-approved/> when the plan is solid and ready for implementation.
</keyword-mode>
```

### 8.5 Ultragoal — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="ultragoal" priority="10">
**ULTRAGOAL MODE ENABLED!**

You are working toward a durable goal with token budget enforcement.

## Goal:
{goal_description}

## Success Criteria:
{criteria_list}

## Rules:
1. Work systematically toward each success criterion
2. Track progress after each significant step
3. Do NOT exceed the token budget
4. If blocked, document the blocker and try alternative approaches
5. Update criteria status as you progress

## Current State:
- Progress: {completed}/{total} criteria met
- Token budget: {remaining}/{total} remaining
- Status: {status} (Active | Complete | Blocked)

{progress_details}

Output <goal-complete/> when ALL success criteria are met.
Output <goal-blocked reason="..."/> if unable to proceed.
</keyword-mode>
```

### 8.6 Ultraqa — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="ultraqa" priority="8">
**ULTRAQA MODE ENABLED!**

Continuous QA cycling until all tests pass.

## Rules:
1. Implement the requested change
2. Run the full test suite
3. If ANY test fails:
   a. Analyze the failure
   b. Fix the issue
   c. Re-run tests
4. If all tests pass:
   a. Run additional checks (clippy, format, lint)
   b. Fix any warnings
5. Repeat until clean

## Current State:
- Cycle: {cycle}/{max_cycles}
- Last run: {passed} passed, {failed} failed
- Status: {status} (Implementing | Testing | Fixing | Passed)

{failure_details}

## Commands:
- Run tests: `cargo test`
- Run clippy: `cargo clippy`
- Run format: `cargo fmt`

Output <qa-passed/> when ALL tests pass AND checks are clean.
Output <qa-failed reason="..."/> if max cycles reached without passing.
</keyword-mode>
```

### 8.7 Security Review — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="security-review" priority="6">
**SECURITY REVIEW MODE ENABLED!**

Perform a thorough security audit of the code changes.

## Checklist:
1. **OWASP Top 10**:
   - [ ] Injection (SQL, Command, LDAP)
   - [ ] Broken Authentication
   - [ ] Sensitive Data Exposure
   - [ ] XML External Entities (XXE)
   - [ ] Broken Access Control
   - [ ] Security Misconfiguration
   - [ ] Cross-Site Scripting (XSS)
   - [ ] Insecure Deserialization
   - [ ] Using Components with Known Vulnerabilities
   - [ ] Insufficient Logging & Monitoring

2. **Secrets Detection**:
   - [ ] Hardcoded passwords/API keys
   - [ ] Credentials in config files
   - [ ] Secrets in logs/error messages

3. **Input Validation**:
   - [ ] All user inputs sanitized
   - [ ] Buffer overflow protection
   - [ ] Path traversal prevention

4. **Authentication/Authorization**:
   - [ ] Proper session management
   - [ ] Token validation
   - [ ] Permission checks

## Output Format:
For each finding:
- **Severity**: Critical | High | Medium | Low | Info
- **Category**: (from checklist above)
- **File:Line**: Location
- **Description**: What's wrong
- **Fix**: How to fix it

Output <security-review-complete/> when audit is finished.
</keyword-mode>
```

### 8.8 Code Review — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="code-review" priority="6">
**CODE REVIEW MODE ENABLED!**

Review the recent code changes thoroughly.

## Review Dimensions:
1. **Correctness**: Logic errors, edge cases, off-by-one
2. **Readability**: Clear naming, appropriate comments, structure
3. **Performance**: Unnecessary allocations, O(n²) patterns, missing caching
4. **Maintainability**: DRY, separation of concerns, testability
5. **Error Handling**: Proper error propagation, no swallowed errors
6. **Safety**: Unsafe blocks justified, no UB, proper lifetimes

## Output Format:
For each finding:
- **Severity**: Critical | Warning | Nit
- **Category**: (from dimensions above)
- **File:Line**: Location
- **Comment**: What's wrong and how to fix it

## Rules:
- Be specific — cite exact lines
- Be constructive — suggest fixes, not just problems
- Be thorough — check every changed file
- Prioritize critical issues over nits

Output <review-complete/> when all files are reviewed.
</keyword-mode>
```

### 8.9 Ultrathink — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="ultrathink" priority="7">
**ULTRATHINK MODE ENABLED!**

Think deeply and thoroughly before acting.

## Rules:
1. Think step-by-step through the problem
2. Consider ALL edge cases before implementing
3. Evaluate multiple approaches and their trade-offs
4. Verify your reasoning at each step
5. Consider security, performance, and maintainability implications
6. Do NOT rush — take the time to think it through properly

## Thinking Framework:
1. **Understand**: What exactly is being asked?
2. **Explore**: What are the possible approaches?
3. **Evaluate**: Which approach is best and why?
4. **Plan**: What are the exact steps?
5. **Verify**: Does this plan handle all cases?

Think silently, then act with confidence.
</keyword-mode>
```

### 8.10 Deepsearch — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="deepsearch" priority="7">
**DEEPSEARCH MODE ENABLED!**

Perform a thorough codebase search before answering.

## Search Strategy:
1. **Text search**: Use `grep` for exact string matches
2. **Pattern search**: Use `glob` for file patterns
3. **Symbol search**: Use `lsp` for definitions and references
4. **Structure search**: Use `ls` and `read` to understand directory layout

## Rules:
1. Search from multiple angles — don't rely on a single grep
2. Follow references — if you find something, trace its usage
3. Check related files — imports, tests, docs
4. Build a mental map of where things live
5. Report findings with exact file:line references

## Search Target:
{search_query}

Output <search-complete/> when thorough search is done.
Report: files found, key locations, relevant code snippets.
</keyword-mode>
```

### 8.11 Analyze — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="analyze" priority="6">
**ANALYZE MODE ENABLED!**

Perform deep, structured analysis.

## Analysis Framework:
1. **Gather**: Collect all relevant information
2. **Structure**: Organize findings into categories
3. **Analyze**: Identify patterns, issues, and insights
4. **Synthesize**: Draw conclusions from the analysis
5. **Recommend**: Suggest actionable next steps

## Output Structure:
- **Summary**: One-paragraph overview
- **Findings**: Detailed list with evidence
- **Patterns**: Recurring themes or issues
- **Recommendations**: Prioritized action items
- **Appendix**: Supporting data/evidence

## Analysis Subject:
{analysis_subject}

Be thorough, evidence-based, and actionable.
</keyword-mode>
```

### 8.12 Wiki — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="wiki" priority="5">
**WIKI MODE ENABLED!**

Look up documentation and provide clear answers.

## Search Order:
1. Local docs (README, docs/, CHANGELOG, inline comments)
2. Source code (actual implementation is truth)
3. Web search (if local info insufficient)

## Rules:
1. Always cite sources (file paths, URLs)
2. Distinguish between documented behavior and observed behavior
3. Note version-specific information
4. If docs are outdated, flag it

## Query:
{wiki_query}

Provide a clear, cited answer.
</keyword-mode>
```

### 8.13 AI Slop Cleaner — Per-Model Prompts

**Claude:**
```
<keyword-mode skill="ai-slop-cleaner" priority="5">
**AI SLOP CLEANER MODE ENABLED!**

Detect and fix AI-generated low-quality code patterns.

## Slop Patterns to Detect:
1. **Obvious comments**: `// increment counter` above `counter += 1`
2. **Over-engineering**: Unnecessary abstractions for simple problems
3. **Verbose names**: `calculateTheTotalSumOfAllItems()` → `total()`
4. **Redundant checks**: `if x != null && x != undefined` (in Rust: `if let Some(x)`)
5. **Copy-paste patterns**: Same logic repeated in multiple places
6. **Excessive error handling**: `.unwrap()` replaced with 10-line match blocks for infallible ops
7. **Boilerplate**: Auto-generated-looking code with no real logic
8. **Defensive programming**: Checking things that can't fail

## Rules:
1. Maintain functionality — don't break working code
2. Improve clarity — shorter, clearer, more idiomatic
3. Use Rust idioms — `if let`, `?`, iterators, `impl`
4. Remove noise — comments that explain the obvious
5. Simplify — fewer lines, same behavior

## Files to Clean:
{file_list}

Output <slop-cleanup-complete/> when all files are cleaned.
Report: patterns found, fixes applied, lines saved.
</keyword-mode>
```

### 8.14 Cancel — No Prompt Injection

Cancel does NOT inject a prompt. It:
1. Clears all active mode states in `.jcode/state/modes.toml`
2. Cancels running background tasks for the session
3. Shows toast: "All modes cancelled"
4. Returns to normal mode

---

## Part 9: Workflow State Machines

### 9.1 Ultrawork State Machine

```
                    ┌─────────────┐
                    │   Initial   │
                    └──────┬──────┘
                           │ detect keyword
                           ▼
                    ┌─────────────┐
                    │  Analyzing  │ ← decompose task into subtasks
                    └──────┬──────┘
                           │ subtasks ready
                           ▼
                    ┌─────────────┐
              ┌────→│  Spawning   │ ← spawn sub-agents (up to max_concurrency)
              │     └──────┬──────┘
              │            │ agents spawned
              │            ▼
              │     ┌─────────────┐
              │     │   Running   │ ← agents working in parallel
              │     └──────┬──────┘
              │            │
              │      ┌─────┴─────┐
              │      │           │
              │      ▼           ▼
              │ ┌────────┐ ┌─────────┐
              │ │Completed│ │  Failed │
              │ └────┬───┘ └────┬────┘
              │      │          │
              │      │          │ retry?
              │      │          ├──────→ Spawning (retry)
              │      │          │
              │      │          └──────→ Failed (max retries)
              │      │
              │      ▼
              │ ┌─────────────┐
              │ │  All Done?  │
              │ └──────┬──────┘
              │        │ yes
              │        ▼
              │ ┌─────────────┐
              └─│ Aggregating │ ← merge results
                └──────┬──────┘
                       │
                       ▼
                ┌─────────────┐
                │  Complete   │
                └─────────────┘
```

**Transitions:**
- `Initial → Analyzing`: keyword detected, task decomposed
- `Analyzing → Spawning`: subtasks identified
- `Spawning → Running`: agents spawned
- `Running → Completed`: agent finishes successfully
- `Running → Failed`: agent fails
- `Failed → Spawning`: retry (if under max retries)
- `Completed → Aggregating`: all agents done
- `Aggregating → Complete`: results merged

**State data:**
```rust
struct UltraworkState {
    subtasks: Vec<Subtask>,
    active_agents: Vec<AgentHandle>,
    completed_count: usize,
    failed_count: usize,
    retry_count: usize,
    max_retries: usize,      // default 3
    max_concurrency: usize,  // default 4
    iteration: usize,
    max_iterations: usize,   // default 10
}
```

### 9.2 Deep-Interview State Machine

```
                    ┌─────────────┐
                    │   Initial   │
                    └──────┬──────┘
                           │ detect keyword
                           ▼
                    ┌─────────────┐
                    │  Analyzing  │ ← identify ambiguities
                    └──────┬──────┘
                           │ ambiguities found
                           ▼
                    ┌─────────────┐
              ┌────→│ Questioning │ ← ask clarifying questions
              │     └──────┬──────┘
              │            │ answers received
              │            ▼
              │     ┌─────────────┐
              │     │  Scoring    │ ← calculate ambiguity score
              │     └──────┬──────┘
              │            │
              │      ┌─────┴─────┐
              │      │           │
              │      ▼           ▼
              │ ┌────────┐ ┌─────────┐
              │ │  ≤ 20  │ │  > 20   │
              │ └────┬───┘ └────┬────┘
              │      │          │
              │      │          │ max rounds?
              │      │          ├──────→ Questioning (next round)
              │      │          │
              │      │          └──────→ Proceeding (force continue)
              │      │
              │      ▼
              │ ┌─────────────┐
              │ │ Summarizing │ ← summarize requirements
              │ └──────┬──────┘
              │        │
              │        ▼
              │ ┌─────────────┐
              └─│   Complete  │
                └─────────────┘
```

**State data:**
```rust
struct DeepInterviewState {
    round: usize,
    max_rounds: usize,        // default 3
    questions: Vec<Question>,
    answers: Vec<Answer>,
    ambiguity_score: usize,   // 0-100
    threshold: usize,         // default 20
    requirements: Option<String>,
}
```

### 9.3 TDD State Machine

```
                ┌─────────────┐
                │   Initial   │
                └──────┬──────┘
                       │ detect keyword
                       ▼
                ┌─────────────┐
          ┌────→│ WriteTest   │ ← write test case
          │     └──────┬──────┘
          │            │ test written
          │            ▼
          │     ┌─────────────┐
          │     │ VerifyFail  │ ← run test, expect FAIL
          │     └──────┬──────┘
          │            │
          │      ┌─────┴─────┐
          │      │           │
          │      ▼           ▼
          │ ┌────────┐ ┌─────────┐
          │ │ Failed │ │ Passed  │ ← unexpected!
          │ └────┬───┘ └────┬────┘
          │      │          │
          │      │          └──→ Fix test (should fail)
          │      ▼
          │ ┌─────────────┐
          │ │ Implement   │ ← write minimal code
          │ └──────┬──────┘
          │        │ code written
          │        ▼
          │ ┌─────────────┐
          │ │ VerifyPass  │ ← run test, expect PASS
          │ └──────┬──────┘
          │        │
          │  ┌─────┴─────┐
          │  │           │
          │  ▼           ▼
          │ ┌────────┐ ┌─────────┐
          │ │ Passed │ │ Failed  │
          │ └────┬───┘ └────┬────┘
          │      │          │
          │      │          └──→ Implement (fix)
          │      ▼
          │ ┌─────────────┐
          │ │  Refactor?  │
          │ └──────┬──────┘
          │        │
          │  ┌─────┴─────┐
          │  │           │
          │  ▼           ▼
          │ ┌────────┐ ┌─────────┐
          │ │  Yes   │ │   No    │
          │ └────┬───┘ └────┬────┘
          │      │          │
          │      ▼          │
          │ ┌──────────┐    │
          │ │ Refactor │    │
          │ └────┬─────┘    │
          │      │          │
          └──────┘          ▼
                     ┌─────────────┐
                     │  Complete   │
                     └─────────────┘
```

**State data:**
```rust
struct TddState {
    phase: TddPhase,
    tests: Vec<TestCase>,
    current_test: Option<String>,
    cycle: usize,
    max_cycles: usize,  // default 20
}

enum TddPhase {
    WriteTest,
    VerifyFail,
    Implement,
    VerifyPass,
    Refactor,
    Complete,
}
```

### 9.4 Ralplan State Machine

```
                ┌─────────────┐
                │   Initial   │
                └──────┬──────┘
                       │ detect keyword
                       ▼
                ┌─────────────┐
          ┌────→│  Drafting   │ ← write plan
          │     └──────┬──────┘
          │            │ draft ready
          │            ▼
          │     ┌─────────────┐
          │     │  Reviewing  │ ← self-review for gaps
          │     └──────┬──────┘
          │            │
          │      ┌─────┴─────┐
          │      │           │
          │      ▼           ▼
          │ ┌────────┐ ┌─────────┐
          │ │ Solid  │ │ Gaps    │
          │ └────┬───┘ └────┬────┘
          │      │          │
          │      │          ▼
          │      │   ┌─────────────┐
          │      │   │  Revising   │ ← fix gaps
          │      │   └──────┬──────┘
          │      │          │
          │      └──────────┘ (loop back to Reviewing)
          │
          ▼
   ┌─────────────┐
   │  Presenting │ ← show plan to user
   └──────┬──────┘
          │
    ┌─────┴─────┐
    │           │
    ▼           ▼
 ┌────────┐ ┌─────────┐
 │Approve │ │ Reject  │
 └────┬───┘ └────┬────┘
      │          │
      ▼          ▼
 ┌─────────┐ ┌──────────┐
 │Complete │ │ Revise   │──→ Drafting
 └─────────┘ └──────────┘
```

**State data:**
```rust
struct RalplanState {
    plan: String,
    revision: usize,
    max_revisions: usize,  // default 5
    reviews: Vec<SelfReview>,
    gaps: Vec<String>,
    status: RalplanStatus,
}

enum RalplanStatus {
    Drafting,
    Reviewing,
    Revising,
    Presenting,
    Approved,
    Rejected,
}
```

### 9.5 Ultraqa State Machine

```
                ┌─────────────┐
                │   Initial   │
                └──────┬──────┘
                       │ detect keyword
                       ▼
                ┌─────────────┐
          ┌────→│ Implementing│ ← write/change code
          │     └──────┬──────┘
          │            │ code ready
          │            ▼
          │     ┌─────────────┐
          │     │  Testing    │ ← run cargo test
          │     └──────┬──────┘
          │            │
          │      ┌─────┴─────┐
          │      │           │
          │      ▼           ▼
          │ ┌────────┐ ┌─────────┐
          │ │  Pass  │ │  Fail   │
          │ └────┬───┘ └────┬────┘
          │      │          │
          │      │          ▼
          │      │   ┌─────────────┐
          │      │   │  Fixing     │ ← analyze + fix failures
          │      │   └──────┬──────┘
          │      │          │
          │      └──────────┘ (loop back to Testing)
          │
          ▼
   ┌─────────────┐
   │  Checks     │ ← clippy, format, lint
   └──────┬──────┘
          │
    ┌─────┴─────┐
    │           │
    ▼           ▼
 ┌────────┐ ┌─────────┐
 │ Clean  │ │ Warning │
 └────┬───┘ └────┬────┘
      │          │
      │          ▼
      │   ┌──────────┐
      │   │ Fix      │──→ Checks
      │   └──────────┘
      ▼
 ┌─────────────┐
 │  Complete   │
 └─────────────┘
```

---

## Part 10: Integration with Existing jcode Tools

### 10.1 Tool Usage Per Workflow

| Workflow | Tools Used | How |
|----------|-----------|-----|
| **ultrawork** | `subagent`, `task_create`, `task_update`, `task_list` | Spawn parallel agents, track subtasks |
| **ultragoal** | `initiative`, `todo` | Create goal, track progress |
| **ultraqa** | `bash` (cargo test), `edit`, `read` | Run tests, fix code |
| **ralplan** | `read`, `write`, `glob`, `grep` | Analyze codebase, draft plan |
| **deep-interview** | (prompt-only) | Ask questions, no tool calls |
| **tdd** | `write`, `bash` (cargo test), `edit` | Write test, run, implement |
| **code-review** | `subagent`, `read`, `diff`, `grep` | Spawn reviewer, read changes |
| **security-review** | `subagent`, `read`, `grep`, `glob` | Spawn security agent, scan code |
| **ultrathink** | (prompt-only) | Think deeply, no tool calls |
| **deepsearch** | `grep`, `glob`, `lsp`, `read`, `agentgrep` | Multi-strategy search |
| **analyze** | `read`, `grep`, `glob`, `lsp` | Gather context, analyze |
| **wiki** | `read`, `websearch`, `webfetch` | Local + web docs |
| **ai-slop-cleaner** | `read`, `edit`, `grep` | Find + fix patterns |
| **cancel** | (state-only) | Clear state, cancel tasks |

### 10.2 Sub-Agent Prompt Templates

**Ultrawork sub-agent prompt:**
```
You are a sub-agent in ULTRAWORK mode. Your task:
{subtask_description}

Rules:
- Work independently on this subtask ONLY
- Do NOT spawn sub-agents yourself
- Report completion with <subtask-complete result="..."/>
- If stuck, report <subtask-blocked reason="..."/>
- Use available tools: read, write, edit, bash, grep, glob
```

**Code-review sub-agent prompt:**
```
You are a code reviewer. Review these changes:
{diff_content}

Check for: correctness, readability, performance, error handling.
Format: severity | file:line | comment
```

**Security-review sub-agent prompt:**
```
You are a security auditor. Audit this code:
{file_list}

Check OWASP Top 10, secrets, input validation, auth patterns.
Format: severity | category | file:line | description | fix
```

---

## Part 11: Error Handling Per Workflow

| Workflow | Error Scenario | Handling |
|----------|---------------|----------|
| **ultrawork** | Sub-agent crashes | Retry up to 3 times, then mark failed |
| **ultrawork** | All agents fail | Report failure, suggest simplification |
| **ultragoal** | Budget exhausted | Stop, report progress, ask user |
| **ultraqa** | Max cycles reached | Report remaining failures, ask user |
| **ralplan** | Max revisions reached | Present best plan, ask user |
| **deep-interview** | Max rounds reached | Proceed with current info, note gaps |
| **tdd** | Can't make test pass | Report stuck point, ask user |
| **code-review** | Sub-agent timeout | Skip file, note in report |
| **security-review** | Sub-agent timeout | Skip file, note in report |
| **deepsearch** | No results found | Expand search, try alternatives |
| **analyze** | Insufficient context | Ask user for more info |
| **wiki** | No docs found | Suggest alternatives |
| **ai-slop-cleaner** | Can't simplify without breaking | Skip pattern, note in report |

---

## Part 12: Edge Cases

### 12.1 Multiple Keywords in One Message

```
User: "run ultrawork and tdd for this feature"
```
**Resolution:** Both activate. Ultrawork (priority 10) > TDD (priority 7). Ultrawork orchestrates, TDD rules apply to each sub-agent.

### 12.2 Keyword in Code Block

```
User: "Here's an example: ```ultrawork mode```"
```
**Resolution:** Sanitizer strips code blocks before matching. No activation.

### 12.3 Keyword in URL

```
User: "Check https://example.com/ultrawork-docs"
```
**Resolution:** Sanitizer strips URLs before matching. No activation.

### 12.4 Informational Query

```
User: "What is ultrawork mode?"
```
**Resolution:** Intent classifier detects question pattern. No activation.

### 12.5 Diagnostic Query

```
User: "ultrawork keeps failing, what's wrong?"
```
**Resolution:** Intent classifier detects diagnostic pattern. No activation.

### 12.6 Cancel During Active Workflow

```
User: (ultrawork running) → "canceljcode"
```
**Resolution:** Cancel clears all state, cancels background tasks, shows toast.

### 12.7 Small Task with Heavy Keyword

```
User: "quick: fix typo with ultrawork"
```
**Resolution:** Escape hatch "quick:" forces small task. Ultrawork suppressed (heavy). Agent fixes typo normally.

### 12.8 Stale Mode State

```
Mode active for > 2 hours without activity
```
**Resolution:** Treat as inactive. Don't inject prompt. Log warning.

### 12.9 Session Restart with Active Modes

```
Mode persisted in .jcode/state/modes.toml, session restarted
```
**Resolution:** Load state on startup. If stale (>2h), deactivate. If fresh, continue.

### 12.10 Conflicting Modes

```
User: "run ultrawork" then "run deep-interview"
```
**Resolution:** Both can coexist. Deep-interview asks questions first, then ultrawork executes.

---

## Part 13: Full File List

### New Files (crates/jcode-keywords/)

| File | Lines (est.) | Purpose |
|------|-------------|---------|
| `Cargo.toml` | 30 | Crate config |
| `src/lib.rs` | 100 | Public API, re-exports |
| `src/registry.rs` | 400 | 50+ keyword entries with full data |
| `src/detector.rs` | 300 | Main detection engine |
| `src/sanitizer.rs` | 250 | 8-stage sanitization pipeline |
| `src/intent.rs` | 200 | Informational/activation/diagnostic |
| `src/task_size.rs` | 150 | Small/medium/large classification |
| `src/conflict.rs` | 100 | Priority resolution |
| `src/state.rs` | 300 | TOML persistence, state ops |
| `src/prompt_builder.rs` | 500 | Per-mode prompt injection (14 modes × per-model) |
| `src/visual.rs` | 50 | Visual effect types |
| `src/workflow/mod.rs` | 150 | WorkflowHandler trait + dispatch |
| `src/workflow/ultrawork.rs` | 400 | Parallel execution workflow |
| `src/workflow/ultragoal.rs` | 300 | Goal tracking workflow |
| `src/workflow/ultraqa.rs` | 300 | QA cycling workflow |
| `src/workflow/ralplan.rs` | 350 | Consensus planning workflow |
| `src/workflow/deep_interview.rs` | 300 | Requirements gathering workflow |
| `src/workflow/tdd.rs` | 300 | Test-driven dev workflow |
| `src/workflow/code_review.rs` | 250 | Code review workflow |
| `src/workflow/security_review.rs` | 250 | Security review workflow |
| `src/workflow/ultrathink.rs` | 100 | Extended thinking workflow |
| `src/workflow/deepsearch.rs` | 200 | Codebase search workflow |
| `src/workflow/analyze.rs` | 150 | Deep analysis workflow |
| `src/workflow/wiki.rs` | 100 | Doc lookup workflow |
| `src/workflow/ai_slop_cleaner.rs` | 200 | Slop cleanup workflow |
| `src/workflow/cancel.rs` | 80 | Cancel workflow |
| `src/tests.rs` | 500 | Comprehensive tests |
| **Total** | **~5,700** | |

### Modified Files

| File | Change | Lines (est.) |
|------|--------|-------------|
| `Cargo.toml` | Add workspace member | 2 |
| `crates/jcode-app-core/src/agent/prompting.rs` | Keyword detection + injection | 80 |
| `crates/jcode-app-core/src/agent/turn_loops.rs` | Pass message to detector | 20 |
| `crates/jcode-tui/src/tui/app/input.rs` | Rainbow + shimmer | 150 |
| `crates/jcode-tui/src/tui/app/state_ui_input_helpers.rs` | Cancel command | 30 |
| `crates/jcode-tui/src/tui/info_widget.rs` | Mode indicator | 50 |
| `crates/jcode-config-types/src/lib.rs` | KeywordsConfig | 30 |
| `crates/jcode-base/src/config.rs` | Load config | 20 |
| **Total** | | **~380** |

### Grand Total: ~6,080 lines of new/modified code
