use jcode_plugin_core::PluginError;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Transpiler {
    cache: Mutex<HashMap<u64, String>>,
}

impl Transpiler {
    pub fn new() -> Self {
        Self { cache: Mutex::new(HashMap::new()) }
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

    fn transpile_inner(&self, _code: &str) -> Result<String, PluginError> {
        Ok(format!(
            r#""use strict";
(function(exports) {{
{}
}})(typeof exports !== 'undefined' ? exports : {{}});
"#,
            _code
        ))
    }

    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}
