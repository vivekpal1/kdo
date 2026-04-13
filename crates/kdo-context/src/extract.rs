//! Tree-sitter based signature extraction.
//!
//! Extracts public API signatures (functions, structs, enums, traits, classes,
//! interfaces, type aliases) WITHOUT function bodies.

use kdo_core::Language;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::debug;

/// Kind of extracted signature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignatureKind {
    /// Function or method.
    Function,
    /// Struct or class.
    Struct,
    /// Enum definition.
    Enum,
    /// Trait or interface.
    Trait,
    /// Type alias.
    TypeAlias,
    /// Constant or static.
    Constant,
    /// Impl block header.
    Impl,
}

/// A single extracted signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    /// The kind of signature.
    pub kind: SignatureKind,
    /// The signature text (no body).
    pub text: String,
    /// Source file path.
    pub file: String,
    /// Line number in the source file.
    pub line: usize,
}

/// Extract all public API signatures from a source file.
///
/// Uses tree-sitter for parsing; falls back to line-based extraction on error.
pub fn extract_signatures(file_path: &Path, language: &Language) -> Vec<Signature> {
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let file_str = file_path.to_string_lossy().to_string();

    match language {
        Language::Rust | Language::Anchor => extract_rust_signatures(&content, &file_str),
        Language::TypeScript | Language::JavaScript => extract_ts_signatures(&content, &file_str),
        Language::Python => extract_python_signatures(&content, &file_str),
    }
}

fn extract_rust_signatures(source: &str, file: &str) -> Vec<Signature> {
    let mut parser = tree_sitter::Parser::new();
    let ts_lang = tree_sitter_rust::language();
    if parser.set_language(&ts_lang).is_err() {
        return fallback_rust_extract(source, file);
    }

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return fallback_rust_extract(source, file),
    };

    let mut sigs = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();

    for node in root.children(&mut cursor) {
        match node.kind() {
            "function_item" => {
                if let Some(sig) = extract_rust_fn_sig(source, &node, file) {
                    sigs.push(sig);
                }
            }
            "struct_item" => {
                if let Some(sig) = extract_rust_type_sig(source, &node, file, SignatureKind::Struct)
                {
                    sigs.push(sig);
                }
            }
            "enum_item" => {
                if let Some(sig) = extract_rust_type_sig(source, &node, file, SignatureKind::Enum) {
                    sigs.push(sig);
                }
            }
            "trait_item" => {
                if let Some(sig) = extract_rust_type_sig(source, &node, file, SignatureKind::Trait)
                {
                    sigs.push(sig);
                }
            }
            "impl_item" => {
                if let Some(sig) = extract_rust_impl_sig(source, &node, file) {
                    sigs.push(sig);
                }
            }
            "type_item" => {
                let text = node_text(source, &node);
                sigs.push(Signature {
                    kind: SignatureKind::TypeAlias,
                    text,
                    file: file.to_string(),
                    line: node.start_position().row + 1,
                });
            }
            "const_item" | "static_item" => {
                if is_pub(source, &node) {
                    let text = node_text(source, &node);
                    sigs.push(Signature {
                        kind: SignatureKind::Constant,
                        text,
                        file: file.to_string(),
                        line: node.start_position().row + 1,
                    });
                }
            }
            _ => {}
        }
    }

    debug!(file = file, count = sigs.len(), "extracted Rust signatures");
    sigs
}

fn extract_rust_fn_sig(
    source: &str,
    node: &tree_sitter::Node<'_>,
    file: &str,
) -> Option<Signature> {
    // Only extract pub functions
    if !is_pub(source, node) {
        return None;
    }

    // Get text up to the body (block)
    let mut sig_end = node.end_byte();
    let mut child_cursor = node.walk();
    for child in node.children(&mut child_cursor) {
        if child.kind() == "block" {
            sig_end = child.start_byte();
            break;
        }
    }

    let text = source[node.start_byte()..sig_end].trim().to_string();
    Some(Signature {
        kind: SignatureKind::Function,
        text,
        file: file.to_string(),
        line: node.start_position().row + 1,
    })
}

fn extract_rust_type_sig(
    source: &str,
    node: &tree_sitter::Node<'_>,
    file: &str,
    kind: SignatureKind,
) -> Option<Signature> {
    if !is_pub(source, node) {
        return None;
    }

    // For structs/enums, get the header before the body
    let sig_end = node.end_byte();
    // For structs with fields, include the whole thing but truncate bodies of methods
    let text = source[node.start_byte()..sig_end].trim().to_string();

    Some(Signature {
        kind,
        text,
        file: file.to_string(),
        line: node.start_position().row + 1,
    })
}

fn extract_rust_impl_sig(
    source: &str,
    node: &tree_sitter::Node<'_>,
    file: &str,
) -> Option<Signature> {
    // Get just the impl header, not the body
    let mut sig_end = node.end_byte();
    let mut child_cursor = node.walk();
    for child in node.children(&mut child_cursor) {
        if child.kind() == "declaration_list" {
            sig_end = child.start_byte();
            break;
        }
    }

    let text = source[node.start_byte()..sig_end].trim().to_string();
    Some(Signature {
        kind: SignatureKind::Impl,
        text,
        file: file.to_string(),
        line: node.start_position().row + 1,
    })
}

fn extract_ts_signatures(source: &str, file: &str) -> Vec<Signature> {
    let mut parser = tree_sitter::Parser::new();
    let ts_lang = tree_sitter_typescript::language_typescript();
    if parser.set_language(&ts_lang).is_err() {
        return fallback_ts_extract(source, file);
    }

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return fallback_ts_extract(source, file),
    };

    let mut sigs = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();

    for node in root.children(&mut cursor) {
        if node.kind() != "export_statement" {
            continue;
        }
        // Look at the exported declaration
        let mut child_cursor = node.walk();
        for child in node.children(&mut child_cursor) {
            match child.kind() {
                "function_declaration" | "function_signature" => {
                    let mut sig_end = child.end_byte();
                    let mut gc = child.walk();
                    for grandchild in child.children(&mut gc) {
                        if grandchild.kind() == "statement_block" {
                            sig_end = grandchild.start_byte();
                            break;
                        }
                    }
                    let text = format!("export {}", source[child.start_byte()..sig_end].trim());
                    sigs.push(Signature {
                        kind: SignatureKind::Function,
                        text,
                        file: file.to_string(),
                        line: child.start_position().row + 1,
                    });
                }
                "class_declaration" => {
                    let mut sig_end = child.end_byte();
                    let mut gc = child.walk();
                    for grandchild in child.children(&mut gc) {
                        if grandchild.kind() == "class_body" {
                            sig_end = grandchild.start_byte();
                            break;
                        }
                    }
                    let text = format!("export {}", source[child.start_byte()..sig_end].trim());
                    sigs.push(Signature {
                        kind: SignatureKind::Struct,
                        text,
                        file: file.to_string(),
                        line: child.start_position().row + 1,
                    });
                }
                "interface_declaration" => {
                    let text = format!("export {}", node_text(source, &child));
                    sigs.push(Signature {
                        kind: SignatureKind::Trait,
                        text,
                        file: file.to_string(),
                        line: child.start_position().row + 1,
                    });
                }
                "type_alias_declaration" => {
                    let text = format!("export {}", node_text(source, &child));
                    sigs.push(Signature {
                        kind: SignatureKind::TypeAlias,
                        text,
                        file: file.to_string(),
                        line: child.start_position().row + 1,
                    });
                }
                "lexical_declaration" => {
                    let text = format!("export {}", node_text(source, &child));
                    sigs.push(Signature {
                        kind: SignatureKind::Constant,
                        text,
                        file: file.to_string(),
                        line: child.start_position().row + 1,
                    });
                }
                _ => {}
            }
        }
    }

    debug!(file = file, count = sigs.len(), "extracted TS signatures");
    sigs
}

fn extract_python_signatures(source: &str, file: &str) -> Vec<Signature> {
    let mut parser = tree_sitter::Parser::new();
    let py_lang = tree_sitter_python::language();
    if parser.set_language(&py_lang).is_err() {
        return fallback_python_extract(source, file);
    }

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return fallback_python_extract(source, file),
    };

    let mut sigs = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();

    for node in root.children(&mut cursor) {
        match node.kind() {
            "function_definition" => {
                // Get signature line only (def ... :)
                let mut sig_end = node.end_byte();
                let mut child_cursor = node.walk();
                for child in node.children(&mut child_cursor) {
                    if child.kind() == "block" {
                        sig_end = child.start_byte();
                        break;
                    }
                }
                let text = source[node.start_byte()..sig_end].trim().to_string();
                // Skip private functions (starting with _)
                if !text.contains("def _") || text.contains("def __init__") {
                    sigs.push(Signature {
                        kind: SignatureKind::Function,
                        text,
                        file: file.to_string(),
                        line: node.start_position().row + 1,
                    });
                }
            }
            "class_definition" => {
                // Get class header only
                let mut sig_end = node.end_byte();
                let mut child_cursor = node.walk();
                for child in node.children(&mut child_cursor) {
                    if child.kind() == "block" {
                        sig_end = child.start_byte();
                        break;
                    }
                }
                let text = source[node.start_byte()..sig_end].trim().to_string();
                sigs.push(Signature {
                    kind: SignatureKind::Struct,
                    text,
                    file: file.to_string(),
                    line: node.start_position().row + 1,
                });
            }
            "expression_statement" => {
                // Top-level type-annotated assignments
                let text = node_text(source, &node);
                if text.contains(':') && !text.starts_with('_') {
                    sigs.push(Signature {
                        kind: SignatureKind::Constant,
                        text,
                        file: file.to_string(),
                        line: node.start_position().row + 1,
                    });
                }
            }
            _ => {}
        }
    }

    debug!(
        file = file,
        count = sigs.len(),
        "extracted Python signatures"
    );
    sigs
}

/// Check if a Rust node has a `pub` visibility modifier.
fn is_pub(source: &str, node: &tree_sitter::Node<'_>) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = node_text(source, &child);
            return text.starts_with("pub");
        }
    }
    false
}

/// Get the text of a tree-sitter node.
fn node_text(source: &str, node: &tree_sitter::Node<'_>) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}

// Fallback extractors for when tree-sitter parsing fails

fn fallback_rust_extract(source: &str, file: &str) -> Vec<Signature> {
    let mut sigs = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("pub trait ")
        {
            let kind = if trimmed.starts_with("pub fn") {
                SignatureKind::Function
            } else if trimmed.starts_with("pub struct") {
                SignatureKind::Struct
            } else if trimmed.starts_with("pub enum") {
                SignatureKind::Enum
            } else {
                SignatureKind::Trait
            };
            sigs.push(Signature {
                kind,
                text: trimmed.trim_end_matches('{').trim().to_string(),
                file: file.to_string(),
                line: i + 1,
            });
        }
    }
    sigs
}

fn fallback_ts_extract(source: &str, file: &str) -> Vec<Signature> {
    let mut sigs = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("export function ")
            || trimmed.starts_with("export class ")
            || trimmed.starts_with("export interface ")
            || trimmed.starts_with("export type ")
            || trimmed.starts_with("export const ")
        {
            sigs.push(Signature {
                kind: SignatureKind::Function,
                text: trimmed.trim_end_matches('{').trim().to_string(),
                file: file.to_string(),
                line: i + 1,
            });
        }
    }
    sigs
}

fn fallback_python_extract(source: &str, file: &str) -> Vec<Signature> {
    let mut sigs = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if (trimmed.starts_with("def ") || trimmed.starts_with("class "))
            && !trimmed.starts_with("def _")
        {
            let kind = if trimmed.starts_with("def ") {
                SignatureKind::Function
            } else {
                SignatureKind::Struct
            };
            sigs.push(Signature {
                kind,
                text: trimmed.trim_end_matches(':').trim().to_string(),
                file: file.to_string(),
                line: i + 1,
            });
        }
    }
    sigs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_extraction() {
        let source = r#"
pub fn hello(name: &str) -> String {
    format!("hello {name}")
}

fn private_fn() {}

pub struct Foo {
    pub bar: u32,
}

pub enum Color {
    Red,
    Green,
    Blue,
}
"#;
        let sigs = extract_rust_signatures(source, "test.rs");
        assert!(sigs.iter().any(|s| s.text.contains("pub fn hello")));
        assert!(!sigs.iter().any(|s| s.text.contains("private_fn")));
        assert!(sigs.iter().any(|s| s.text.contains("pub struct Foo")));
    }

    #[test]
    fn test_python_extraction() {
        let source = r#"
def hello(name: str) -> str:
    return f"hello {name}"

def _private():
    pass

class Greeter:
    def __init__(self):
        pass
"#;
        let sigs = extract_python_signatures(source, "test.py");
        assert!(sigs.iter().any(|s| s.text.contains("def hello")));
        assert!(!sigs.iter().any(|s| s.text == "def _private():"));
        assert!(sigs.iter().any(|s| s.text.contains("class Greeter")));
    }
}
