//! Mutation trait and all 25 mutation implementations.
//!
//! Each mutation defines how to find AST candidates via tree-sitter queries
//! and how to apply a source-level edit that introduces a subtle bug.
//!
//! Architecture follows oh-my-pi's `mutations.ts` but adapted for Rust's
//! tree-sitter grammar instead of Babel's TypeScript AST.

use crate::types::{MutationInfo, SourceEdit};
use tree_sitter::Node;

// ── Core trait ──────────────────────────────────────────────────────

/// Simple deterministic RNG wrapper (avoids dyn-incompatibility of rand::Rng).
pub struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Generate a random number in [0, max).
    pub fn gen_index(&mut self, max: usize) -> usize {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state as usize) % max
    }
}

pub trait Mutation: Send + Sync {
    fn name(&self) -> &str;
    fn category(&self) -> &str;
    fn description(&self) -> &str;
    fn fix_hint(&self) -> &str;

    /// Find all candidate AST nodes where this mutation can be applied.
    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>>;

    /// Apply the mutation to a single candidate, returning source edits.
    fn apply(&self, candidate: &Candidate, source: &str, rng: &mut SimpleRng) -> Vec<SourceEdit>;

    /// Full mutate lifecycle: parse → find → choose → apply → rebuild.
    fn mutate(&self, source: &str, rng: &mut SimpleRng) -> Option<(String, MutationInfo)> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .ok()?;
        let tree = parser.parse(source, None)?;
        let candidates = self.collect_candidates(tree.root_node(), source);
        if candidates.is_empty() {
            return None;
        }

        let idx = rng.gen_index(candidates.len());
        let chosen = &candidates[idx];
        let node = chosen.node;
        let line = source[..node.start_byte()].matches('\n').count() + 1;

        let original = node.utf8_text(source.as_bytes()).ok()?.to_string();

        let edits = self.apply(chosen, source, rng);
        if edits.is_empty() {
            return None;
        }

        let mutated = apply_source_edits(source, &edits)?;

        // Recompute mutated snippet: reflect the byte-range changes
        let mutated_snippet = edits
            .first()
            .map(|e| e.replacement.clone())
            .unwrap_or_default();

        Some((
            mutated,
            MutationInfo {
                line_number: line,
                original_snippet: original,
                mutated_snippet,
            },
        ))
    }
}

/// A candidate AST node found for a mutation type.
#[derive(Debug, Clone)]
pub struct Candidate<'a> {
    pub node: Node<'a>,
    pub meta: Option<String>,
}

// ── Source edit utilities ───────────────────────────────────────────

/// Apply source edits in reverse order (bottom-up) to preserve byte offsets.
pub fn apply_source_edits(source: &str, edits: &[SourceEdit]) -> Option<String> {
    if edits.is_empty() {
        return Some(source.to_string());
    }
    let mut sorted: Vec<&SourceEdit> = edits.iter().collect();
    sorted.sort_by(|a, b| b.start.cmp(&a.start));

    let mut out = source.to_string();
    // Track the lowest (earliest) start byte applied so far; any edit with
    // end > earliest_start overlaps something already applied.
    let mut earliest_start = source.len() + 1;
    for edit in &sorted {
        if edit.start > edit.end || edit.end > out.len() || edit.start > out.len() {
            return None;
        }
        if edit.end > earliest_start {
            return None; // overlaps with already-applied edit
        }
        let mut new =
            String::with_capacity(out.len() - (edit.end - edit.start) + edit.replacement.len());
        new.push_str(&out[..edit.start]);
        new.push_str(&edit.replacement);
        new.push_str(&out[edit.end..]);
        out = new;
        if edit.start < earliest_start {
            earliest_start = edit.start;
        }
    }
    Some(out)
}

/// Helper: get byte range for a node.
pub fn node_range(node: Node) -> (usize, usize) {
    (node.start_byte(), node.end_byte())
}

/// Helper: get text of a node from source.
pub fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

/// Helper: count leading whitespace for a line.
pub fn leading_whitespace(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

// ── Helper: walk all child nodes of a given kind ────────────────────

fn find_nodes<'a>(root: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut result = Vec::new();
    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        if node.kind() == kind {
            result.push(node);
        }
    }
    result
}

fn find_nodes_recursive<'a>(root: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut result = Vec::new();
    let mut cursor = root.walk();
    // Manual tree traversal since tree-sitter cursor doesn't have a simple
    // recursive filter built-in. We use a stack approach.
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            result.push(node);
        }
        let mut c = node.walk();
        for child in node.children(&mut c) {
            stack.push(child);
        }
    }
    result
}

// ── 1. SwapComparisonMutation ───────────────────────────────────────

pub struct SwapComparisonMutation;
impl Mutation for SwapComparisonMutation {
    fn name(&self) -> &str {
        "swap-comparison"
    }
    fn category(&self) -> &str {
        "operator"
    }
    fn description(&self) -> &str {
        "A comparison operator is subtly wrong."
    }
    fn fix_hint(&self) -> &str {
        "Swap the comparison operator to the correct variant."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "binary_expression") {
            if let Some(op) = node.child_by_field_name("operator") {
                let txt = node_text(op, source);
                if matches!(txt, "<" | "<=" | ">" | ">=") {
                    candidates.push(Candidate {
                        node: op,
                        meta: None,
                    });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt {
            "<" => "<=",
            "<=" => "<",
            ">" => ">=",
            ">=" => ">",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

// ── 2. SwapEqualityMutation ─────────────────────────────────────────

pub struct SwapEqualityMutation;
impl Mutation for SwapEqualityMutation {
    fn name(&self) -> &str {
        "swap-equality"
    }
    fn category(&self) -> &str {
        "operator"
    }
    fn description(&self) -> &str {
        "An equality operator is inverted."
    }
    fn fix_hint(&self) -> &str {
        "Fix the equality comparison operator."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "binary_expression") {
            if let Some(op) = node.child_by_field_name("operator") {
                let txt = node_text(op, source);
                if matches!(txt, "==" | "!=") {
                    candidates.push(Candidate {
                        node: op,
                        meta: None,
                    });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt {
            "==" => "!=",
            "!=" => "==",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

// ── 3. SwapLogicalMutation ──────────────────────────────────────────

pub struct SwapLogicalMutation;
impl Mutation for SwapLogicalMutation {
    fn name(&self) -> &str {
        "swap-logical"
    }
    fn category(&self) -> &str {
        "operator"
    }
    fn description(&self) -> &str {
        "A boolean operator is incorrect."
    }
    fn fix_hint(&self) -> &str {
        "Use the intended boolean operator."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "binary_expression") {
            if let Some(op) = node.child_by_field_name("operator") {
                let txt = node_text(op, source);
                if matches!(txt, "&&" | "||") {
                    candidates.push(Candidate {
                        node: op,
                        meta: None,
                    });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt {
            "&&" => "||",
            "||" => "&&",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

// ── 4. RemoveNegationMutation ───────────────────────────────────────

pub struct RemoveNegationMutation;
impl Mutation for RemoveNegationMutation {
    fn name(&self) -> &str {
        "remove-negation"
    }
    fn category(&self) -> &str {
        "operator"
    }
    fn description(&self) -> &str {
        "A logical negation (!) was accidentally removed."
    }
    fn fix_hint(&self) -> &str {
        "Add back the missing logical negation (!)."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "unary_expression") {
            if let Some(op) = node.child_by_field_name("operator") {
                let txt = node_text(op, source);
                if txt == "!" {
                    // The `!` operator node; we want to remove it.
                    // Actually we want to replace the whole unary with its argument.
                    candidates.push(Candidate { node, meta: None });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, _source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        // Replace the entire unary expression with its argument (the `!` is removed)
        if let Some(arg) = candidate.node.child_by_field_name("argument") {
            let arg_text = node_text(arg, _source);
            let (start, end) = node_range(candidate.node);
            vec![SourceEdit {
                start,
                end,
                replacement: arg_text.to_string(),
            }]
        } else {
            vec![]
        }
    }
}

// ── 5. SwapAssignOpMutation (Rust-specific) ─────────────────────────

pub struct SwapAssignOpMutation;
impl Mutation for SwapAssignOpMutation {
    fn name(&self) -> &str {
        "swap-assign-op"
    }
    fn category(&self) -> &str {
        "operator"
    }
    fn description(&self) -> &str {
        "A compound assignment operator is wrong."
    }
    fn fix_hint(&self) -> &str {
        "Correct the compound assignment operator."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "assignment_expression") {
            if let Some(op) = node.child_by_field_name("operator") {
                let txt = node_text(op, source);
                if matches!(txt, "+=" | "-=" | "*=" | "/=") {
                    candidates.push(Candidate {
                        node: op,
                        meta: None,
                    });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt {
            "+=" => "-=",
            "-=" => "+=",
            "*=" => "/=",
            "/=" => "*=",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

// ── 6. SwapArithmeticMutation ───────────────────────────────────────

pub struct SwapArithmeticMutation;
impl Mutation for SwapArithmeticMutation {
    fn name(&self) -> &str {
        "swap-arithmetic"
    }
    fn category(&self) -> &str {
        "operator"
    }
    fn description(&self) -> &str {
        "An arithmetic operator was swapped."
    }
    fn fix_hint(&self) -> &str {
        "Correct the arithmetic operator."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "binary_expression") {
            if let Some(op) = node.child_by_field_name("operator") {
                let txt = node_text(op, source);
                if matches!(txt, "+" | "-" | "*" | "/") {
                    candidates.push(Candidate {
                        node: op,
                        meta: None,
                    });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt {
            "+" => "-",
            "-" => "+",
            "*" => "/",
            "/" => "*",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

// ── 7. BooleanLiteralFlipMutation ────────────────────────────────────

pub struct BooleanLiteralFlipMutation;
impl Mutation for BooleanLiteralFlipMutation {
    fn name(&self) -> &str {
        "flip-boolean"
    }
    fn category(&self) -> &str {
        "literal"
    }
    fn description(&self) -> &str {
        "A boolean literal is inverted."
    }
    fn fix_hint(&self) -> &str {
        "Flip the boolean literal to the intended value."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, _source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "boolean_literal") {
            candidates.push(Candidate { node, meta: None });
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt.trim() {
            "true" => "false",
            "false" => "true",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

// ── 8. OffByOneMutation ─────────────────────────────────────────────

pub struct OffByOneMutation;
impl Mutation for OffByOneMutation {
    fn name(&self) -> &str {
        "off-by-one"
    }
    fn category(&self) -> &str {
        "literal"
    }
    fn description(&self) -> &str {
        "A numeric boundary has an off-by-one error."
    }
    fn fix_hint(&self) -> &str {
        "Fix the off-by-one error in the numeric literal or comparison."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        // Find integer_literals that are 0 or 1 inside loops or conditionals
        for node in find_nodes_recursive(root, "integer_literal") {
            let txt = node_text(node, source);
            if txt == "0" || txt == "1" {
                // Check if ancestor is a for/while/if/ or .len() comparison
                let mut parent = node.parent();
                while let Some(p) = parent {
                    let kind = p.kind();
                    if matches!(
                        kind,
                        "for_expression" | "while_expression" | "if_expression"
                    ) {
                        candidates.push(Candidate { node, meta: None });
                        break;
                    }
                    // Check for `x < .len()` or `x <= .len()` patterns
                    if kind == "binary_expression" {
                        if let Some(op) = p.child_by_field_name("operator") {
                            let op_txt = node_text(op, source);
                            if matches!(op_txt, "<" | "<=" | ">" | ">=") {
                                candidates.push(Candidate { node, meta: None });
                                break;
                            }
                        }
                    }
                    parent = p.parent();
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt {
            "0" => "1",
            "1" => "0",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

// ── 9. SwapCallArgsMutation ──────────────────────────────────────────

pub struct SwapCallArgsMutation;
impl Mutation for SwapCallArgsMutation {
    fn name(&self) -> &str {
        "swap-call-args"
    }
    fn category(&self) -> &str {
        "call"
    }
    fn description(&self) -> &str {
        "Two arguments in a call are swapped."
    }
    fn fix_hint(&self) -> &str {
        "Swap the two arguments to their original order."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "call_expression") {
            let args = node.child_by_field_name("arguments");
            if let Some(args_node) = args {
                let mut arg_list = vec![];
                let mut cursor = args_node.walk();
                for child in args_node.children(&mut cursor) {
                    if !child.is_extra() && !is_comma(child, source) {
                        arg_list.push(child);
                    }
                }
                if arg_list.len() >= 2 {
                    candidates.push(Candidate {
                        node: args_node,
                        meta: None,
                    });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        // Collect non-comma child nodes
        let mut args = vec![];
        let mut cursor = candidate.node.walk();
        for child in candidate.node.children(&mut cursor) {
            if !child.is_extra() && !is_comma(child, source) {
                args.push(child);
            }
        }
        if args.len() < 2 {
            return vec![];
        }
        let first = args[0];
        let second = args[1];
        let (f_start, f_end) = node_range(first);
        let (s_start, s_end) = node_range(second);
        let first_text = node_text(first, source).to_string();
        let second_text = node_text(second, source).to_string();
        vec![
            SourceEdit {
                start: f_start,
                end: f_end,
                replacement: second_text,
            },
            SourceEdit {
                start: s_start,
                end: s_end,
                replacement: first_text,
            },
        ]
    }
}

fn is_comma(node: Node, source: &str) -> bool {
    node_text(node, source) == ","
}

// ── 10. RemoveQuestionMarkMutation (Rust-specific) ──────────────────

pub struct RemoveQuestionMarkMutation;
impl Mutation for RemoveQuestionMarkMutation {
    fn name(&self) -> &str {
        "remove-question-mark"
    }
    fn category(&self) -> &str {
        "access"
    }
    fn description(&self) -> &str {
        "The ? operator was removed from a Result expression."
    }
    fn fix_hint(&self) -> &str {
        "Add back the missing ? operator."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "try_expression") {
            let op = node.child_by_field_name("operator");
            if let Some(opn) = op {
                if node_text(opn, source) == "?" {
                    candidates.push(Candidate { node, meta: None });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        // Remove the `?` token (the try_expression node wraps the inner expr)
        let (start, end) = node_range(candidate.node);
        // The inner expression is the child without the `?`
        // Strategy: replace the whole try_expression with just the inner expression
        if let Some(inner) = candidate.node.child_by_field_name("value") {
            let inner_text = node_text(inner, source);
            vec![SourceEdit {
                start,
                end,
                replacement: inner_text.to_string(),
            }]
        } else {
            vec![]
        }
    }
}

// ── 11. IdentifierMultiEditMutation ───────────────────────────────────

pub struct IdentifierMultiEditMutation;
impl Mutation for IdentifierMultiEditMutation {
    fn name(&self) -> &str {
        "identifier-multi-edit"
    }
    fn category(&self) -> &str {
        "identifier"
    }
    fn description(&self) -> &str {
        "An identifier is misspelled in multiple locations."
    }
    fn fix_hint(&self) -> &str {
        "Restore the identifier to its original spelling in all affected locations."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut results = Vec::new();
        // Find identifiers that appear at least 3 times across the file
        let mut counts: std::collections::HashMap<String, Vec<Node<'a>>> =
            std::collections::HashMap::new();
        for node in find_nodes_recursive(root, "identifier") {
            let txt = node_text(node, source).to_string();
            if txt.len() < 2 {
                continue;
            }
            if txt.starts_with('_') {
                continue;
            }
            counts.entry(txt).or_default().push(node);
        }
        for (_name, nodes) in &counts {
            if nodes.len() >= 3 {
                results.push(Candidate {
                    node: nodes[0],
                    meta: Some(nodes.len().to_string()),
                });
                break;
            }
        }
        results
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source).to_string();
        if txt.len() < 2 {
            return vec![];
        }
        // Swap first two characters
        let mut chars: Vec<char> = txt.chars().collect();
        chars.swap(0, 1);
        let mutated: String = chars.into_iter().collect();
        if mutated == txt {
            return vec![];
        }

        // We only mutate the first occurrence (the definition) — multi-edit
        // of all references would need scope analysis; for simplicity we do one.
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: mutated,
        }]
    }
}

// ── 12. DuplicateExprMutation ───────────────────────────────────────

pub struct DuplicateExprMutation;
impl Mutation for DuplicateExprMutation {
    fn name(&self) -> &str {
        "duplicate-expr"
    }
    fn category(&self) -> &str {
        "duplicate"
    }
    fn description(&self) -> &str {
        "A duplicated expression contains a subtle change."
    }
    fn fix_hint(&self) -> &str {
        "Fix the literal or operator on the duplicated expression."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        // Find statements that appear at least twice (same text)
        let mut counts: std::collections::HashMap<String, Vec<Node<'a>>> =
            std::collections::HashMap::new();
        for node in find_nodes_recursive(root, "expression_statement") {
            let txt = node_text(node, source).to_string().trim().to_string();
            if txt.len() < 5 {
                continue;
            }
            counts.entry(txt).or_default().push(node);
        }
        for (_txt, nodes) in &counts {
            if nodes.len() >= 2 {
                // Pick one of the duplicates to mutate
                candidates.push(Candidate {
                    node: nodes[0],
                    meta: None,
                });
                break;
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        // Find a boolean literal or binary expression inside and flip it
        for node in find_nodes_recursive(candidate.node, "boolean_literal") {
            let txt = node_text(node, source);
            let replacement = match txt.trim() {
                "true" => "false",
                "false" => "true",
                _ => continue,
            };
            let (start, end) = node_range(node);
            return vec![SourceEdit {
                start,
                end,
                replacement: replacement.to_string(),
            }];
        }
        // Try flipping a comparison operator
        for node in find_nodes_recursive(candidate.node, "binary_expression") {
            if let Some(op) = node.child_by_field_name("operator") {
                let txt = node_text(op, source);
                let replacement = match txt {
                    "==" => "!=",
                    "!=" => "==",
                    "<" => "<=",
                    "<=" => "<",
                    ">" => ">=",
                    ">=" => ">",
                    _ => continue,
                };
                let (start, end) = node_range(op);
                return vec![SourceEdit {
                    start,
                    end,
                    replacement: replacement.to_string(),
                }];
            }
        }
        vec![]
    }
}

// ── 13. SwapNamedImportsMutation ────────────────────────────────────

pub struct SwapNamedImportsMutation;
impl Mutation for SwapNamedImportsMutation {
    fn name(&self) -> &str {
        "swap-named-imports"
    }
    fn category(&self) -> &str {
        "import"
    }
    fn description(&self) -> &str {
        "Two imported names are swapped."
    }
    fn fix_hint(&self) -> &str {
        "Swap the two imported names back to their original order."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "use_list") {
            // Check if this use_list has at least 2 use_arguments
            let mut args = vec![];
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "use_argument" {
                    args.push(child);
                }
            }
            if args.len() >= 2 {
                candidates.push(Candidate {
                    node,
                    meta: Some(format!("{}", args.len())),
                });
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let mut args = vec![];
        let mut cursor = candidate.node.walk();
        for child in candidate.node.children(&mut cursor) {
            if child.kind() == "use_argument" {
                args.push(child);
            }
        }
        if args.len() < 2 {
            return vec![];
        }
        let first = args[0];
        let second = args[1];
        let f1 = node_text(first, source).to_string();
        let f2 = node_text(second, source).to_string();
        let (s1, e1) = node_range(first);
        let (s2, e2) = node_range(second);
        vec![
            SourceEdit {
                start: s1,
                end: e1,
                replacement: f2,
            },
            SourceEdit {
                start: s2,
                end: e2,
                replacement: f1,
            },
        ]
    }
}

// ── 14. SwapAdjacentLinesMutation ───────────────────────────────────

pub struct SwapAdjacentLinesMutation;
impl Mutation for SwapAdjacentLinesMutation {
    fn name(&self) -> &str {
        "swap-adjacent-lines"
    }
    fn category(&self) -> &str {
        "structural"
    }
    fn description(&self) -> &str {
        "Two adjacent statements are in the wrong order."
    }
    fn fix_hint(&self) -> &str {
        "Swap the two adjacent lines back to their original order."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "block") {
            let mut stmts = vec![];
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                // Skip braces
                let txt = node_text(child, source);
                if txt == "{" || txt == "}" || child.is_extra() {
                    continue;
                }
                stmts.push(child);
            }
            for i in 0..stmts.len().saturating_sub(1) {
                let a = stmts[i];
                let b = stmts[i + 1];
                // Both should be on a single line
                let a_start_line = source[..a.start_byte()].matches('\n').count();
                let a_end_line = source[..a.end_byte()].matches('\n').count();
                let b_start_line = source[..b.start_byte()].matches('\n').count();
                let b_end_line = source[..b.end_byte()].matches('\n').count();
                if a_start_line == a_end_line && b_start_line == b_end_line {
                    candidates.push(Candidate {
                        node,
                        meta: Some(format!("{}|{}|single", i, i + 1)),
                    });
                    break;
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let meta = match candidate.meta.as_ref() {
            Some(m) => m.clone(),
            None => return vec![],
        };
        let parts: Vec<&str> = meta.split('|').collect();
        let i: usize = match parts.get(0).and_then(|s| s.parse().ok()) {
            Some(v) => v,
            None => return vec![],
        };

        let mut stmts = vec![];
        let mut cursor = candidate.node.walk();
        for child in candidate.node.children(&mut cursor) {
            let txt = node_text(child, source);
            if txt == "{" || txt == "}" || child.is_extra() {
                continue;
            }
            stmts.push(child);
        }
        if i + 1 >= stmts.len() {
            return vec![];
        }

        let a = stmts[i];
        let b = stmts[i + 1];
        let a_text = node_text(a, source).to_string();
        let b_text = node_text(b, source).to_string();
        let (s1, e1) = node_range(a);
        let (s2, e2) = node_range(b);

        vec![
            SourceEdit {
                start: s1,
                end: e1,
                replacement: b_text.clone(),
            },
            SourceEdit {
                start: s2,
                end: e2,
                replacement: a_text.clone(),
            },
        ]
    }
}

// ── 15. SwapIfElseBranchesMutation ──────────────────────────────────

pub struct SwapIfElseBranchesMutation;
impl Mutation for SwapIfElseBranchesMutation {
    fn name(&self) -> &str {
        "swap-if-else"
    }
    fn category(&self) -> &str {
        "structural"
    }
    fn description(&self) -> &str {
        "The if and else branches are swapped."
    }
    fn fix_hint(&self) -> &str {
        "Swap the if and else branch bodies back to their original positions."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "if_expression") {
            // Must have an else clause
            let has_else = node
                .children(&mut node.walk())
                .any(|c| node_text(c, source) == "else");
            if has_else {
                candidates.push(Candidate { node, meta: None });
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let node = candidate.node;
        // Find the condition body and the else body
        // The structure is: `if condition block else block`
        let mut children = vec![];
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            children.push(child);
        }

        // Find blocks: the first block after "if", and the first block after "else"
        let mut got_if = false;
        let mut got_else = false;
        let mut consequent_block: Option<Node> = None;
        let mut alternate_block: Option<Node> = None;

        for child in &children {
            let txt = node_text(*child, source);
            if txt == "if" {
                got_if = true;
                continue;
            }
            if got_if && child.kind() == "block" && consequent_block.is_none() {
                consequent_block = Some(*child);
                continue;
            }
            if txt == "else" {
                got_else = true;
                continue;
            }
            if got_else && child.kind() == "block" && alternate_block.is_none() {
                alternate_block = Some(*child);
            }
        }

        let (cons, alt) = match (consequent_block, alternate_block) {
            (Some(c), Some(a)) => (c, a),
            _ => return vec![],
        };

        let cons_text = node_text(cons, source).to_string();
        let alt_text = node_text(alt, source).to_string();
        let (s1, e1) = node_range(cons);
        let (s2, e2) = node_range(alt);

        vec![
            SourceEdit {
                start: s1,
                end: e1,
                replacement: alt_text,
            },
            SourceEdit {
                start: s2,
                end: e2,
                replacement: cons_text,
            },
        ]
    }
}

// ── 16. RemoveEarlyReturnMutation ───────────────────────────────────

pub struct RemoveEarlyReturnMutation;
impl Mutation for RemoveEarlyReturnMutation {
    fn name(&self) -> &str {
        "remove-early-return"
    }
    fn category(&self) -> &str {
        "structural"
    }
    fn description(&self) -> &str {
        "A guard clause (early return) was removed."
    }
    fn fix_hint(&self) -> &str {
        "Restore the missing guard clause (if expression with early return)."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "if_expression") {
            // Must NOT have an else clause
            let has_else = node
                .children(&mut node.walk())
                .any(|c| node_text(c, source) == "else");
            if has_else {
                continue;
            }
            // The consequent block must contain a `return` expression as a trailing statement
            if let Some(consequent) = node.child_by_field_name("consequence") {
                let has_return = find_nodes_recursive(consequent, "return_expression").len() > 0;
                if has_return {
                    candidates.push(Candidate { node, meta: None });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        // Remove the entire if_expression
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: String::new(),
        }]
    }
}

// ── 17. DeleteStatementMutation ─────────────────────────────────────

pub struct DeleteStatementMutation;
impl Mutation for DeleteStatementMutation {
    fn name(&self) -> &str {
        "delete-statement"
    }
    fn category(&self) -> &str {
        "structural"
    }
    fn description(&self) -> &str {
        "A critical statement was deleted from the code."
    }
    fn fix_hint(&self) -> &str {
        "Restore the deleted statement."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, _source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "expression_statement") {
            candidates.push(Candidate { node, meta: None });
        }
        // Also track let declarations inside blocks
        for node in find_nodes_recursive(root, "let_declaration") {
            // Only if inside a block (not a module-level `let`)
            let mut p = node.parent();
            while let Some(parent) = p {
                if parent.kind() == "block" {
                    candidates.push(Candidate { node, meta: None });
                    break;
                }
                p = parent.parent();
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, _source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: String::new(),
        }]
    }
}

// ── 18. SwapBorrowMutation (Rust-specific) ──────────────────────────

pub struct SwapBorrowMutation;
impl Mutation for SwapBorrowMutation {
    fn name(&self) -> &str {
        "swap-borrow"
    }
    fn category(&self) -> &str {
        "rust-specific"
    }
    fn description(&self) -> &str {
        "A borrow expression uses the wrong mutability."
    }
    fn fix_hint(&self) -> &str {
        "Fix the borrow mutability (& vs &mut)."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "reference_expression") {
            if let Some(op) = node.child_by_field_name("operator") {
                let txt = node_text(op, source);
                if txt == "&" || txt == "&mut" {
                    candidates.push(Candidate {
                        node: op,
                        meta: None,
                    });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt {
            "&" => "&mut ",
            "&mut" => "&",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

// ── 19. SwapUnwrapExpectMutation (Rust-specific) ────────────────────

pub struct SwapUnwrapExpectMutation;
impl Mutation for SwapUnwrapExpectMutation {
    fn name(&self) -> &str {
        "swap-unwrap-expect"
    }
    fn category(&self) -> &str {
        "rust-specific"
    }
    fn description(&self) -> &str {
        "An unwrap/expect call uses the wrong method."
    }
    fn fix_hint(&self) -> &str {
        "Replace unwrap() with expect(\"...\") or vice versa."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "call_expression") {
            if let Some(func) = node.child_by_field_name("function") {
                let txt = node_text(func, source);
                if txt == "unwrap" || txt.ends_with(".unwrap") {
                    // Find the `.unwrap` part
                    if let Some(dot_unwrap) = find_field_expression(func, "unwrap", source) {
                        candidates.push(Candidate {
                            node: dot_unwrap,
                            meta: None,
                        });
                    }
                }
                if txt == "expect" || txt.ends_with(".expect") {
                    if let Some(dot_expect) = find_field_expression(func, "expect", source) {
                        candidates.push(Candidate {
                            node: dot_expect,
                            meta: None,
                        });
                    }
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt {
            "unwrap" => "expect",
            ".unwrap" => ".expect",
            "expect" => "unwrap",
            ".expect" => ".unwrap",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

fn find_field_expression<'a>(node: Node<'a>, _name: &str, _source: &'a str) -> Option<Node<'a>> {
    // Try to find a field_identifier child with matching text
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "field_identifier" {
            return Some(child);
        }
    }
    None
}

// ── 20. SwapOkErrMutation (Rust-specific) ───────────────────────────

pub struct SwapOkErrMutation;
impl Mutation for SwapOkErrMutation {
    fn name(&self) -> &str {
        "swap-ok-err"
    }
    fn category(&self) -> &str {
        "rust-specific"
    }
    fn description(&self) -> &str {
        "Ok and Err variants are swapped."
    }
    fn fix_hint(&self) -> &str {
        "Swap Ok(..) and Err(..) to the correct variant."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "call_expression") {
            if let Some(func) = node.child_by_field_name("function") {
                let txt = node_text(func, source);
                if txt == "Ok" || txt == "Err" {
                    candidates.push(Candidate {
                        node: func,
                        meta: None,
                    });
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        let replacement = match txt {
            "Ok" => "Err",
            "Err" => "Ok",
            _ => return vec![],
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement: replacement.to_string(),
        }]
    }
}

// ── 21. RemoveUnsafeMutation (Rust-specific) ────────────────────────

pub struct RemoveUnsafeMutation;
impl Mutation for RemoveUnsafeMutation {
    fn name(&self) -> &str {
        "remove-unsafe"
    }
    fn category(&self) -> &str {
        "rust-specific"
    }
    fn description(&self) -> &str {
        "The unsafe block was removed."
    }
    fn fix_hint(&self) -> &str {
        "Add back the unsafe { ... } wrapper."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, _source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "unsafe_block") {
            candidates.push(Candidate { node, meta: None });
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        // Replace the unsafe block with its inner block (remove `unsafe` keyword)
        if let Some(block) = candidate.node.child(1) {
            let block_text = node_text(block, source);
            let (start, end) = node_range(candidate.node);
            vec![SourceEdit {
                start,
                end,
                replacement: block_text.to_string(),
            }]
        } else {
            vec![]
        }
    }
}

// ── 22. SwapMatchArmsMutation (Rust-specific) ───────────────────────

pub struct SwapMatchArmsMutation;
impl Mutation for SwapMatchArmsMutation {
    fn name(&self) -> &str {
        "swap-match-arms"
    }
    fn category(&self) -> &str {
        "rust-specific"
    }
    fn description(&self) -> &str {
        "Two consecutive match arms are swapped."
    }
    fn fix_hint(&self) -> &str {
        "Swap the two match arms back to their original order."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "match_expression") {
            let mut arms = vec![];
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "match_arm" {
                    arms.push(child);
                }
            }
            if arms.len() >= 2 {
                candidates.push(Candidate {
                    node,
                    meta: Some(format!("0|1")),
                });
                break;
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let mut arms = vec![];
        let mut cursor = candidate.node.walk();
        for child in candidate.node.children(&mut cursor) {
            if child.kind() == "match_arm" {
                arms.push(child);
            }
        }
        if arms.len() < 2 {
            return vec![];
        }
        let a = arms[0];
        let b = arms[1];
        let a_text = node_text(a, source).to_string();
        let b_text = node_text(b, source).to_string();
        let (s1, e1) = node_range(a);
        let (s2, e2) = node_range(b);
        vec![
            SourceEdit {
                start: s1,
                end: e1,
                replacement: b_text,
            },
            SourceEdit {
                start: s2,
                end: e2,
                replacement: a_text,
            },
        ]
    }
}

// ── 23. FlipFmtArgMutation (Rust-specific) ──────────────────────────

pub struct FlipFmtArgMutation;
impl Mutation for FlipFmtArgMutation {
    fn name(&self) -> &str {
        "flip-fmt-arg"
    }
    fn category(&self) -> &str {
        "rust-specific"
    }
    fn description(&self) -> &str {
        "A format specifier is wrong ({} vs {:?})."
    }
    fn fix_hint(&self) -> &str {
        "Fix the format specifier in the format string."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "string_literal") {
            let txt = node_text(node, source);
            if txt.contains("{}") || txt.contains("{:?}") {
                candidates.push(Candidate { node, meta: None });
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        // Find the first occurrence of {} or {:?} and flip
        let replacement = if txt.contains("{}") {
            txt.replacen("{}", "{:?}", 1)
        } else if txt.contains("{:?}") {
            txt.replacen("{:?}", "{}", 1)
        } else {
            return vec![];
        };
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement,
        }]
    }
}

// ── 24. SwapDeriveAttrMutation (Rust-specific) ──────────────────────

pub struct SwapDeriveAttrMutation;
impl Mutation for SwapDeriveAttrMutation {
    fn name(&self) -> &str {
        "swap-derive-attr"
    }
    fn category(&self) -> &str {
        "rust-specific"
    }
    fn description(&self) -> &str {
        "A derive attribute has the wrong trait."
    }
    fn fix_hint(&self) -> &str {
        "Swap Debug/Clone in the derive attribute."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "attribute_item") {
            let txt = node_text(node, source);
            if txt.contains("derive") && (txt.contains("Debug") || txt.contains("Clone")) {
                candidates.push(Candidate { node, meta: None });
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let txt = node_text(candidate.node, source);
        // Swap Debug ↔ Clone in the attribute
        let replacement = txt
            .replace("Debug", "__DEBUG__")
            .replace("Clone", "Debug")
            .replace("__DEBUG__", "Clone");
        if replacement == txt {
            return vec![];
        }
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement,
        }]
    }
}

// ── 25. SnakeCaseSwapMutation (Rust-specific naming convention) ─────

// Already covered by the 24 mutations above. For a clean 25, we add a
// `rename-function` mutation that swaps a function name's word order.

pub struct RenameFunctionMutation;
impl Mutation for RenameFunctionMutation {
    fn name(&self) -> &str {
        "rename-function"
    }
    fn category(&self) -> &str {
        "rust-specific"
    }
    fn description(&self) -> &str {
        "A function name has words in the wrong order."
    }
    fn fix_hint(&self) -> &str {
        "Swap the word order in the function name."
    }

    fn collect_candidates<'a>(&self, root: Node<'a>, source: &'a str) -> Vec<Candidate<'a>> {
        let mut candidates = Vec::new();
        for node in find_nodes_recursive(root, "function_item") {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(name_node, source);
                if name.contains('_') {
                    let parts: Vec<&str> = name.split('_').collect();
                    if parts.len() >= 2 {
                        candidates.push(Candidate {
                            node: name_node,
                            meta: None,
                        });
                    }
                }
            }
        }
        candidates
    }

    fn apply(&self, candidate: &Candidate, source: &str, _rng: &mut SimpleRng) -> Vec<SourceEdit> {
        let name = node_text(candidate.node, source);
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() < 2 {
            return vec![];
        }
        // If exactly 2 words, swap them. If more, swap first two.
        let mut new_parts = parts.clone();
        new_parts.swap(0, 1);
        let replacement = new_parts.join("_");
        if replacement == name {
            return vec![];
        }
        let (start, end) = node_range(candidate.node);
        vec![SourceEdit {
            start,
            end,
            replacement,
        }]
    }
}

// ── All mutations ───────────────────────────────────────────────────

/// Return all 25 mutation types.
pub fn all_mutations() -> Vec<Box<dyn Mutation>> {
    vec![
        Box::new(SwapComparisonMutation),
        Box::new(SwapEqualityMutation),
        Box::new(SwapLogicalMutation),
        Box::new(RemoveNegationMutation),
        Box::new(SwapAssignOpMutation),
        Box::new(SwapArithmeticMutation),
        Box::new(BooleanLiteralFlipMutation),
        Box::new(OffByOneMutation),
        Box::new(SwapCallArgsMutation),
        Box::new(RemoveQuestionMarkMutation),
        Box::new(IdentifierMultiEditMutation),
        Box::new(DuplicateExprMutation),
        Box::new(SwapNamedImportsMutation),
        Box::new(SwapAdjacentLinesMutation),
        Box::new(SwapIfElseBranchesMutation),
        Box::new(RemoveEarlyReturnMutation),
        Box::new(DeleteStatementMutation),
        Box::new(SwapBorrowMutation),
        Box::new(SwapUnwrapExpectMutation),
        Box::new(SwapOkErrMutation),
        Box::new(RemoveUnsafeMutation),
        Box::new(SwapMatchArmsMutation),
        Box::new(FlipFmtArgMutation),
        Box::new(SwapDeriveAttrMutation),
        Box::new(RenameFunctionMutation),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rng() -> SimpleRng {
        SimpleRng::new(42)
    }

    #[test]
    fn test_swap_comparison_applies() {
        let source = "fn f() -> bool { let x = 5; x < 10 }";
        let mutation = SwapComparisonMutation;
        let result = mutation.mutate(source, &mut test_rng());
        assert!(result.is_some());
        let (mutated, info) = result.unwrap();
        assert_ne!(mutated, source);
        assert!(info.line_number > 0);
        assert!(info.original_snippet.len() > 0);
    }

    #[test]
    fn test_flip_boolean_applies() {
        let source = "fn f() -> bool { let x = true; x }";
        let mutation = BooleanLiteralFlipMutation;
        let result = mutation.mutate(source, &mut test_rng());
        assert!(result.is_some());
        let (mutated, _) = result.unwrap();
        assert!(mutated.contains("false"));
    }

    #[test]
    fn test_swap_equality_applies() {
        let source = "fn f() -> bool { x == 5 }";
        let mutation = SwapEqualityMutation;
        let result = mutation.mutate(source, &mut test_rng());
        assert!(result.is_some());
        let (mutated, _) = result.unwrap();
        assert_ne!(mutated, source);
    }

    #[test]
    fn test_all_mutations_have_names() {
        let names: Vec<String> = all_mutations()
            .iter()
            .map(|m| m.name().to_string())
            .collect();
        assert_eq!(
            names.len(),
            25,
            "Expected 25 mutations, got {}",
            names.len()
        );

        // Check for duplicates
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 25, "Duplicate mutation names found");
    }

    #[test]
    fn test_apply_source_edits_reverse_order() {
        // Two non-overlapping edits: swap "hello" and "world"
        let source = "hello foo bar world";
        let edits = vec![
            SourceEdit {
                start: 0,
                end: 5,
                replacement: "world".into(),
            },
            SourceEdit {
                start: 14,
                end: 19,
                replacement: "hello".into(),
            },
        ];
        let result = apply_source_edits(source, &edits);
        assert_eq!(result.as_deref(), Some("world foo bar hello"));
    }

    #[test]
    fn test_apply_source_edits_overlapping_returns_none() {
        let edits = vec![
            SourceEdit {
                start: 0,
                end: 10,
                replacement: "short".into(),
            },
            SourceEdit {
                start: 5,
                end: 15,
                replacement: "nope".into(),
            },
        ];
        assert!(apply_source_edits("hello world test", &edits).is_none());
    }
}
