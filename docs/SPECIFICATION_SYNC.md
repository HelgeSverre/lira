# Lira Specification Sync Strategy

This document defines the sources of truth for the Lira language and how they should be kept in sync.

## Sources of Truth

Lira has four primary artifacts that define the language:

| Artifact | Location | Purpose | Authority |
|----------|----------|---------|-----------|
| **Formal Spec** | `docs/FORMAL_SPECIFICATION.md` | Normative language definition | **PRIMARY** for semantics |
| **Tree-sitter** | `editors/tree-sitter-lira/grammar.js` | Editor/IDE syntax support | **PRIMARY** for editor tooling |
| **Lexer** | `crates/lirac/src/lexer.rs` | Token definitions | **PRIMARY** for tokenization |
| **Parser** | `crates/lirac/src/parser.rs` | AST construction | **PRIMARY** for implementation |

## Authority Hierarchy

```
                    ┌─────────────────────┐
                    │   Formal Spec       │  ← What the language SHOULD be
                    │   (EBNF + Prose)    │
                    └──────────┬──────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
              ▼                ▼                ▼
    ┌─────────────────┐ ┌─────────────┐ ┌─────────────────┐
    │   Lexer/Parser  │ │ Tree-sitter │ │   Type Checker  │
    │  (lirac crate)  │ │  (editors)  │ │   (checker.rs)  │
    └─────────────────┘ └─────────────┘ └─────────────────┘
              │                │                │
              └────────────────┼────────────────┘
                               │
                               ▼
                    ┌─────────────────────┐
                    │   lira-spec crate   │  ← Validates all sources agree
                    └─────────────────────┘
```

## Sync Direction by Domain

### 1. Syntax (Grammar Rules)

| Aspect | Source of Truth | Sync To | Rationale |
|--------|-----------------|---------|-----------|
| Production names | Formal Spec | Tree-sitter, Parser | Spec defines canonical names |
| Operator precedence | Parser (Pratt) | Formal Spec, Tree-sitter | Parser is tested, proven correct |
| Conflict resolution | Tree-sitter | Formal Spec | Tree-sitter exposes real ambiguities |

**Current State:**
- Formal Spec: 164 productions
- Tree-sitter: 133 rules (8 hidden)
- Sync percentage: ~12% (due to naming conventions)

**Action Required:**
- [ ] Normalize naming: `_decl` ↔ `_declaration`, `_stmt` ↔ `_statement`, `_expr` ↔ `_expression`
- [ ] Add missing tree-sitter constructs to formal spec
- [ ] Document tree-sitter conflicts in formal spec's grammar notes

### 2. Keywords

| Aspect | Source of Truth | Sync To | Rationale |
|--------|-----------------|---------|-----------|
| Keyword list | Lexer (`lexer.rs`) | Formal Spec, Tree-sitter | Lexer is implementation |
| Keyword categories | Formal Spec | Documentation | Spec organizes semantically |

**Current State:**
- Formal Spec: 52 keywords (Appendix A)
- Lexer: 48 keyword tokens
- Tree-sitter: 95 "keywords" (includes field names)

**Discrepancies:**
- `await`, `receive`, `throw` in spec but not tree-sitter
- Tree-sitter includes field names (`body`, `name`, `value`) as "keywords"

**Action Required:**
- [ ] Audit lexer keywords against spec Appendix A
- [ ] Fix tree-sitter keyword extraction (exclude field names)
- [ ] Ensure all reserved words are in all three sources

### 3. Type System

| Aspect | Source of Truth | Sync To | Rationale |
|--------|-----------------|---------|-----------|
| Type inference rules | Formal Spec | Checker | Spec has formal notation |
| Primitive types | Lexer/Checker | Formal Spec | Implementation defines sizes |
| Generic constraints | Parser | Formal Spec | Parser handles syntax |

**Current State:**
- Formal Spec: 15 type inference rules
- lira-spec validates: 27/27 type tests passing

**Action Required:**
- [ ] Add more type rules to lira-spec (generics, trait bounds)
- [ ] Verify checker implements all spec rules

### 4. Semantics

| Aspect | Source of Truth | Sync To | Rationale |
|--------|-----------------|---------|-----------|
| Evaluation order | Formal Spec | VM | Spec defines semantics |
| Memory model | Formal Spec | VM | Spec defines ARC behavior |
| Concurrency | Formal Spec | VM | Spec defines fiber/channel semantics |

**Current State:**
- Formal Spec has complete semantics (Sections 5-7)
- No automated validation of semantic conformance yet

**Action Required:**
- [ ] Add semantic validation tests to lira-spec
- [ ] Test VM against spec's operational semantics

## Naming Convention Mapping

The following mappings should be applied when comparing sources:

```
Formal Spec          ↔  Tree-sitter           ↔  Parser Method
─────────────────────────────────────────────────────────────
program              ↔  source_file           ↔  parse()
variable_decl        ↔  variable_declaration  ↔  parse_variable_decl()
function_decl        ↔  function_declaration  ↔  parse_function_decl()
struct_decl          ↔  struct_declaration    ↔  parse_struct_decl()
class_decl           ↔  class_declaration     ↔  parse_class_decl()
enum_decl            ↔  enum_declaration      ↔  parse_enum_decl()
trait_decl           ↔  trait_declaration     ↔  parse_trait_decl()
impl_decl            ↔  impl_block            ↔  parse_impl_decl()
if_expr              ↔  if_expression         ↔  parse_if_expr()
match_expr           ↔  match_expression      ↔  parse_match_expr()
for_stmt             ↔  for_statement         ↔  parse_for_stmt()
while_stmt           ↔  while_statement       ↔  parse_while_stmt()
return_stmt          ↔  return_statement      ↔  parse_return_stmt()
break_stmt           ↔  break_statement       ↔  parse_break_stmt()
lambda_expr          ↔  lambda_expression     ↔  parse_lambda_expr()
```

## Validation Tools

### lira-spec Crate

The `lira-spec` crate provides automated validation:

```bash
# Validate implementation against spec
just spec-validate

# Compare EBNF with tree-sitter
just spec-compare

# Run all conformance tests
just spec-test
```

**Current Validation Coverage:**
- Grammar validation: 189/434 tests (43%)
- Type validation: 27/27 tests (100%)
- Tree-sitter comparison: 32/257 rules matched (12%)

### Improving Validation

To reach higher sync percentages:

1. **Name normalization** - Apply mapping table above
2. **Structural comparison** - Compare rule bodies, not just names
3. **Semantic tests** - Add runtime behavior tests
4. **Fuzzing** - Generate random valid programs from grammar

## Remaining Work

### Phase 21 Updates Needed

| Task | Status | Notes |
|------|--------|-------|
| T21.8 Standard library spec | Pending | Document stdlib API contracts |
| T21.9 Versioning policy | Pending | Define semver rules |
| T21.10 Spec website | Pending | Publish to docs.lira-lang.org |

### New Tasks to Add

| Task | Description | Priority |
|------|-------------|----------|
| T21.12 | Name normalization in lira-spec comparison | High |
| T21.13 | Sync tree-sitter rule names to formal spec | Medium |
| T21.14 | Add missing keywords to all sources | High |
| T21.15 | Semantic conformance tests | Medium |
| T21.16 | Grammar fuzzer for conformance | Low |

## Maintenance Process

When changing the language:

1. **Update Formal Spec first** - This is the normative source
2. **Update lexer/parser** - Implement the change
3. **Update tree-sitter** - Editor support
4. **Run `just spec-compare`** - Verify sync
5. **Run `just spec-test`** - Verify conformance

## Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2024-01-01 | Formal Spec is primary source of truth | Enables language evolution independent of implementation |
| 2024-01-01 | Parser precedence is authoritative | Battle-tested through real usage |
| 2024-01-01 | Tree-sitter conflicts inform spec | Reveals real grammar ambiguities |
