// Hello Plugin — real working example for jcode's plugin system.
//
// This file is:
//   1. Discovered by jcode-plugin-runtime::PluginLoader::scan_directory
//      (looks for *.ts files in ~/.jcode/plugins/ or configured plugin_dirs)
//   2. Transpiled by jcode-plugin-runtime::Transpiler (SWC, TypeScript -> JS)
//   3. Evaluated by jcode-plugin-runtime::SandboxContext (QuickJS)
//   4. The `jcode` object is injected by jcode-plugin-runtime::api::PluginApiBindings
//
// Available APIs on `jcode` (as of jcode v0.29):
//   jcode.on(event, handler)              — register event handler
//   jcode.registerTool(toolDef)           — register a tool the LLM can call
//   jcode.logger.{info,warn,error,debug}  — log to jcode's tracing system
//   jcode.kv.{get,set}                    — per-plugin key/value store
//   jcode.sleep(ms)                       — sleep (capped at 5s)
//   jcode.uuid()                          — generate a UUID
//   jcode.cwd                             — current working directory (string)

// Subscribe to SessionStart event.
jcode.on("SessionStart", function(event) {
    jcode.logger.info("hello-plugin: session starting, id=" + event.sessionId);
});

// Subscribe to PreToolUse event.
jcode.on("PreToolUse", function(event) {
    jcode.logger.info("hello-plugin: tool about to be called: " + event.toolName);
});

// Register a tool the LLM could (eventually) call.
jcode.registerTool({
    name: "hello",
    description: "Say hello and return a greeting",
});

// Use the per-plugin key/value store.
jcode.kv.set("hello-plugin:loaded-at", "test");

// Generate a UUID.
var instanceUuid = jcode.uuid();
jcode.logger.info("hello-plugin: instance uuid = " + instanceUuid);

// Final log line — proves the plugin's top-level code ran to completion.
jcode.logger.info("hello-plugin: registered 2 handlers + 1 tool stub, kv set done");
