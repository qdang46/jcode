use jcode_plugin_core::PluginError;
use std::collections::HashMap;
use std::sync::Mutex;
use swc_common::{FileName, GLOBALS, Globals, Mark, SourceMap, sync::Lrc};
use swc_ecma_ast::EsVersion;
use swc_ecma_codegen::to_code_default;
use swc_ecma_parser::{Syntax, TsSyntax, parse_file_as_program};
use swc_ecma_transforms_base::{fixer::fixer, resolver};
use swc_ecma_transforms_typescript::strip;

/// Transpiler that converts TypeScript source to plain JavaScript using SWC.
///
/// Results are cached by content hash to avoid redundant work when the same
/// snippet is transpiled multiple times.
pub struct Transpiler {
    cache: Mutex<HashMap<u64, String>>,
}

impl Transpiler {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn transpile(&self, code: &str, filename: &str) -> Result<String, PluginError> {
        let hash = seahash::hash(code.as_bytes());

        if let Ok(cache) = self.cache.lock() {
            if let Some(cached) = cache.get(&hash) {
                return Ok(cached.clone());
            }
        }

        if filename.ends_with(".ts") || filename.ends_with(".tsx") {
            let result = self.transpile_inner(code)?;

            if let Ok(mut cache) = self.cache.lock() {
                cache.insert(hash, result.clone());
            }

            Ok(result)
        } else {
            Ok(code.to_string())
        }
    }

    fn transpile_inner(&self, code: &str) -> Result<String, PluginError> {
        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            Lrc::new(FileName::Custom("input.ts".into())),
            code.to_string(),
        );

        let mut recovered_errors = Vec::new();
        let program = parse_file_as_program(
            &fm,
            Syntax::Typescript(TsSyntax {
                tsx: true,
                ..Default::default()
            }),
            EsVersion::latest(),
            None,
            &mut recovered_errors,
        )
        .map_err(|e| PluginError::Transpile(format!("TypeScript parse error: {e:?}")))?;

        let output = GLOBALS.set(&Globals::default(), || {
            let unresolved_mark = Mark::new();
            let top_level_mark = Mark::new();

            let program = program
                .apply(resolver(unresolved_mark, top_level_mark, true))
                .apply(strip(unresolved_mark, top_level_mark))
                .apply(fixer(None));

            to_code_default(cm, None, &program)
        });

        Ok(output)
    }

    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_ts_transpile() {
        let t = Transpiler::new();
        let input = "const x: number = 42;";
        let result = t.transpile(input, "test.ts").unwrap();
        assert!(result.contains("42"), "output should contain the value 42");
        assert!(
            !result.contains(": number"),
            "type annotation should be stripped"
        );
    }

    #[test]
    fn interface_stripping() {
        let t = Transpiler::new();
        let input = r#"
interface User {
    name: string;
    age: number;
}
const u: User = { name: "alice", age: 30 };
"#;
        let result = t.transpile(input, "test.ts").unwrap();
        assert!(
            !result.contains("interface"),
            "interface keyword should be removed"
        );
        assert!(result.contains("alice"), "runtime values preserved");
    }

    #[test]
    fn arrow_function_types() {
        let t = Transpiler::new();
        let input = "const fn = (x: number, y: string): boolean => true;";
        let result = t.transpile(input, "test.ts").unwrap();
        assert!(result.contains("true"), "function body preserved");
        assert!(
            !result.contains(": number"),
            "parameter type should be stripped"
        );
        assert!(
            !result.contains(": string"),
            "parameter type should be stripped"
        );
        assert!(
            !result.contains(": boolean"),
            "return type should be stripped"
        );
    }

    #[test]
    fn import_type_stripping() {
        let t = Transpiler::new();
        let input = r#"import type { Foo } from "./types";
import { Bar } from "./module";
const b = new Bar();
"#;
        let result = t.transpile(input, "test.ts").unwrap();
        assert!(
            !result.contains("Foo"),
            "type-only import should be removed"
        );
        assert!(result.contains("Bar"), "value import should remain");
        assert!(result.contains("module"), "import path preserved");
    }

    #[test]
    fn cache_hit() {
        let t = Transpiler::new();
        let input = "const x: number = 1;";
        let r1 = t.transpile(input, "test.ts").unwrap();
        let r2 = t.transpile(input, "test.ts").unwrap();
        assert_eq!(r1, r2, "cached result should match original");

        let hash = seahash::hash(input.as_bytes());
        assert!(
            t.cache.lock().unwrap().contains_key(&hash),
            "cache should contain the entry"
        );
    }

    #[test]
    fn non_ts_passthrough() {
        let t = Transpiler::new();
        let input = "const x = 42;";
        let result = t.transpile(input, "test.js").unwrap();
        assert_eq!(result, input, "non-TS files should pass through unchanged");
    }
}
