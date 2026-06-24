# Plan: NodeId + SemanticTables + Typed HIR

## Goal

Replace the current `TypedProgram = Program` pattern with a proper semantic pipeline:

```
AST (source-shaped, immutable)
  → Checker produces SemanticTables (types, resolutions, instantiations)
    → HIR (typed lowered representation)
      → Codegen reads HIR, not raw AST
```

---

## Phase 1: Add NodeId to AST + Parser

**Goal:** Every AST node gets a unique `NodeId` for stable side-table keys.

### 1.1 Create `crates/lirac/src/ids.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);
```

### 1.2 Add `id: NodeId` to AST nodes

Add to: `Expression`, `Statement`, `Pattern`, `Parameter`

```rust
pub struct Expression {
    pub id: NodeId,
    pub kind: ExpressionKind,
    pub span: Span,
}

pub struct Statement {
    pub id: NodeId,
    pub kind: StatementKind,
    pub span: Span,
}
```

### 1.3 Parser assigns IDs

Add `NodeIdGen` to parser:

```rust
struct NodeIdGen(u32);

impl NodeIdGen {
    fn next(&mut self) -> NodeId {
        let id = NodeId(self.0);
        self.0 += 1;
        id
    }
}
```

Update every AST construction site to use `self.node_id.next()`.

### TDD Tests

- Parse a program, verify all nodes have unique IDs
- Verify IDs are assigned in source order
- Verify re-parsing same source produces same ID sequence

### Checkpoint: Commit with message "feat: add NodeId to AST nodes"

---

## Phase 2: Create SemanticTables

**Goal:** Define the side-table structures that capture all checker facts.

### 2.1 Create `crates/lirac/src/sema.rs`

```rust
use crate::ast::NodeId;
use crate::checker::Type;

pub struct SemanticTables {
    /// Expression result types
    pub expr_types: HashMap<NodeId, Type>,

    /// Statement types (for VarDecl, the declared/inferred type)
    pub stmt_types: HashMap<NodeId, Type>,

    /// Pattern types
    pub pattern_types: HashMap<NodeId, Type>,

    /// Resolved symbol references
    pub symbol_refs: HashMap<NodeId, SymbolId>,

    /// Symbol table (all declarations)
    pub symbols: HashMap<SymbolId, SymbolEntry>,

    /// Call resolution (which function/method was called?)
    pub call_resolution: HashMap<NodeId, CallResolution>,

    /// Field resolution (which field was accessed?)
    pub field_resolution: HashMap<NodeId, FieldResolution>,

    /// Generic instantiations
    pub generic_instantiations: Vec<GenericInstantiation>,
}

pub struct SymbolEntry {
    pub id: SymbolId,
    pub name: String,
    pub ty: Type,
    pub kind: SymbolKind,
    pub decl_node: NodeId,
}

pub enum CallResolution {
    Function { name: String },
    Method { type_name: String, method_name: String },
    StaticMethod { type_name: String, method_name: String },
}

pub struct FieldResolution {
    pub owner_type: Type,
    pub field_name: String,
    pub is_method: bool,
    pub resolved_type: Type,
}
```

### TDD Tests

- Create `SemanticTables` from a simple program
- Verify `expr_types` contains correct types for literals, binary ops, calls
- Verify `symbol_refs` resolves identifiers to their declarations
- Verify `call_resolution` distinguishes free functions vs methods

### Checkpoint: Commit with message "feat: add SemanticTables structure"

---

## Phase 3: Wire Checker to Fill SemanticTables

**Goal:** The checker already computes all these facts — we just need to capture them.

### 3.1 Add `sema: SemanticTables` to `TypeChecker`

```rust
pub struct TypeChecker {
    env: TypeEnv,
    sema: SemanticTables,
    next_symbol_id: u32,
    // ... existing fields
}
```

### 3.2 Record `expr_types` in `check_expression`

```rust
fn check_expression(&mut self, expr: &Expression) -> Type {
    let ty = match &expr.kind {
        ExpressionKind::IntLiteral(_) => Type::Int,
        // ... same logic
    };
    self.sema.expr_types.insert(expr.id, ty.clone());
    ty
}
```

### 3.3 Record `symbol_refs` in identifier resolution

In `ExpressionKind::Identifier` handler:
```rust
if let Some(symbol) = self.env.lookup(name) {
    self.sema.symbol_refs.insert(expr.id, symbol.id);
    symbol.ty.clone()
}
```

### 3.4 Record `call_resolution` in call checking

After resolving callee:
```rust
let resolution = match &callee.kind {
    ExpressionKind::Identifier(name) => CallResolution::Function { name: name.clone() },
    ExpressionKind::FieldAccess { object, field } => {
        CallResolution::Method { type_name, method_name: field.clone() }
    }
    _ => CallResolution::Function { name: "<anonymous>".to_string() },
};
self.sema.call_resolution.insert(expr.id, resolution);
```

### 3.5 Record `field_resolution` in field access checking

After resolving field:
```rust
self.sema.field_resolution.insert(expr.id, FieldResolution {
    owner_type: obj_type.clone(),
    field_name: field.clone(),
    is_method: is_method_call,
    resolved_type: field_type.clone(),
});
```

### 3.6 Change `TypedProgram` to `CheckedProgram`

```rust
pub struct CheckedProgram {
    pub program: Program,
    pub sema: SemanticTables,
}

pub fn check(program: Program) -> Result<CheckedProgram, DiagnosticBag> {
    let mut checker = TypeChecker::new();
    let sema = checker.check_program(program)?;
    Ok(CheckedProgram { program, sema })
}
```

### TDD Tests

- Parse + check a program, verify `sema.expr_types` has correct types
- Test symbol resolution for variables, functions, structs
- Test call resolution for free functions and methods
- Test field resolution for struct fields and methods
- All existing checker tests still pass

### Checkpoint: Commit with message "feat: wire checker to fill SemanticTables"

---

## Phase 4: Update Codegen to Use SemanticTables

**Goal:** Codegen reads types from sema instead of re-deriving them.

### 4.1 Change `generate` signature

```rust
pub fn generate(program: &CheckedProgram) -> Result<Vec<u8>, String> {
```

### 4.2 Replace `local_types` with sema lookups

Currently:
```rust
local_types: Vec<HashMap<String, String>>,
```

Replace with queries to `program.sema.expr_types` and `program.sema.symbol_refs`.

### 4.3 Replace method dispatch guessing with `field_resolution`

Instead of string-matching method names:
```rust
if let Some(resolution) = program.sema.field_resolution.get(&expr.id) {
    let mangled = format!("{}_{}", resolution.owner_type.display_name(), field);
}
```

### 4.4 Remove `infer_primitive_type()` hack

This function exists because codegen doesn't have type info. With sema, it's unnecessary.

### TDD Tests

- All 84 existing integration tests pass
- Bytecode output is identical for all programs (no behavior change)
- Remove type-inference hacks, verify correctness

### Checkpoint: Commit with message "feat: codegen reads from SemanticTables"

---

## Phase 5: Add HIR + Lowering

**Goal:** Typed HIR simplifies codegen by resolving all names and types upfront.

### 5.1 Create `crates/lirac/src/hir.rs`

```rust
pub struct HirProgram {
    pub functions: Vec<HirFunction>,
    pub top_level: Vec<HirStatement>,
}

pub struct HirFunction {
    pub id: FunctionId,
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: Vec<HirStatement>,
}

pub struct HirExpr {
    pub id: HirExprId,
    pub ty: Type,
    pub kind: HirExprKind,
}

pub enum HirExprKind {
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    BoolLiteral(bool),
    Identifier(SymbolId),
    Binary { op: BinaryOp, left: HirExprId, right: HirExprId },
    Call { func: FunctionId, args: Vec<HirExprId> },
    MethodCall { receiver: HirExprId, method: MethodId, args: Vec<HirExprId> },
    FieldGet { object: HirExprId, field: FieldId },
    // ... etc
}
```

### 5.2 Create `crates/lirac/src/lower.rs`

```rust
pub fn lower(checked: &CheckedProgram) -> Result<HirProgram, String> {
    // Walk AST, using sema to resolve all names/types/methods
    // Produce simplified HIR where:
    //   - All names are resolved to IDs (no string matching)
    //   - All types are explicit (no inference variables)
    //   - Method calls are distinguished from field access
    //   - Generic instantiations are explicit
}
```

### 5.3 Update pipeline

```rust
pub fn compile(source: &str) -> Result<Vec<u8>, String> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens)?;
    let checked = checker::check(ast)?;
    let hir = hir::lower(&checked)?;
    let bytecode = codegen::generate(&hir)?;
    Ok(bytecode)
}
```

### TDD Tests

- Lower a simple program, verify HIR structure
- Verify HIR types match expected types
- All integration tests still pass
- Codegen reads HIR instead of AST

### Checkpoint: Commit with message "feat: add typed HIR and lowering"

---

## Phase 6: Cleanup + Final Verification

**Goal:** Remove dead code, verify everything works.

### 6.1 Remove old codegen hacks

- Remove `infer_primitive_type()`
- Remove `local_types` field
- Remove type-name string matching in method dispatch

### 6.2 Run full test suite

```bash
just test
```

All 1077+ tests pass.

### 6.3 Final commit with message "refactor: remove dead codegen hacks"

---

## Implementation Order Summary

| Phase | Files Changed | Effort | Tests |
|-------|---------------|--------|-------|
| 1. NodeId in AST | ast.rs, parser.rs, ids.rs (new) | Medium | 2-3 new tests |
| 2. SemanticTables | sema.rs (new) | Medium | 3-4 new tests |
| 3. Wire checker | checker.rs, lib.rs | Large | 5-6 new tests |
| 4. Codegen uses sema | codegen.rs, lib.rs | Large | All existing pass |
| 5. HIR + lowering | hir.rs, lower.rs (new), codegen.rs | Large | 4-5 new tests |
| 6. Cleanup | codegen.rs | Small | All existing pass |

---

## Design Decisions

1. **NodeId allocation:** Sequential u32 assigned by parser. Simple, fast, no HashMap needed for generation.

2. **SemanticTables ownership:** Owned by `CheckedProgram`, passed by reference to codegen/lowering.

3. **SymbolId for identifiers:** When an identifier resolves to a known declaration, store the SymbolId. This enables "go to definition" in the LSP.

4. **CallResolution variants:** Distinguish function calls, method calls, and static calls. The checker already knows which is which — we just need to record it.

5. **HIR as separate phase:** The HIR is a simplification of the AST, not just AST+types. Method resolution, field resolution, and generic instantiation are all resolved at HIR construction time.

6. **Span upgrade:** Deferred. Current `(line, column)` is sufficient. Can add `(file_id, start_byte, end_byte)` later when we need multi-file support or LSP go-to-definition.

---

## Risk Mitigation

1. **Breaking AST construction:** Every `Expression { kind, span }` becomes `Expression { id, kind, span }`. This is mechanical but touches many files. Use compiler errors to guide fixes.

2. **Checker mutation:** The checker currently takes `&mut self` and mutates the AST via `check_statement`. With NodeId, we only add to sema, never mutate the AST. This is cleaner.

3. **Codegen regression:** Run all 84 integration tests after each phase. Bytecode output should be identical until Phase 5 (HIR).

4. **HIR complexity:** Keep HIR simple. It's a typed representation for codegen, not a full IR with optimizations.
