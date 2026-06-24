//! Markdown documentation generator
//!
//! Generates clean Markdown documentation from extracted documentation structures.

use crate::types::*;

/// Generate Markdown documentation for a module
pub fn generate_markdown(module: &ModuleDoc) -> String {
    let mut output = String::new();

    // Module header
    output.push_str(&format!("# {}\n\n", module.name));

    // Description
    if let Some(desc) = &module.description {
        output.push_str(desc);
        output.push_str("\n\n");
    }

    // Table of contents
    output.push_str(&generate_toc(module));
    output.push('\n');

    // Constants
    if !module.constants.is_empty() {
        output.push_str(&generate_constants_section(&module.constants));
    }

    // Type aliases
    if !module.type_aliases.is_empty() {
        output.push_str(&generate_type_aliases_section(&module.type_aliases));
    }

    // Enums
    if !module.enums.is_empty() {
        output.push_str(&generate_enums_section(&module.enums));
    }

    // Structs
    if !module.structs.is_empty() {
        output.push_str(&generate_structs_section(&module.structs));
    }

    // Classes
    if !module.classes.is_empty() {
        output.push_str(&generate_classes_section(&module.classes));
    }

    // Traits
    if !module.traits.is_empty() {
        output.push_str(&generate_traits_section(&module.traits));
    }

    // Impl blocks
    if !module.impls.is_empty() {
        output.push_str(&generate_impls_section(&module.impls));
    }

    // Functions
    if !module.functions.is_empty() {
        output.push_str(&generate_functions_section(&module.functions));
    }

    output
}

/// Generate table of contents
fn generate_toc(module: &ModuleDoc) -> String {
    let mut toc = String::from("## Contents\n\n");
    let mut has_content = false;

    if !module.constants.is_empty() {
        toc.push_str("- [Constants](#constants)\n");
        has_content = true;
    }
    if !module.type_aliases.is_empty() {
        toc.push_str("- [Type Aliases](#type-aliases)\n");
        has_content = true;
    }
    if !module.enums.is_empty() {
        toc.push_str("- [Enums](#enums)\n");
        has_content = true;
    }
    if !module.structs.is_empty() {
        toc.push_str("- [Structs](#structs)\n");
        has_content = true;
    }
    if !module.classes.is_empty() {
        toc.push_str("- [Classes](#classes)\n");
        has_content = true;
    }
    if !module.traits.is_empty() {
        toc.push_str("- [Traits](#traits)\n");
        has_content = true;
    }
    if !module.impls.is_empty() {
        toc.push_str("- [Implementations](#implementations)\n");
        has_content = true;
    }
    if !module.functions.is_empty() {
        toc.push_str("- [Functions](#functions)\n");
        has_content = true;
    }

    if has_content {
        toc.push('\n');
        toc
    } else {
        String::new()
    }
}

/// Generate constants section
fn generate_constants_section(constants: &[ConstDoc]) -> String {
    let mut output = String::from("## Constants\n\n");

    for c in constants {
        output.push_str(&format!("### `{}`\n\n", c.name));

        if let Some(type_name) = &c.type_name {
            output.push_str(&format!("**Type:** `{}`\n\n", type_name));
        }

        if let Some(doc) = &c.doc_comment {
            output.push_str(doc);
            output.push_str("\n\n");
        }

        output.push_str("---\n\n");
    }

    output
}

/// Generate type aliases section
fn generate_type_aliases_section(aliases: &[TypeAliasDoc]) -> String {
    let mut output = String::from("## Type Aliases\n\n");

    for alias in aliases {
        output.push_str(&format!("### `{}`\n\n", alias.name));
        output.push_str(&format!(
            "```lira\ntype {} = {}\n```\n\n",
            alias.name, alias.target_type
        ));

        if let Some(doc) = &alias.doc_comment {
            output.push_str(doc);
            output.push_str("\n\n");
        }

        output.push_str("---\n\n");
    }

    output
}

/// Generate enums section
fn generate_enums_section(enums: &[EnumDoc]) -> String {
    let mut output = String::from("## Enums\n\n");

    for e in enums {
        output.push_str(&format!("### `{}`\n\n", e.name));

        // Generate enum definition
        output.push_str("```lira\n");
        output.push_str(&format!("enum {} {{\n", e.name));
        for variant in &e.variants {
            if variant.fields.is_empty() {
                output.push_str(&format!("    {},\n", variant.name));
            } else {
                output.push_str(&format!(
                    "    {}({}),\n",
                    variant.name,
                    variant.fields.join(", ")
                ));
            }
        }
        output.push_str("}\n```\n\n");

        if let Some(doc) = &e.doc_comment {
            output.push_str(doc);
            output.push_str("\n\n");
        }

        // Variants table
        if !e.variants.is_empty() {
            output.push_str("#### Variants\n\n");
            output.push_str("| Variant | Fields |\n");
            output.push_str("|---------|--------|\n");
            for variant in &e.variants {
                let fields = if variant.fields.is_empty() {
                    "-".to_string()
                } else {
                    format!("`{}`", variant.fields.join(", "))
                };
                output.push_str(&format!("| `{}` | {} |\n", variant.name, fields));
            }
            output.push('\n');
        }

        output.push_str("---\n\n");
    }

    output
}

/// Generate structs section
fn generate_structs_section(structs: &[StructDoc]) -> String {
    let mut output = String::from("## Structs\n\n");

    for s in structs {
        output.push_str(&format!("### `{}`", s.name));
        if !s.type_params.is_empty() {
            output.push_str(&format!("<{}>", s.type_params.join(", ")));
        }
        output.push_str("\n\n");

        // Generate struct definition
        output.push_str("```lira\n");
        output.push_str(&format!("struct {}", s.name));
        if !s.type_params.is_empty() {
            output.push_str(&format!("<{}>", s.type_params.join(", ")));
        }
        output.push_str(" {\n");
        for field in &s.fields {
            let mut modifiers = Vec::new();
            if field.is_public {
                modifiers.push("pub");
            }
            if field.is_mutable {
                modifiers.push("var");
            }
            let prefix = if modifiers.is_empty() {
                String::new()
            } else {
                format!("{} ", modifiers.join(" "))
            };
            output.push_str(&format!(
                "    {}{}: {},\n",
                prefix, field.name, field.type_name
            ));
        }
        output.push_str("}\n```\n\n");

        if let Some(doc) = &s.doc_comment {
            output.push_str(doc);
            output.push_str("\n\n");
        }

        // Fields table
        if !s.fields.is_empty() {
            output.push_str("#### Fields\n\n");
            output.push_str("| Field | Type | Visibility |\n");
            output.push_str("|-------|------|------------|\n");
            for field in &s.fields {
                let vis = if field.is_public { "public" } else { "private" };
                output.push_str(&format!(
                    "| `{}` | `{}` | {} |\n",
                    field.name, field.type_name, vis
                ));
            }
            output.push('\n');
        }

        // Methods
        if !s.methods.is_empty() {
            output.push_str("#### Methods\n\n");
            for method in &s.methods {
                output.push_str(&format!("##### `{}`\n\n", method.name));
                output.push_str(&format!("```lira\n{}\n```\n\n", method.signature));
                if let Some(doc) = &method.doc_comment {
                    output.push_str(doc);
                    output.push_str("\n\n");
                }
            }
        }

        output.push_str("---\n\n");
    }

    output
}

/// Generate classes section
fn generate_classes_section(classes: &[ClassDoc]) -> String {
    let mut output = String::from("## Classes\n\n");

    for c in classes {
        output.push_str(&format!("### `{}`", c.name));
        if let Some(parent) = &c.parent {
            output.push_str(&format!(" extends `{}`", parent));
        }
        output.push_str("\n\n");

        if !c.interfaces.is_empty() {
            output.push_str(&format!(
                "**Implements:** {}\n\n",
                c.interfaces
                    .iter()
                    .map(|i| format!("`{}`", i))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if let Some(doc) = &c.doc_comment {
            output.push_str(doc);
            output.push_str("\n\n");
        }

        // Fields table
        if !c.fields.is_empty() {
            output.push_str("#### Fields\n\n");
            output.push_str("| Field | Type | Visibility |\n");
            output.push_str("|-------|------|------------|\n");
            for field in &c.fields {
                let vis = if field.is_public { "public" } else { "private" };
                output.push_str(&format!(
                    "| `{}` | `{}` | {} |\n",
                    field.name, field.type_name, vis
                ));
            }
            output.push('\n');
        }

        // Methods
        if !c.methods.is_empty() {
            output.push_str("#### Methods\n\n");
            for method in &c.methods {
                output.push_str(&format!("##### `{}`\n\n", method.name));
                output.push_str(&format!("```lira\n{}\n```\n\n", method.signature));
                if let Some(doc) = &method.doc_comment {
                    output.push_str(doc);
                    output.push_str("\n\n");
                }
            }
        }

        output.push_str("---\n\n");
    }

    output
}

/// Generate traits section
fn generate_traits_section(traits: &[TraitDoc]) -> String {
    let mut output = String::from("## Traits\n\n");

    for t in traits {
        output.push_str(&format!("### `{}`", t.name));
        if !t.type_params.is_empty() {
            output.push_str(&format!("<{}>", t.type_params.join(", ")));
        }
        output.push_str("\n\n");

        if let Some(doc) = &t.doc_comment {
            output.push_str(doc);
            output.push_str("\n\n");
        }

        // Methods
        if !t.methods.is_empty() {
            output.push_str("#### Required Methods\n\n");
            output.push_str("| Method | Default |\n");
            output.push_str("|--------|--------|\n");
            for method in &t.methods {
                let default = if method.has_default { "Yes" } else { "No" };
                output.push_str(&format!("| `{}` | {} |\n", method.signature, default));
            }
            output.push('\n');
        }

        output.push_str("---\n\n");
    }

    output
}

/// Generate implementations section
fn generate_impls_section(impls: &[ImplDoc]) -> String {
    let mut output = String::from("## Implementations\n\n");

    for i in impls {
        if let Some(trait_name) = &i.trait_name {
            output.push_str(&format!("### `impl {} for {}`", trait_name, i.type_name));
        } else {
            output.push_str(&format!("### `impl {}`", i.type_name));
        }
        if !i.type_params.is_empty() {
            output.push_str(&format!("<{}>", i.type_params.join(", ")));
        }
        output.push_str("\n\n");

        // Methods
        if !i.methods.is_empty() {
            output.push_str("#### Methods\n\n");
            for method in &i.methods {
                output.push_str(&format!("##### `{}`\n\n", method.name));
                output.push_str(&format!("```lira\n{}\n```\n\n", method.signature));
                if let Some(doc) = &method.doc_comment {
                    output.push_str(doc);
                    output.push_str("\n\n");
                }
            }
        }

        output.push_str("---\n\n");
    }

    output
}

/// Generate functions section
fn generate_functions_section(functions: &[FunctionDoc]) -> String {
    let mut output = String::from("## Functions\n\n");

    for f in functions {
        output.push_str(&format!("### `{}`\n\n", f.name));
        output.push_str(&format!("```lira\n{}\n```\n\n", f.signature));

        if let Some(doc) = &f.doc_comment {
            output.push_str(doc);
            output.push_str("\n\n");
        }

        // Parameters table (if there are any)
        if !f.params.is_empty() {
            output.push_str("**Parameters:**\n\n");
            output.push_str("| Name | Type | Default |\n");
            output.push_str("|------|------|---------|\n");
            for param in &f.params {
                let default = match &param.default_value {
                    Some(v) => format!("`{}`", v),
                    None => String::new(),
                };
                output.push_str(&format!(
                    "| `{}` | `{}` | {} |\n",
                    param.name, param.type_name, default
                ));
            }
            output.push('\n');
        }

        if let Some(ret) = &f.return_type {
            output.push_str(&format!("**Returns:** `{}`\n\n", ret));
        }

        output.push_str("---\n\n");
    }

    output
}
