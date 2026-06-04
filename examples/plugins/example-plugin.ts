/**
 * Example Plugin for jcode
 *
 * This demonstrates the full plugin API including lifecycle hooks,
 * event handlers, tool registration, state management, configuration,
 * persistence, and capability declarations.
 *
 * Plugins run in QuickJS sandboxes with no DOM, no Node.js built-ins,
 * and limited global objects. The runtime injects a `__jcode_pi` global
 * (aliased as `pi` below) that provides all plugin APIs.
 *
 * Plugin lifecycle:
 *   1. Discovery  ─ jcode scans plugin directories / npm cache / config
 *   2. Preflight  ─ static analysis for capability enforcement
 *   3. Load       ─ eval (transpile TS→JS, then QuickJS eval)
 *   4. Activate   ─ handlers are registered into the dispatcher
 *   5. Runtime    ─ events dispatched → plugin handlers invoked
 *   6. Unload     ─ cleanup on session end or plugin disable
 */

// ─── Plugin Identity & Manifest ────────────────────────────────────────────
//
// Every plugin MUST export a default object with identity metadata.
// jcode reads this at load time to register the plugin and wire up
// lifecycle hooks.

type HandlerResult = { action: string; output?: unknown; error?: string };

interface PluginManifest {
  name: string;
  version: string;
  description?: string;
  author?: string;
  capabilities: {
    fs_read?: string[];
    fs_write?: string[];
    network?: string[];
    shell?: boolean;
    register_tools?: boolean;
    read_config?: boolean;
    write_config?: boolean;
    events?: string[];
    llm_access?: boolean;
    session_access?: boolean;
  };
}

const manifest: PluginManifest = {
  name: 'example-plugin',
  version: '1.0.0',
  description: 'Demo plugin showing all jcode plugin API capabilities',
  author: 'jcode team',

  // Declare required capabilities. The runtime checks these against
  // the plugin's static analysis and the user's security policy.
  capabilities: {
    fs_read: ['$HOME/.jcode/data'],
    network: ['api.github.com'],
    register_tools: true,
    read_config: true,
    events: ['TurnStart', 'TurnEnd', 'MessageStart', 'MessageEnd', 'PreToolUse', 'PostToolUse', 'Notification'],
  },
};

// ─── State Management ──────────────────────────────────────────────────────
//
// Module-level variables persist for the plugin's lifetime (from load
// to unload). Use `pi.kv` for durable cross-session persistence.

let turnCount = 0;
let totalToolDuration = 0;
const toolCallHistory: Array<{ name: string; startedAt: number }> = [];

// ─── Event Handlers ────────────────────────────────────────────────────────
//
// Handlers are registered via pi.on(eventName, callback).
// The callback receives an event object with fields specific to the event type.
// Return value (optional) can modify the event's outcome for events that
// support it (e.g. PreToolUse can block or modify input).

function setupHandlers(): void {
  const logger = pi.logger;

  /**
   * TurnStart — fired when a conversation turn begins.
   * Event fields: { session_id, turn_number, messages }
   */
  pi.on('TurnStart', (event: { session_id: string; turn_number: number; messages: unknown }) => {
    turnCount++;
    logger.info(`[example-plugin] Turn #${event.turn_number} started (session: ${event.session_id})`);
  });

  /**
   * TurnEnd — fired when a turn completes.
   * Event fields: { session_id, turn_number, duration_ms }
   */
  pi.on('TurnEnd', (event: { session_id: string; turn_number: number; duration_ms: number }) => {
    logger.info(`[example-plugin] Turn #${event.turn_number} ended (${event.duration_ms}ms)`);
    logger.info(`[example-plugin] Total tools duration this session: ${totalToolDuration}ms`);
  });

  /**
   * MessageStart — fired when the model or user starts a new message.
   * Event fields: { session_id, role }  (role = "user" | "assistant" | "system")
   */
  pi.on('MessageStart', (event: { session_id: string; role: string }) => {
    logger.debug(`[example-plugin] ${event.role} message starting`);
  });

  /**
   * MessageEnd — fired when a message is fully produced.
   * Event fields: { session_id, role, content }
   */
  pi.on('MessageEnd', (event: { session_id: string; role: string; content: string }) => {
    logger.debug(`[example-plugin] ${event.role} message ended (${event.content.length} chars)`);
  });

  /**
   * PreToolUse — fired BEFORE a tool is executed.
   * Event fields: { tool_name, tool_input, session_id }
   *
   * Can return a modified input or block the tool entirely.
   * Return { action: 'block', output: 'reason' } to prevent execution.
   * Return { action: 'continue', output: { modified_input } } to modify args.
   */
  pi.on('PreToolUse', (event: { tool_name: string; tool_input: Record<string, unknown>; session_id: string }) => {
    logger.info(`[example-plugin] Tool about to run: ${event.tool_name}`);
    toolCallHistory.push({ name: event.tool_name, startedAt: Date.now() });

    // Example: block dangerous-sounding tools
    if (event.tool_name === 'dangerous_tool' || event.tool_name === 'rm') {
      return { action: 'block', output: 'Blocked by example-plugin safety policy' };
    }

    // Example: auto-append a flag to Read tool calls
    if (event.tool_name === 'Read') {
      const input = { ...event.tool_input };
      if (!input.limit) {
        input.limit = 200;
      }
      return { action: 'continue', output: { modified_input: input } };
    }

    return { action: 'continue' };
  });

  /**
   * PostToolUse — fired AFTER a tool returns successfully.
   * Event fields: { tool_name, tool_input, tool_output, duration_ms, success, session_id }
   *
   * Can optionally modify the tool output returned to the model.
   */
  pi.on('PostToolUse', (event: {
    tool_name: string;
    tool_input: Record<string, unknown>;
    tool_output: unknown;
    duration_ms: number;
    success: boolean;
    session_id: string;
  }) => {
    totalToolDuration += event.duration_ms;
    logger.info(`[example-plugin] Tool completed: ${event.tool_name} (${event.duration_ms}ms, success=${event.success})`);
  });

  /**
   * Notification — arbitrary notifications from the system.
   * Event fields: { level, message, session_id? }
   * Can suppress or modify the notification.
   */
  pi.on('Notification', (event: { level: string; message: string; session_id?: string }) => {
    logger.debug(`[example-plugin] Notification [${event.level}]: ${event.message}`);
  });
}

// ─── Tool Registration ─────────────────────────────────────────────────────
//
// Custom tools are registered via pi.registerTool(toolDefinition).
// The tool definition must include a name, description, parameter schema,
// and a handler function. The handler runs inside the QuickJS sandbox.

function registerCustomTools(): void {
  pi.registerTool({
    name: 'example_hello',
    description: 'Greet a user by name. Useful for demonstrating plugin tool registration.',
    parameters: {
      type: 'object',
      properties: {
        name: { type: 'string', description: 'The name to greet' },
        title: { type: 'string', description: 'Optional title (Mr., Dr., etc.)', default: '' },
      },
      required: ['name'],
    },
    handler: (params: { name: string; title?: string }) => {
      const prefix = params.title ? `${params.title} ` : '';
      return `Hello, ${prefix}${params.name}! From example-plugin v1.0.0`;
    },
  });

  pi.registerTool({
    name: 'example_counter',
    description: 'Return the current turn counter value maintained by the plugin.',
    parameters: { type: 'object', properties: {} },
    handler: () => {
      return { turnCount, totalToolDuration };
    },
  });

  pi.registerTool({
    name: 'example_echo',
    description: 'Echo back whatever input is provided. Use to verify plugin tool routing.',
    parameters: {
      type: 'object',
      properties: {
        message: { type: 'string', description: 'The message to echo' },
      },
      required: ['message'],
    },
    handler: (params: { message: string }) => {
      return `Echo: ${params.message}`;
    },
  });
}

// ─── Configuration & Persistence ──────────────────────────────────────────
//
// pi.getConfig(key) reads plugin config from the global jcode config.
// pi.kv.get(key) / pi.kv.set(key, value) provides durable storage
//   that persists across sessions (backed by the runtime).

function loadConfig(): Record<string, unknown> {
  const logLevel = pi.getConfig('example-plugin.logLevel') || 'info';
  const maxHistory = pi.getConfig('example-plugin.maxHistory') || 100;

  // Restore persistent state
  const saved = pi.kv.get('example-plugin.toolHistory');
  const history = saved ? JSON.parse(saved) : [];

  return { logLevel, maxHistory, history };
}

function saveState(): void {
  pi.kv.set('example-plugin.toolHistory', JSON.stringify(toolCallHistory.slice(-100)));
}

// ─── Plugin Load ───────────────────────────────────────────────────────────
//
// The module scope runs at load time. This is where you wire up
// everything: register tools, bind event handlers, read config.
// pi.logger is available immediately.

const config = loadConfig();
const logger = pi.logger;

logger.info(`[example-plugin] Loading v${manifest.version} (config: ${JSON.stringify(config)})`);

// Register event handlers for lifecycle hooks.
setupHandlers();

// Register custom tools that the model can invoke.
registerCustomTools();

// Log our identity.
logger.info(`[example-plugin] Registered plugin: ${pi.name}@${pi.version}`);

// ─── Graceful Shutdown (optional) ──────────────────────────────────────────
//
// pi.on('SessionEnd') or pi.on('Stop') can be used for cleanup.

pi.on('SessionEnd', () => {
  saveState();
  logger.info('[example-plugin] Plugin shutting down, state persisted');
});

// ─── Default Export ────────────────────────────────────────────────────────
//
// jcode loads the module and reads the default export for plugin metadata.
// The actual work (handlers, tools, config) happens at module scope above,
// but the export ensures the runtime can identify the plugin.

export default {
  name: manifest.name,
  version: manifest.version,
  description: manifest.description,
  author: manifest.author,
  manifest,
};
