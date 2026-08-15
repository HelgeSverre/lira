//! Lowering checked Lira into Cranelift IR.
//!
//! The bytecode backend in `lirac::codegen` emits stack machine instructions
//! against a tagged `Value`. This backend walks the same AST but consults the
//! checker's `SemanticTables` at every expression, so it always knows the
//! concrete type it is dealing with and can emit unboxed machine operations:
//! `a + b` on two `int`s is one `iadd`, `pt.x` is one `load` at a constant
//! offset, and a `match` over an enum is a load of the discriminant followed by
//! a comparison chain.
//!
//! Anything the backend cannot lower yet is reported as
//! `CodegenError::Unsupported` rather than mis-compiled — the bytecode VM stays
//! the complete implementation of the language.

use std::collections::{HashMap, HashSet};

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    types, AbiParam, Block, FuncRef, InstBuilder, MemFlagsData, Signature, StackSlotData,
    StackSlotKind, Type as ClifType, Value,
};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};

use lirac::ast::{
    Argument, BinaryOp, Block as AstBlock, Expression, ExpressionKind, MatchArm, Parameter,
    Pattern, PatternKind, Program, SelectArm, SelectArmKind, Span, Statement, StatementKind,
    UnaryOp,
};
use lirac::checker::Type;
use lirac::sema::SemanticTables;

use crate::abi::{is_unsigned, optional_is_boxed, repr_of, Repr};
use crate::error::{CodegenError, CodegenResult};
use crate::layout::{
    self, storage_size, LayoutMap, CLOSURE_CAPTURES_OFFSET, CLOSURE_CODE_OFFSET,
    CLOSURE_COUNT_OFFSET, ENUM_PAYLOAD_OFFSET, ENUM_TAG_OFFSET, HEADER_SIZE, OPTIONAL_SLOT_OFFSET,
    SLOT_SIZE,
};
use crate::runtime;

/// Symbol of the generated fiber-0 entry point.
pub const ENTRY_SYMBOL: &str = "lira__entry";

/// A user function or method, resolved to its symbol and signature.
struct FnInfo {
    symbol: String,
    func_id: FuncId,
    /// Declared parameters, `self` included when present.
    params: Vec<ParamInfo>,
    ret: Type,
    /// Set for methods, naming the type `self` belongs to.
    owner: Option<String>,
}

struct ParamInfo {
    name: String,
    ty: Type,
    default: Option<Expression>,
}

/// A top-level `let`/`var`/`const`, which functions may reference by name.
#[derive(Clone)]
struct GlobalInfo {
    data_id: DataId,
    ty: Type,
}

/// A lambda, queued while lowering the function it appears in and emitted
/// afterwards as a standalone function taking its closure as the first
/// argument.
struct PendingLambda {
    symbol: String,
    func_id: FuncId,
    params: Vec<ParamInfo>,
    ret: Type,
    /// Names copied into the closure object, with the types they had at the
    /// point of capture.
    captures: Vec<(String, Type)>,
    body: Expression,
}

/// A named function used as a value, which needs an `(env, args...)` wrapper to
/// be callable through the same path as a lambda.
struct PendingFnWrapper {
    symbol: String,
    func_id: FuncId,
    /// Key of the function the wrapper forwards to.
    target: String,
}

/// A `spawn f(...)` site, queued while lowering the enclosing function and
/// emitted afterwards as a `LiraFiberEntry` thunk.
struct PendingSpawn {
    symbol: String,
    func_id: FuncId,
    /// Symbol of the function the fiber will run.
    callee: String,
    /// Types of the arguments captured into the heap-allocated environment.
    arg_types: Vec<Type>,
}

/// A local binding: either an SSA variable or a slot in a global cell.
#[derive(Clone)]
enum Binding {
    Local { var: Variable, ty: Type },
    Global(GlobalInfo),
}

/// Where `break` and `continue` jump to in the innermost loop.
struct LoopFrame {
    continue_to: Block,
    exit: Block,
    exit_used: bool,
}

/// Shared state for lowering one program into one module.
pub struct Lowerer<'a> {
    module: &'a mut dyn Module,
    sema: &'a SemanticTables,
    layouts: LayoutMap,
    pointer_ty: ClifType,
    call_conv: CallConv,
    funcs: HashMap<String, FnInfo>,
    globals: HashMap<String, GlobalInfo>,
    runtime_ids: HashMap<String, FuncId>,
    strings: HashMap<String, DataId>,
    spawns: Vec<PendingSpawn>,
    lambdas: Vec<PendingLambda>,
    fn_wrappers: Vec<PendingFnWrapper>,
    /// Closure objects standing in for named functions, one per function.
    fn_values: HashMap<String, DataId>,
    next_spawn: usize,
    next_string: usize,
    next_lambda: usize,
}

impl<'a> Lowerer<'a> {
    pub fn new(
        module: &'a mut dyn Module,
        program: &Program,
        sema: &'a SemanticTables,
    ) -> CodegenResult<Self> {
        let pointer_ty = module.target_config().pointer_type();
        if pointer_ty != types::I64 {
            return Err(CodegenError::unsupported(
                "the native backend targets 64-bit platforms only",
            ));
        }
        // String literals are emitted as pre-built `LiraStr` images with
        // little-endian header words, so a big-endian target would read garbage
        // lengths rather than merely running slowly.
        if module.isa().endianness() != cranelift_codegen::ir::Endianness::Little {
            return Err(CodegenError::unsupported(
                "the native backend targets little-endian platforms only",
            ));
        }
        let call_conv = module.isa().default_call_conv();
        let layouts = LayoutMap::build(program)?;
        Ok(Self {
            module,
            sema,
            layouts,
            pointer_ty,
            call_conv,
            funcs: HashMap::new(),
            globals: HashMap::new(),
            runtime_ids: HashMap::new(),
            strings: HashMap::new(),
            spawns: Vec::new(),
            lambdas: Vec::new(),
            fn_wrappers: Vec::new(),
            fn_values: HashMap::new(),
            next_spawn: 0,
            next_string: 0,
            next_lambda: 0,
        })
    }

    /// Lower a whole program: every function and method, the top-level entry
    /// point, and any `spawn` thunks discovered along the way.
    ///
    /// Returns the id of the generated entry point, which the JIT resolves to an
    /// address and the AOT driver hands to `lira_rt_boot` from `main`.
    pub fn lower_program(&mut self, program: &Program) -> CodegenResult<FuncId> {
        self.declare_functions(program)?;
        self.declare_globals(program)?;

        for (owner, decl) in collect_function_decls(program) {
            self.lower_function(owner.as_deref(), decl)?;
        }

        let entry_id = self.lower_entry(program)?;

        // These are discovered while lowering, and lowering one can discover
        // more: a lambda body may itself contain a lambda or a spawn.
        loop {
            if let Some(pending) = self.lambdas.pop() {
                self.lower_lambda_body(&pending)?;
                continue;
            }
            if let Some(pending) = self.spawns.pop() {
                self.lower_spawn_thunk(&pending)?;
                continue;
            }
            if let Some(pending) = self.fn_wrappers.pop() {
                self.lower_fn_wrapper(&pending)?;
                continue;
            }
            break;
        }
        Ok(entry_id)
    }

    // ---------------------------------------------------------------- //
    // Declaration passes                                                //
    // ---------------------------------------------------------------- //

    fn declare_functions(&mut self, program: &Program) -> CodegenResult<()> {
        for (owner, decl) in collect_function_decls(program) {
            let symbol = match &owner {
                Some(type_name) => format!("lira__{}__{}", type_name, decl.name),
                None => format!("lira__{}", decl.name),
            };
            let key = fn_key(owner.as_deref(), decl.name);
            if self.funcs.contains_key(&key) {
                return Err(CodegenError::unsupported_at(
                    format!("`{}` is defined more than once", key),
                    decl.span,
                ));
            }

            let mut params = Vec::with_capacity(decl.params.len());
            for param in decl.params {
                let ty = if param.name == "self" {
                    match &owner {
                        Some(type_name) => self.user_type(type_name),
                        None => {
                            return Err(CodegenError::unsupported_at(
                                "`self` outside of a method",
                                &param.span,
                            ))
                        }
                    }
                } else {
                    self.normalize(layout::type_of_ann(&param.type_ann))
                };
                params.push(ParamInfo {
                    name: param.name.clone(),
                    ty,
                    default: param.default.clone(),
                });
            }
            let ret = decl
                .return_type
                .map(|t| self.normalize(layout::type_of_ann(t)))
                .unwrap_or(Type::Void);

            let sig = self.signature_for(&params, &ret)?;
            let func_id = self
                .module
                .declare_function(&symbol, Linkage::Local, &sig)
                .map_err(|e| CodegenError::internal(e.to_string()))?;

            self.funcs.insert(
                key,
                FnInfo {
                    symbol,
                    func_id,
                    params,
                    ret,
                    owner: owner.clone(),
                },
            );
        }
        Ok(())
    }

    fn declare_globals(&mut self, program: &Program) -> CodegenResult<()> {
        // Globals are declared in source order, so an initialiser can only refer
        // to names already settled; `known` is what inference resolves against.
        let mut known: HashMap<String, Type> = HashMap::new();
        for stmt in &program.statements {
            let (name, ty) = match &stmt.kind {
                StatementKind::VarDecl {
                    pattern,
                    type_ann,
                    initializer,
                    ..
                } => {
                    let PatternKind::Variable(name) = &pattern.kind else {
                        // A destructuring declaration binds several names at
                        // once. Rather than mint a global cell for each, they
                        // become locals of the entry function — visible to the
                        // rest of the top level, but not to other functions.
                        continue;
                    };
                    let ty = self.declared_type(
                        type_ann.as_ref(),
                        self.sema.pattern_types.get(&pattern.id),
                        self.sema.stmt_types.get(&stmt.id),
                        initializer.as_ref(),
                        &known,
                    );
                    let Some(ty) = ty else {
                        return Err(CodegenError::unsupported_at(
                            format!("cannot infer the type of `{}`", name),
                            &stmt.span,
                        ));
                    };
                    (name.clone(), ty)
                }
                StatementKind::ConstDecl {
                    name,
                    type_ann,
                    initializer,
                } => {
                    let ty = self
                        .declared_type(
                            type_ann.as_ref(),
                            None,
                            self.sema.stmt_types.get(&stmt.id),
                            Some(initializer),
                            &known,
                        )
                        .ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("cannot infer the type of `{}`", name),
                                &stmt.span,
                            )
                        })?;
                    (name.clone(), ty)
                }
                _ => continue,
            };

            // Reject up front rather than at first use, so an unsupported global
            // type is reported once with its declaration site.
            repr_of(&ty).map_err(|e| match e {
                CodegenError::Unsupported { message, .. } => {
                    CodegenError::unsupported_at(message, &stmt.span)
                }
                other => other,
            })?;

            // A name can be declared more than once at the top level — two
            // merged modules, or a `var` reassigned by redeclaration. The first
            // cell stands; later declarations store into it.
            if let Some(existing) = self.globals.get(&name) {
                known.insert(name.clone(), existing.ty.clone());
                continue;
            }

            let symbol = format!("lira__global__{}", name);
            let mut description = DataDescription::new();
            // Every global is one 8-byte cell: a scalar, or a pointer to a heap
            // object. Zero doubles as `0`, `false` and `null`.
            description.define_zeroinit(SLOT_SIZE as usize);
            let data_id = self
                .module
                .declare_data(&symbol, Linkage::Local, true, false)
                .map_err(|e| CodegenError::internal(e.to_string()))?;
            self.module
                .define_data(data_id, &description)
                .map_err(|e| CodegenError::internal(e.to_string()))?;
            known.insert(name.clone(), ty.clone());
            self.globals.insert(name, GlobalInfo { data_id, ty });
        }
        Ok(())
    }

    /// Settle a declaration's type from the annotation, the checker's tables, or
    /// the initialiser — whichever is both present and concrete.
    fn declared_type(
        &self,
        annotation: Option<&lirac::ast::TypeExpr>,
        pattern_ty: Option<&Type>,
        stmt_ty: Option<&Type>,
        initializer: Option<&Expression>,
        known: &HashMap<String, Type>,
    ) -> Option<Type> {
        annotation
            .map(|ann| self.normalize(layout::type_of_ann(ann)))
            .and_then(concrete)
            .or_else(|| {
                pattern_ty
                    .cloned()
                    .map(|t| self.normalize(t))
                    .and_then(concrete)
            })
            .or_else(|| {
                stmt_ty
                    .cloned()
                    .map(|t| self.normalize(t))
                    .and_then(concrete)
            })
            .or_else(|| {
                let resolve = |name: &str| known.get(name).cloned();
                initializer
                    .and_then(|init| infer_or_checked_with(self, &resolve, init))
                    .and_then(concrete)
            })
            .or_else(|| {
                // `let ch = chan(5)` has no better answer than `any`. That is
                // still pointer-shaped and storable; an operation that needs a
                // sharper type fails later, at the use, with a clearer message.
                let resolve = |name: &str| known.get(name).cloned();
                match initializer.and_then(|init| infer_or_checked_with(self, &resolve, init)) {
                    Some(Type::Any) => Some(Type::Any),
                    _ => None,
                }
            })
    }

    fn signature_for(&self, params: &[ParamInfo], ret: &Type) -> CodegenResult<Signature> {
        let mut sig = Signature::new(self.call_conv);
        for param in params {
            let repr = repr_of(&param.ty)?;
            if let Some(clif) = repr.clif(self.pointer_ty) {
                sig.params.push(AbiParam::new(clif));
            } else {
                return Err(CodegenError::unsupported(format!(
                    "parameter `{}` has type `void`",
                    param.name
                )));
            }
        }
        if let Some(clif) = repr_of(ret)?.clif(self.pointer_ty) {
            sig.returns.push(AbiParam::new(clif));
        }
        Ok(sig)
    }

    // ---------------------------------------------------------------- //
    // Type helpers                                                      //
    // ---------------------------------------------------------------- //

    /// Resolve a type name against the built-in types and the program's layouts.
    ///
    /// `impl int { fn abs(self) -> int { ... } }` is how the standard library is
    /// written, so `self` in an impl block is not always an aggregate.
    fn user_type(&self, name: &str) -> Type {
        if let Some(primitive) = layout::primitive_type(name) {
            return primitive;
        }
        // `type Integer = int` makes `Integer` a spelling of `int`, not a type
        // of its own; resolve it before anything asks for a layout.
        if let Some(target) = self.layouts.resolve_alias(name) {
            return self.normalize(target);
        }
        if self.layouts.enums.contains_key(name) {
            Type::Enum(name.to_string())
        } else if let Some(element) = array_impl_element(name) {
            Type::Array(Box::new(element))
        } else {
            Type::Struct(name.to_string())
        }
    }

    /// The checker and the AST both spell a user type as `Struct(name)` when
    /// they have not distinguished it from an enum; settle that here.
    fn normalize(&self, ty: Type) -> Type {
        match ty {
            Type::Result { ok_type, err_type } => Type::Result {
                ok_type: Box::new(self.normalize(*ok_type)),
                err_type: Box::new(self.normalize(*err_type)),
            },
            Type::Struct(name) | Type::Class(name) | Type::Enum(name) => self.user_type(&name),
            Type::Array(inner) => Type::Array(Box::new(self.normalize(*inner))),
            Type::Optional(inner) => Type::Optional(Box::new(self.normalize(*inner))),
            other => other,
        }
    }

    /// The checked type of an expression.
    fn ty_of(&self, expr: &Expression) -> CodegenResult<Type> {
        if let Some(ty) = self.sema.expr_types.get(&expr.id) {
            return Ok(self.normalize(ty.clone()));
        }
        // The checker records a type for every expression it visits. Literals
        // are still worth answering directly: they are the one case that can
        // appear in a position the checker skipped (for example a default
        // argument that was never instantiated).
        Ok(match &expr.kind {
            ExpressionKind::IntLiteral(_) => Type::Int,
            ExpressionKind::FloatLiteral(_) => Type::Float,
            ExpressionKind::StringLiteral(_) => Type::String,
            ExpressionKind::CharLiteral(_) => Type::Char,
            ExpressionKind::BoolLiteral(_) => Type::Bool,
            // A call whose callee the backend does not know is the usual cause
            // here — normally a built-in the native runtime has not grown yet.
            // Naming it beats reporting a missing type.
            ExpressionKind::Call { callee, .. } => {
                if let ExpressionKind::Identifier(name) = &callee.kind {
                    return Err(CodegenError::unsupported_at(
                        format!("unknown function `{}`", name),
                        &expr.span,
                    ));
                }
                return Err(CodegenError::unsupported_at(
                    "the type checker did not record a type for this call",
                    &expr.span,
                ));
            }
            ExpressionKind::MethodCall { method, .. } => {
                return Err(CodegenError::unsupported_at(
                    format!("`.{}()` is not lowered by the native backend yet", method),
                    &expr.span,
                ))
            }
            _ => {
                return Err(CodegenError::unsupported_at(
                    "the type checker did not record a type for this expression",
                    &expr.span,
                ))
            }
        })
    }
}

/// Key under which a function is registered: `name` for free functions,
/// `Type::name` for methods.
fn fn_key(owner: Option<&str>, name: &str) -> String {
    match owner {
        Some(type_name) => format!("{}::{}", type_name, name),
        None => name.to_string(),
    }
}

/// A function declaration paired with the type that owns it, if any.
struct FnDeclRef<'p> {
    name: &'p str,
    params: &'p [Parameter],
    return_type: Option<&'p lirac::ast::TypeExpr>,
    body: &'p AstBlock,
    span: &'p Span,
}

/// Gather every function and method in declaration order, including those
/// nested inside `struct`, `class` and `impl` bodies.
fn collect_function_decls(program: &Program) -> Vec<(Option<String>, FnDeclRef<'_>)> {
    let mut out = Vec::new();
    collect_decls_in(&program.statements, None, &mut out);
    out
}

fn collect_decls_in<'p>(
    statements: &'p [Statement],
    owner: Option<&str>,
    out: &mut Vec<(Option<String>, FnDeclRef<'p>)>,
) {
    for stmt in statements {
        match &stmt.kind {
            StatementKind::FnDecl {
                name,
                params,
                return_type,
                body,
                ..
            } => out.push((
                owner.map(|o| o.to_string()),
                FnDeclRef {
                    name,
                    params,
                    return_type: return_type.as_ref(),
                    body,
                    span: &stmt.span,
                },
            )),
            StatementKind::StructDecl { name, methods, .. }
            | StatementKind::ClassDecl { name, methods, .. } => {
                collect_decls_in(methods, Some(name), out)
            }
            StatementKind::ImplDecl {
                type_name, methods, ..
            } => collect_decls_in(methods, Some(type_name), out),
            StatementKind::Block(block) => collect_decls_in(&block.statements, owner, out),
            _ => {}
        }
    }
}

/// True when a top-level statement is a declaration rather than something the
/// entry point should execute.
fn is_declaration(stmt: &Statement) -> bool {
    matches!(
        stmt.kind,
        StatementKind::FnDecl { .. }
            | StatementKind::StructDecl { .. }
            | StatementKind::ClassDecl { .. }
            | StatementKind::EnumDecl { .. }
            | StatementKind::InterfaceDecl { .. }
            | StatementKind::TraitDecl { .. }
            | StatementKind::ImplDecl { .. }
            | StatementKind::TypeAlias { .. }
            | StatementKind::Import { .. }
            | StatementKind::Use { .. }
    )
}

// ====================================================================== //
// Function lowering                                                       //
// ====================================================================== //

impl<'a> Lowerer<'a> {
    fn lower_function(&mut self, owner: Option<&str>, decl: FnDeclRef<'_>) -> CodegenResult<()> {
        let key = fn_key(owner, decl.name);
        let info = self
            .funcs
            .get(&key)
            .ok_or_else(|| CodegenError::internal(format!("`{}` was never declared", key)))?;
        let func_id = info.func_id;
        let symbol = info.symbol.clone();
        let ret_ty = info.ret.clone();
        let params: Vec<(String, Type)> = info
            .params
            .iter()
            .map(|p| (p.name.clone(), p.ty.clone()))
            .collect();
        let sig = self.signature_for(&info.params, &info.ret)?;

        let frontend_config = self.module.target_config();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);

            let mut gen = FuncGen::new(self, builder, ret_ty.clone());
            gen.push_scope();
            for (index, (name, ty)) in params.iter().enumerate() {
                let value = gen.builder.block_params(entry)[index];
                gen.declare_local(name, ty.clone(), Some(value))?;
            }

            // A body may end in a bare expression rather than a `return` —
            // `fn f() -> string { match x { ... } }` is the common shape.
            let expected = (!matches!(ret_ty, Type::Void)).then(|| ret_ty.clone());
            let (tail, terminated) = gen.lower_block_value(decl.body, expected.as_ref())?;
            if !terminated {
                match (&ret_ty, tail) {
                    (Type::Void, _) => {
                        gen.builder.ins().return_(&[]);
                    }
                    (_, Some(value)) => {
                        gen.builder.ins().return_(&[value]);
                    }
                    (_, None) => {
                        return Err(CodegenError::unsupported_at(
                            format!(
                                "`{}` can finish without returning a `{}`",
                                decl.name,
                                ret_ty.display_name()
                            ),
                            decl.span,
                        ))
                    }
                }
            }
            gen.pop_scope();
            gen.builder.seal_all_blocks();
            gen.builder.finalize(frontend_config);
        }

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| CodegenError::internal(format!("{}: {}", symbol, e)))?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    /// Build fiber 0's entry point out of the program's top-level statements.
    ///
    /// `fn main()` is invoked afterwards, mirroring the bytecode backend — and
    /// skipped when the top level already calls it, so the common
    /// `fn main() {...}` + `main()` pairing does not run twice.
    fn lower_entry(&mut self, program: &Program) -> CodegenResult<FuncId> {
        let mut sig = Signature::new(self.call_conv);
        sig.params.push(AbiParam::new(self.pointer_ty)); // unused fiber env
        let func_id = self
            .module
            .declare_function(ENTRY_SYMBOL, Linkage::Export, &sig)
            .map_err(|e| CodegenError::internal(e.to_string()))?;

        let top_level_calls_main = program.statements.iter().any(|stmt| {
            let StatementKind::Expression(expr) = &stmt.kind else {
                return false;
            };
            let ExpressionKind::Call { callee, args, .. } = &expr.kind else {
                return false;
            };
            args.is_empty()
                && matches!(&callee.kind, ExpressionKind::Identifier(name) if name == "main")
        });
        let call_main = !top_level_calls_main
            && self
                .funcs
                .get("main")
                .is_some_and(|info| info.params.is_empty());

        let frontend_config = self.module.target_config();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);

            let mut gen = FuncGen::new(self, builder, Type::Void);
            gen.push_scope();
            let mut terminated = false;
            for stmt in &program.statements {
                if is_declaration(stmt) {
                    continue;
                }
                if terminated {
                    break;
                }
                terminated = gen.lower_stmt(stmt)?;
            }
            if !terminated {
                if call_main {
                    let main_ref = gen.func_ref("main")?;
                    gen.builder.ins().call(main_ref, &[]);
                }
                gen.builder.ins().return_(&[]);
            }
            gen.pop_scope();
            gen.builder.seal_all_blocks();
            gen.builder.finalize(frontend_config);
        }

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| CodegenError::internal(format!("{}: {}", ENTRY_SYMBOL, e)))?;
        self.module.clear_context(&mut ctx);
        Ok(func_id)
    }

    /// Emit the `LiraFiberEntry` thunk for one `spawn` site.
    ///
    /// Native code cannot hand the scheduler a partially applied call, so the
    /// arguments are boxed into a heap cell at the spawn site and unpacked here.
    fn lower_spawn_thunk(&mut self, pending: &PendingSpawn) -> CodegenResult<()> {
        let callee_info = self.funcs.get(&pending.callee).ok_or_else(|| {
            CodegenError::internal(format!("spawn target `{}` is missing", pending.callee))
        })?;
        let callee_id = callee_info.func_id;
        let callee_returns_value = !matches!(callee_info.ret, Type::Void);

        let mut sig = Signature::new(self.call_conv);
        sig.params.push(AbiParam::new(self.pointer_ty));

        let frontend_config = self.module.target_config();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let env = builder.block_params(entry)[0];

            let mut gen = FuncGen::new(self, builder, Type::Void);
            let mut args = Vec::with_capacity(pending.arg_types.len());
            for (index, ty) in pending.arg_types.iter().enumerate() {
                let offset = HEADER_SIZE + SLOT_SIZE * index as i32;
                let slot = gen
                    .builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), env, offset);
                args.push(gen.slot_to_value(slot, ty)?);
            }
            let callee_ref = gen.func_ref_by_id(callee_id);
            let call = gen.builder.ins().call(callee_ref, &args);
            if callee_returns_value {
                // A fiber's result has nowhere to go; the call is for its effects.
                let _ = gen.builder.inst_results(call);
            }
            gen.builder.ins().return_(&[]);
            gen.builder.seal_all_blocks();
            gen.builder.finalize(frontend_config);
        }

        self.module
            .define_function(pending.func_id, &mut ctx)
            .map_err(|e| CodegenError::internal(format!("{}: {}", pending.symbol, e)))?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }
}

// ====================================================================== //
// Per-function code generation                                            //
// ====================================================================== //

struct FuncGen<'a, 'b, 'c> {
    l: &'a mut Lowerer<'b>,
    builder: FunctionBuilder<'c>,
    scopes: Vec<HashMap<String, Binding>>,
    loops: Vec<LoopFrame>,
    func_refs: HashMap<FuncId, FuncRef>,
    data_refs: HashMap<DataId, cranelift_codegen::ir::GlobalValue>,
    /// Whether the block currently being filled already has a terminator.
    /// `cranelift-frontend` keeps that state private, and expression lowering
    /// needs it: a block expression can end in `return`.
    terminated: bool,
    return_ty: Type,
}

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    fn new(l: &'a mut Lowerer<'b>, builder: FunctionBuilder<'c>, return_ty: Type) -> Self {
        Self {
            l,
            builder,
            scopes: Vec::new(),
            loops: Vec::new(),
            func_refs: HashMap::new(),
            data_refs: HashMap::new(),
            terminated: false,
            return_ty,
        }
    }

    fn pointer_ty(&self) -> ClifType {
        self.l.pointer_ty
    }

    // ------------------------------------------------------------------ //
    // Scopes                                                              //
    // ------------------------------------------------------------------ //

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare_local(
        &mut self,
        name: &str,
        ty: Type,
        init: Option<Value>,
    ) -> CodegenResult<Variable> {
        let ty = self.l.normalize(ty);
        let repr = repr_of(&ty)?;
        let clif = repr.clif(self.pointer_ty()).ok_or_else(|| {
            CodegenError::unsupported(format!("`{}` would have type `void`", name))
        })?;
        let var = self.builder.declare_var(clif);
        let value = match init {
            Some(value) => value,
            None => self.zero_of(repr),
        };
        self.builder.def_var(var, value);
        self.scopes
            .last_mut()
            .expect("a scope is always open while lowering")
            .insert(name.to_string(), Binding::Local { var, ty });
        Ok(var)
    }

    fn lookup(&self, name: &str) -> Option<Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.get(name) {
                return Some(binding.clone());
            }
        }
        self.l.globals.get(name).cloned().map(Binding::Global)
    }

    fn zero_of(&mut self, repr: Repr) -> Value {
        match repr {
            Repr::Int => self.builder.ins().iconst(types::I64, 0),
            Repr::Float => self.builder.ins().f64const(0.0),
            Repr::Bool => self.builder.ins().iconst(types::I8, 0),
            Repr::Ref => {
                let ptr = self.pointer_ty();
                self.builder.ins().iconst(ptr, 0)
            }
            Repr::Void => unreachable!("void values are never materialised"),
        }
    }

    // ------------------------------------------------------------------ //
    // References to other functions and data                              //
    // ------------------------------------------------------------------ //

    fn func_ref_by_id(&mut self, func_id: FuncId) -> FuncRef {
        if let Some(existing) = self.func_refs.get(&func_id) {
            return *existing;
        }
        let func_ref = self
            .l
            .module
            .declare_func_in_func(func_id, self.builder.func);
        self.func_refs.insert(func_id, func_ref);
        func_ref
    }

    fn func_ref(&mut self, key: &str) -> CodegenResult<FuncRef> {
        let func_id = self
            .l
            .funcs
            .get(key)
            .ok_or_else(|| CodegenError::internal(format!("unknown function `{}`", key)))?
            .func_id;
        Ok(self.func_ref_by_id(func_id))
    }

    /// Declare (once per module) and reference a runtime symbol.
    fn rt_ref(&mut self, name: &str) -> CodegenResult<FuncRef> {
        let func_id = match self.l.runtime_ids.get(name) {
            Some(id) => *id,
            None => {
                let sig = runtime::signature(name, self.l.call_conv, self.l.pointer_ty)?;
                let id = self
                    .l
                    .module
                    .declare_function(name, Linkage::Import, &sig)
                    .map_err(|e| CodegenError::internal(e.to_string()))?;
                self.l.runtime_ids.insert(name.to_string(), id);
                id
            }
        };
        Ok(self.func_ref_by_id(func_id))
    }

    /// Call a runtime function, returning its single result if it has one.
    fn call_rt(&mut self, name: &str, args: &[Value]) -> CodegenResult<Option<Value>> {
        let func_ref = self.rt_ref(name)?;
        let call = self.builder.ins().call(func_ref, args);
        let results = self.builder.inst_results(call);
        Ok(results.first().copied())
    }

    fn call_rt_value(&mut self, name: &str, args: &[Value]) -> CodegenResult<Value> {
        self.call_rt(name, args)?
            .ok_or_else(|| CodegenError::internal(format!("`{}` returned no value", name)))
    }

    /// Switch to `block` and reset the terminator flag, which tracks whether the
    /// block being filled has already been closed.
    fn goto(&mut self, block: Block) {
        self.builder.switch_to_block(block);
        self.terminated = false;
    }

    fn jump_to(&mut self, block: Block, args: &[Value]) {
        let args: Vec<_> = args.iter().map(|v| (*v).into()).collect();
        self.builder.ins().jump(block, &args);
        self.terminated = true;
    }

    fn global_value(&mut self, data_id: DataId) -> cranelift_codegen::ir::GlobalValue {
        if let Some(existing) = self.data_refs.get(&data_id) {
            return *existing;
        }
        let gv = self
            .l
            .module
            .declare_data_in_func(data_id, self.builder.func);
        self.data_refs.insert(data_id, gv);
        gv
    }
}

// ====================================================================== //
// Statements                                                              //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Lower a block in its own scope. Returns true when control flow left the
    /// block (a `return`, `break` or `continue`), so the caller knows not to
    /// append a fall-through jump.
    fn lower_block(&mut self, block: &AstBlock) -> CodegenResult<bool> {
        Ok(self.lower_block_value(block, None)?.1)
    }

    /// Lower a block and return the value of its trailing expression, if it has
    /// one, alongside the terminated flag.
    ///
    /// `let status = if c { "yes" } else { "no" }` puts a block in value
    /// position, so the last expression statement in a block is its result.
    fn lower_block_value(
        &mut self,
        block: &AstBlock,
        expected: Option<&Type>,
    ) -> CodegenResult<(Option<Value>, bool)> {
        self.push_scope();
        let mut terminated = false;
        let mut value = None;
        let last = block.statements.len().saturating_sub(1);
        for (index, stmt) in block.statements.iter().enumerate() {
            if terminated {
                // Unreachable statements are dropped rather than emitted into a
                // block that is already closed.
                break;
            }
            if index == last {
                if let StatementKind::Expression(expr) = &stmt.kind {
                    value = match expected {
                        Some(expected) => self.lower_expr_typed(expr, expected)?,
                        None => self.lower_expr(expr)?,
                    };
                    terminated = self.terminated;
                    break;
                }
            }
            terminated = self.lower_stmt(stmt)?;
        }
        self.pop_scope();
        Ok((value, terminated))
    }

    fn lower_stmt(&mut self, stmt: &Statement) -> CodegenResult<bool> {
        match &stmt.kind {
            StatementKind::VarDecl {
                pattern,
                type_ann,
                initializer,
                ..
            } => {
                let PatternKind::Variable(name) = &pattern.kind else {
                    // `let (a, b) = pair` — the pattern always matches, so bind
                    // it directly rather than routing through a `match`.
                    let Some(init) = initializer else {
                        return Err(CodegenError::unsupported_at(
                            "a destructuring declaration needs an initialiser",
                            &stmt.span,
                        ));
                    };
                    let ty = match type_ann {
                        Some(ann) => self.l.normalize(layout::type_of_ann(ann)),
                        None => self.ty_of(init)?,
                    };
                    let value = self.lower_expr_value(init, &ty)?;
                    self.bind_irrefutable(pattern, value, &ty)?;
                    return Ok(false);
                };

                let declared = match type_ann {
                    Some(ann) => Some(self.l.normalize(layout::type_of_ann(ann))),
                    None => None,
                };
                let (ty, value) = match initializer {
                    Some(init) => {
                        let ty = match declared.clone().filter(|t| !matches!(t, Type::Any)) {
                            Some(annotated) => annotated,
                            None => self.ty_of(init)?,
                        };
                        let value = self.lower_expr_typed(init, &ty)?.ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("`{}` is initialised with a value of type `void`", name),
                                &stmt.span,
                            )
                        })?;
                        (ty, Some(value))
                    }
                    None => {
                        let ty = declared.ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("`{}` needs a type annotation or an initialiser", name),
                                &stmt.span,
                            )
                        })?;
                        (ty, None)
                    }
                };

                // At the top level the name already has a global cell; inside a
                // function it becomes an SSA variable.
                if self.scopes.len() == 1 && self.l.globals.contains_key(name) {
                    let global = self.l.globals[name].clone();
                    if let Some(value) = value {
                        self.store_global(&global, value)?;
                    }
                } else {
                    self.declare_local(name, ty, value)?;
                }
                Ok(false)
            }

            StatementKind::ConstDecl {
                name, initializer, ..
            } => {
                let ty = self.ty_of(initializer)?;
                let value = self.lower_expr_typed(initializer, &ty)?.ok_or_else(|| {
                    CodegenError::unsupported_at("a constant cannot be `void`", &stmt.span)
                })?;
                if self.scopes.len() == 1 && self.l.globals.contains_key(name) {
                    let global = self.l.globals[name].clone();
                    self.store_global(&global, value)?;
                } else {
                    self.declare_local(name, ty, Some(value))?;
                }
                Ok(false)
            }

            StatementKind::Expression(expr) => {
                self.lower_expr_discard(expr)?;
                // An expression statement can end the block: a `select` whose
                // every arm returns, or a block expression containing a
                // `return`. Reporting otherwise would keep emitting into a
                // block that already has a terminator.
                Ok(self.terminated)
            }

            StatementKind::Return(value) => {
                match value {
                    Some(expr) => {
                        let ret_ty = self.return_ty.clone();
                        let value = self.lower_expr_typed(expr, &ret_ty)?;
                        match value {
                            Some(value) => {
                                self.builder.ins().return_(&[value]);
                                self.terminated = true;
                            }
                            None => {
                                self.builder.ins().return_(&[]);
                                self.terminated = true;
                            }
                        }
                    }
                    None => {
                        self.builder.ins().return_(&[]);
                        self.terminated = true;
                    }
                }
                Ok(true)
            }

            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.lower_if(condition, then_branch, else_branch.as_ref()),

            StatementKind::While { condition, body } => {
                let header = self.builder.create_block();
                let body_block = self.builder.create_block();
                let exit = self.builder.create_block();

                self.jump_to(header, &[]);
                self.goto(header);
                let cond = self.lower_condition(condition)?;
                self.builder.ins().brif(cond, body_block, &[], exit, &[]);

                self.goto(body_block);
                self.loops.push(LoopFrame {
                    continue_to: header,
                    exit,
                    exit_used: true, // the loop's own exit edge always reaches it
                });
                let terminated = self.lower_block(body)?;
                self.loops.pop();
                if !terminated {
                    self.jump_to(header, &[]);
                }

                self.goto(exit);
                Ok(false)
            }

            StatementKind::Loop { body } => {
                let header = self.builder.create_block();
                let exit = self.builder.create_block();

                self.jump_to(header, &[]);
                self.goto(header);
                self.loops.push(LoopFrame {
                    continue_to: header,
                    exit,
                    exit_used: false,
                });
                let terminated = self.lower_block(body)?;
                let frame = self.loops.pop().expect("frame pushed above");
                if !terminated {
                    self.jump_to(header, &[]);
                }

                self.goto(exit);
                if !frame.exit_used {
                    // `loop { }` with no `break` never falls out. The block still
                    // has to be filled for the builder to finalise cleanly.
                    self.builder.ins().trap(unreachable_trap());
                    self.terminated = true;
                    return Ok(true);
                }
                Ok(false)
            }

            StatementKind::For {
                variable,
                iterable,
                body,
            } => self.lower_for(variable, iterable, body, &stmt.span),

            StatementKind::Break(value) => {
                if value.is_some() {
                    return Err(CodegenError::unsupported_at(
                        "`break` with a value is not lowered by the native backend yet",
                        &stmt.span,
                    ));
                }
                let frame = self.loops.last_mut().ok_or_else(|| {
                    CodegenError::unsupported_at("`break` outside of a loop", &stmt.span)
                })?;
                frame.exit_used = true;
                let exit = frame.exit;
                self.jump_to(exit, &[]);
                Ok(true)
            }

            StatementKind::Continue => {
                let target = self
                    .loops
                    .last()
                    .ok_or_else(|| {
                        CodegenError::unsupported_at("`continue` outside of a loop", &stmt.span)
                    })?
                    .continue_to;
                self.jump_to(target, &[]);
                Ok(true)
            }

            StatementKind::Block(block) => self.lower_block(block),

            // Declarations were handled by the collection passes.
            _ if is_declaration(stmt) => Ok(false),

            other => Err(CodegenError::unsupported_at(
                format!(
                    "{} is not lowered by the native backend yet",
                    describe(other)
                ),
                &stmt.span,
            )),
        }
    }

    fn lower_if(
        &mut self,
        condition: &Expression,
        then_branch: &AstBlock,
        else_branch: Option<&AstBlock>,
    ) -> CodegenResult<bool> {
        let cond = self.lower_condition(condition)?;
        let then_block = self.builder.create_block();
        let else_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(cond, then_block, &[], else_block, &[]);

        // The merge block is created on demand: when both arms return, there is
        // no fall-through path and an empty block would never be filled.
        let mut merge: Option<Block> = None;

        self.goto(then_block);
        if !self.lower_block(then_branch)? {
            let target = *merge.get_or_insert_with(|| self.builder.create_block());
            self.jump_to(target, &[]);
        }

        self.goto(else_block);
        let else_terminated = match else_branch {
            Some(block) => self.lower_block(block)?,
            None => false,
        };
        if !else_terminated {
            let target = *merge.get_or_insert_with(|| self.builder.create_block());
            self.jump_to(target, &[]);
        }

        match merge {
            Some(merge) => {
                self.goto(merge);
                Ok(false)
            }
            None => Ok(true),
        }
    }

    /// `for x in ...` over an array, or over a `a..b` range written inline.
    fn lower_for(
        &mut self,
        variable: &str,
        iterable: &Expression,
        body: &AstBlock,
        span: &Span,
    ) -> CodegenResult<bool> {
        if let ExpressionKind::Range {
            start,
            end,
            inclusive,
        } = &iterable.kind
        {
            let (Some(start), Some(end)) = (start, end) else {
                return Err(CodegenError::unsupported_at(
                    "an open-ended range cannot be iterated",
                    span,
                ));
            };
            let start = self.lower_expr_value(start, &Type::Int)?;
            let end = self.lower_expr_value(end, &Type::Int)?;
            return self.lower_counted_loop(variable, start, end, *inclusive, body);
        }

        let iter_ty = self.ty_of(iterable)?;

        // A range that reached here through a variable rather than written
        // inline still iterates; read its bounds back out of the object.
        if matches!(&iter_ty, Type::Struct(name) if name == layout::RANGE_TYPE) {
            let range = self.lower_expr_value(iterable, &iter_ty)?;
            let layout = self.range_layout(span)?;
            let start = self.load_at(range, layout.0, &Type::Int)?;
            let end = self.load_at(range, layout.1, &Type::Int)?;
            let inclusive = self.load_at(range, layout.2, &Type::Bool)?;
            return self.lower_dynamic_range_loop(variable, start, end, inclusive, body);
        }

        let Type::Array(element_ty) = iter_ty.clone() else {
            return Err(CodegenError::unsupported_at(
                format!(
                    "cannot iterate a value of type `{}`; the native backend iterates arrays and ranges",
                    iter_ty.display_name()
                ),
                span,
            ));
        };
        let array = self.lower_expr_value(iterable, &iter_ty)?;
        let len = self.call_rt_value("lira_rt_array_len", &[array])?;
        let zero = self.builder.ins().iconst(types::I64, 0);

        self.push_scope();
        let index = self.declare_local("__lira_index", Type::Int, Some(zero))?;
        let element = self.declare_local(variable, (*element_ty).clone(), None)?;

        let header = self.builder.create_block();
        let body_block = self.builder.create_block();
        let step = self.builder.create_block();
        let exit = self.builder.create_block();

        self.jump_to(header, &[]);
        self.goto(header);
        let current = self.builder.use_var(index);
        let more = self.builder.ins().icmp(IntCC::SignedLessThan, current, len);
        self.builder.ins().brif(more, body_block, &[], exit, &[]);

        self.goto(body_block);
        let current = self.builder.use_var(index);
        let slot = self.call_rt_value("lira_rt_array_get", &[array, current])?;
        let value = self.slot_to_value(slot, &element_ty)?;
        self.builder.def_var(element, value);

        self.loops.push(LoopFrame {
            continue_to: step,
            exit,
            exit_used: true,
        });
        let terminated = self.lower_block(body)?;
        self.loops.pop();
        if !terminated {
            self.jump_to(step, &[]);
        }

        self.goto(step);
        let current = self.builder.use_var(index);
        let next = self.builder.ins().iadd_imm_s(current, 1);
        self.builder.def_var(index, next);
        self.jump_to(header, &[]);

        self.goto(exit);
        self.pop_scope();
        Ok(false)
    }

    fn lower_counted_loop(
        &mut self,
        variable: &str,
        start: Value,
        end: Value,
        inclusive: bool,
        body: &AstBlock,
    ) -> CodegenResult<bool> {
        self.push_scope();
        let counter = self.declare_local(variable, Type::Int, Some(start))?;

        let header = self.builder.create_block();
        let body_block = self.builder.create_block();
        let step = self.builder.create_block();
        let exit = self.builder.create_block();

        self.jump_to(header, &[]);
        self.goto(header);
        let current = self.builder.use_var(counter);
        let cc = if inclusive {
            IntCC::SignedLessThanOrEqual
        } else {
            IntCC::SignedLessThan
        };
        let more = self.builder.ins().icmp(cc, current, end);
        self.builder.ins().brif(more, body_block, &[], exit, &[]);

        self.goto(body_block);
        self.loops.push(LoopFrame {
            continue_to: step,
            exit,
            exit_used: true,
        });
        let terminated = self.lower_block(body)?;
        self.loops.pop();
        if !terminated {
            self.jump_to(step, &[]);
        }

        self.goto(step);
        let current = self.builder.use_var(counter);
        let next = self.builder.ins().iadd_imm_s(current, 1);
        self.builder.def_var(counter, next);
        self.jump_to(header, &[]);

        self.goto(exit);
        self.pop_scope();
        Ok(false)
    }

    /// Lower an expression that must produce a `bool`.
    fn lower_condition(&mut self, expr: &Expression) -> CodegenResult<Value> {
        self.lower_expr_value(expr, &Type::Bool)
    }
}

/// A short human-readable name for a statement kind, used in error messages.
fn describe(kind: &StatementKind) -> &'static str {
    match kind {
        StatementKind::VarDecl { .. } => "this declaration",
        StatementKind::Break(_) => "`break`",
        StatementKind::Continue => "`continue`",
        _ => "this statement",
    }
}

fn unreachable_trap() -> cranelift_codegen::ir::TrapCode {
    cranelift_codegen::ir::TrapCode::unwrap_user(1)
}

// ====================================================================== //
// Expressions                                                             //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// The type of an expression, preferring what the backend recorded for a
    /// name it bound itself.
    ///
    /// The checker erases some types the backend has to keep: an enum payload
    /// bound by `Option::Some(x)` is `any` in `expr_types`, but the variant's
    /// declaration says `int`, and native code needs the `int`.
    fn ty_of(&self, expr: &Expression) -> CodegenResult<Type> {
        self.infer_or_checked(expr).map(Ok).unwrap_or_else(|| {
            // Fall through to the shared path so the error carries a span and
            // the literal special cases still apply.
            self.l.ty_of(expr)
        })
    }

    /// Lower an expression and coerce the result to `expected`.
    fn lower_expr_typed(
        &mut self,
        expr: &Expression,
        expected: &Type,
    ) -> CodegenResult<Option<Value>> {
        // In statement position the value is thrown away, and a `match` whose
        // arms are statements has none to give.
        if matches!(expected, Type::Void) {
            self.lower_expr_discard(expr)?;
            return Ok(None);
        }

        // `match` and `if` in value position build a merge block whose parameter
        // has to be the type the context wants. The checker often records `any`
        // for them, so push the expected type down rather than lowering to a
        // pointer and failing to convert it back.
        if !matches!(expected, Type::Any | Type::Unknown) {
            match &expr.kind {
                // `return Result::Ok(x)` — the variant's payload type comes from
                // the `Result<T, E>` the context expects, not from the call.
                ExpressionKind::Call { callee, args, .. } => {
                    if let Some(value) = self.lower_result_construction(callee, args, expected)? {
                        return Ok(Some(value));
                    }
                }
                ExpressionKind::EnumVariant {
                    enum_name,
                    variant_name,
                } if enum_name == layout::RESULT_TYPE => {
                    return self
                        .lower_result_variant(variant_name, None, expected, &expr.span)
                        .map(Some)
                }
                ExpressionKind::Match { subject, arms } => {
                    return self.lower_match(subject, arms, expected, &expr.span)
                }
                ExpressionKind::IfExpr {
                    condition,
                    then_expr,
                    else_expr,
                } => return self.lower_if_expr(condition, then_expr, else_expr, expected),
                _ => {}
            }
        }

        let actual = self.ty_of(expr)?;
        let Some(value) = self.lower_expr(expr)? else {
            return Ok(None);
        };
        self.check_value_type(value, &actual, &expr.span)?;
        Ok(Some(self.coerce(value, &actual, expected, &expr.span)?))
    }

    /// Assert that a lowered value really has the machine type its Lira type
    /// implies.
    ///
    /// `coerce` and every store downstream trust this correspondence. When a
    /// built-in and the checker disagree about a call's result type, the
    /// mismatch would otherwise reinterpret the bits — an `i64` read as an
    /// `f64` — and produce a wrong answer with no diagnostic anywhere. Catching
    /// it here turns that into a loud internal error instead.
    fn check_value_type(&self, value: Value, ty: &Type, span: &Span) -> CodegenResult<()> {
        let Some(expected) = repr_of(ty)?.clif(self.pointer_ty()) else {
            return Ok(());
        };
        let actual = self.builder.func.dfg.value_type(value);
        if actual != expected {
            return Err(CodegenError::internal(format!(
                "{}:{}: lowered a `{}` as {} but the program expects {}",
                span.line,
                span.column,
                ty.display_name(),
                actual,
                expected
            )));
        }
        Ok(())
    }

    /// Lower an expression for its effects, discarding any value.
    fn lower_expr_discard(&mut self, expr: &Expression) -> CodegenResult<()> {
        match &expr.kind {
            ExpressionKind::Match { subject, arms } => {
                self.lower_match(subject, arms, &Type::Void, &expr.span)?;
            }
            ExpressionKind::IfExpr {
                condition,
                then_expr,
                else_expr,
            } => {
                self.lower_if_expr(condition, then_expr, else_expr, &Type::Void)?;
            }
            _ => {
                self.lower_expr(expr)?;
            }
        }
        Ok(())
    }

    fn lower_expr_value(&mut self, expr: &Expression, expected: &Type) -> CodegenResult<Value> {
        self.lower_expr_typed(expr, expected)?.ok_or_else(|| {
            CodegenError::unsupported_at("expected a value, found `void`", &expr.span)
        })
    }

    /// Widen or convert between representations where Lira allows it silently.
    fn coerce(
        &mut self,
        value: Value,
        from: &Type,
        to: &Type,
        span: &Span,
    ) -> CodegenResult<Value> {
        // `T` flows into `T?` implicitly, and back out where the checker has
        // already established the value is present.
        if let Type::Optional(inner) = to {
            if !matches!(from, Type::Optional(_) | Type::Null) {
                return self.wrap_optional(value, from, inner, span);
            }
        }
        if let Type::Optional(inner) = from {
            if !matches!(to, Type::Optional(_)) {
                let unwrapped = self.unwrap_optional(value, inner)?;
                return self.coerce(unwrapped, inner, to, span);
            }
        }

        let from_repr = repr_of(from)?;
        let to_repr = repr_of(to)?;
        if from_repr == to_repr {
            return Ok(value);
        }
        Ok(match (from_repr, to_repr) {
            // `1 + 2.5` and `let x: float = 1` both rely on this.
            (Repr::Int, Repr::Float) => self.builder.ins().fcvt_from_sint(types::F64, value),
            (Repr::Bool, Repr::Int) => self.builder.ins().uextend(types::I64, value),
            _ => {
                return Err(CodegenError::unsupported_at(
                    format!(
                        "cannot convert `{}` to `{}` here",
                        from.display_name(),
                        to.display_name()
                    ),
                    span,
                ))
            }
        })
    }

    fn lower_expr(&mut self, expr: &Expression) -> CodegenResult<Option<Value>> {
        match &expr.kind {
            ExpressionKind::IntLiteral(v) => Ok(Some(self.builder.ins().iconst(types::I64, *v))),
            ExpressionKind::FloatLiteral(v) => Ok(Some(self.builder.ins().f64const(*v))),
            ExpressionKind::BoolLiteral(v) => {
                Ok(Some(self.builder.ins().iconst(types::I8, i64::from(*v))))
            }
            ExpressionKind::CharLiteral(v) => Ok(Some(
                self.builder.ins().iconst(types::I64, i64::from(*v as u32)),
            )),
            ExpressionKind::StringLiteral(s) => Ok(Some(self.string_constant(s)?)),
            ExpressionKind::Null => {
                let ptr = self.pointer_ty();
                Ok(Some(self.builder.ins().iconst(ptr, 0)))
            }

            ExpressionKind::Identifier(name) => {
                if let Some(binding) = self.lookup(name) {
                    return Ok(Some(self.load_binding(&binding)));
                }
                // A bare function name in value position — `apply(double, 3)` —
                // becomes a closure object wrapping it.
                if self.l.funcs.contains_key(name.as_str()) {
                    return self.function_value(&name.clone()).map(Some);
                }
                Err(CodegenError::unsupported_at(
                    format!("unknown name `{}`", name),
                    &expr.span,
                ))
            }

            ExpressionKind::Binary { left, op, right } => {
                self.lower_binary(left, *op, right, &expr.span)
            }
            ExpressionKind::Unary { op, operand } => self.lower_unary(*op, operand, &expr.span),

            ExpressionKind::Call {
                callee,
                args,
                type_args,
            } => {
                if !type_args.is_empty() {
                    return Err(CodegenError::unsupported_at(
                        "explicit type arguments are not lowered by the native backend yet",
                        &expr.span,
                    ));
                }
                self.lower_call(callee, args, &expr.span)
            }

            ExpressionKind::MethodCall {
                receiver,
                method,
                args,
                type_args,
            } => {
                if !type_args.is_empty() {
                    return Err(CodegenError::unsupported_at(
                        "explicit type arguments are not lowered by the native backend yet",
                        &expr.span,
                    ));
                }
                self.lower_method_call(receiver, method, args, &expr.span)
            }

            ExpressionKind::FieldAccess { object, field } => {
                if let Some(value) = self.lower_enum_reflection(object, field)? {
                    return Ok(Some(value));
                }
                let (base, offset, field_ty) = self.field_address(object, field, &expr.span)?;
                Ok(Some(self.load_at(base, offset, &field_ty)?))
            }

            ExpressionKind::Index { object, index } => {
                let object_ty = self.ty_of(object)?;
                match object_ty.clone() {
                    Type::Array(element_ty) => {
                        let array = self.lower_expr_value(object, &object_ty)?;
                        let index = self.lower_expr_value(index, &Type::Int)?;
                        let slot = self.call_rt_value("lira_rt_array_get", &[array, index])?;
                        Ok(Some(self.slot_to_value(slot, &element_ty)?))
                    }
                    Type::Map(_, value_ty) => {
                        let value_ty = self.l.normalize(*value_ty);
                        let map = self.lower_expr_value(object, &object_ty)?;
                        let key = self.lower_expr_value(index, &Type::String)?;
                        let slot = self.call_rt_value("lira_rt_map_get", &[map, key])?;
                        Ok(Some(self.slot_to_value(slot, &value_ty)?))
                    }
                    Type::Tuple(element_types) => {
                        // A tuple is an array underneath, so a constant index
                        // reads the slot at that position's declared type.
                        let ExpressionKind::IntLiteral(position) = index.kind else {
                            return Err(CodegenError::unsupported_at(
                                "a tuple can only be indexed by a literal position",
                                &expr.span,
                            ));
                        };
                        let element_ty = element_types
                            .get(position as usize)
                            .ok_or_else(|| {
                                CodegenError::unsupported_at(
                                    format!("this tuple has no position {}", position),
                                    &expr.span,
                                )
                            })?
                            .clone();
                        let tuple = self.lower_expr_value(object, &object_ty)?;
                        let at = self.builder.ins().iconst(types::I64, position);
                        let slot = self.call_rt_value("lira_rt_array_get", &[tuple, at])?;
                        Ok(Some(self.slot_to_value(slot, &element_ty)?))
                    }
                    other => Err(CodegenError::unsupported_at(
                        format!("cannot index a value of type `{}`", other.display_name()),
                        &expr.span,
                    )),
                }
            }

            ExpressionKind::Array(elements) => {
                let ty = self.ty_of(expr)?;
                let element_ty = match &ty {
                    Type::Array(inner) => (**inner).clone(),
                    _ => {
                        return Err(CodegenError::unsupported_at(
                            "array literal without an array type",
                            &expr.span,
                        ))
                    }
                };
                let capacity = self.builder.ins().iconst(types::I64, elements.len() as i64);
                let array = self.call_rt_value("lira_rt_array_new", &[capacity])?;
                for element in elements {
                    let value = self.lower_expr_value(element, &element_ty)?;
                    let slot = self.value_to_slot(value, &element_ty)?;
                    self.call_rt("lira_rt_array_push", &[array, slot])?;
                }
                Ok(Some(array))
            }

            ExpressionKind::StructLiteral { name, fields } => {
                let name = name.clone().or_else(|| match self.ty_of(expr).ok() {
                    Some(Type::Struct(n)) | Some(Type::Enum(n)) => Some(n),
                    _ => None,
                });
                let Some(name) = name else {
                    return Err(CodegenError::unsupported_at(
                        "anonymous object literals are not lowered by the native backend yet",
                        &expr.span,
                    ));
                };
                self.lower_struct_literal(&name, fields, &expr.span)
                    .map(Some)
            }

            ExpressionKind::IfExpr {
                condition,
                then_expr,
                else_expr,
            } => {
                let result_ty = self.ty_of(expr)?;
                self.lower_if_expr(condition, then_expr, else_expr, &result_ty)
            }

            ExpressionKind::Match { subject, arms } => {
                let result_ty = self.ty_of(expr)?;
                self.lower_match(subject, arms, &result_ty, &expr.span)
            }

            ExpressionKind::Assign { target, value } => {
                let target_ty = self.ty_of(target)?;
                let value = self.lower_expr_value(value, &target_ty)?;
                self.assign_to(target, value, &expr.span)?;
                Ok(Some(value))
            }

            ExpressionKind::CompoundAssign { target, op, value } => {
                let target_ty = self.ty_of(target)?;
                let current = self.lower_expr_value(target, &target_ty)?;
                let combined = self.binary_values(current, &target_ty, *op, value, &expr.span)?;
                self.assign_to(target, combined, &expr.span)?;
                Ok(Some(combined))
            }

            ExpressionKind::Block(block) => {
                let (value, terminated) = self.lower_block_value(block, None)?;
                self.terminated = terminated;
                Ok(value)
            }

            ExpressionKind::Spawn(call) => self.lower_spawn(call, &expr.span).map(Some),

            ExpressionKind::EnumVariant {
                enum_name,
                variant_name,
            } => self
                .lower_enum_construction(enum_name, variant_name, &[], &expr.span)
                .map(Some),

            ExpressionKind::Path { segments } => self.lower_path(segments, &expr.span).map(Some),

            ExpressionKind::Cast { expr: inner, .. } => {
                let target = self.ty_of(expr)?;
                let source = self.ty_of(inner)?;
                let value = self.lower_expr_value(inner, &source)?;
                self.lower_cast(value, &source, &target, &expr.span)
                    .map(Some)
            }

            ExpressionKind::Lambda { params, body } => {
                // The lambda's own recorded type settles the signature. Both the
                // lifted function and every indirect call site have to agree on
                // it, and an indirect call has no way to catch a mismatch.
                let recorded = self.ty_of(expr).ok();
                self.lower_lambda(params, body, recorded.as_ref(), &expr.span)
                    .map(Some)
            }
            ExpressionKind::Map(pairs) => {
                let ty = self.ty_of(expr)?;
                let Type::Map(key_ty, value_ty) = ty else {
                    return Err(CodegenError::unsupported_at(
                        "map literal without a map type",
                        &expr.span,
                    ));
                };
                if !matches!(*key_ty, Type::String) {
                    return Err(CodegenError::unsupported_at(
                        format!(
                            "the native backend keys maps by string, not by `{}`",
                            key_ty.display_name()
                        ),
                        &expr.span,
                    ));
                }
                let value_ty = self.l.normalize(*value_ty);
                let map = self.call_rt_value("lira_rt_map_new", &[])?;
                for (key, value) in pairs {
                    let key = self.lower_expr_value(key, &Type::String)?;
                    let value = self.lower_expr_value(value, &value_ty)?;
                    let slot = self.value_to_slot(value, &value_ty)?;
                    self.call_rt("lira_rt_map_set", &[map, key, slot])?;
                }
                Ok(Some(map))
            }
            ExpressionKind::Tuple(elements) => {
                let ty = self.ty_of(expr)?;
                let Type::Tuple(element_types) = ty else {
                    return Err(CodegenError::unsupported_at(
                        "tuple literal without a tuple type",
                        &expr.span,
                    ));
                };
                if element_types.len() != elements.len() {
                    return Err(CodegenError::internal(
                        "tuple literal arity does not match its type",
                    ));
                }
                let capacity = self.builder.ins().iconst(types::I64, elements.len() as i64);
                let tuple = self.call_rt_value("lira_rt_array_new", &[capacity])?;
                for (element, element_ty) in elements.iter().zip(element_types.iter()) {
                    let element_ty = self.l.normalize(element_ty.clone());
                    let value = self.lower_expr_value(element, &element_ty)?;
                    let slot = self.value_to_slot(value, &element_ty)?;
                    self.call_rt("lira_rt_array_push", &[tuple, slot])?;
                }
                Ok(Some(tuple))
            }
            ExpressionKind::Range {
                start,
                end,
                inclusive,
            } => self
                .lower_range_value(start.as_deref(), end.as_deref(), *inclusive, &expr.span)
                .map(Some),
            ExpressionKind::Try(inner) => self.lower_try(inner, &expr.span).map(Some),
            ExpressionKind::Select(arms) => {
                self.lower_select(arms, &expr.span)?;
                Ok(None)
            }
            ExpressionKind::OptionalAccess { object, field } => self
                .lower_optional_access(object, field, &expr.span)
                .map(Some),
            ExpressionKind::TypeCheck { .. } => Err(CodegenError::unsupported_at(
                "runtime type checks are not lowered by the native backend yet",
                &expr.span,
            )),
        }
    }

    // ------------------------------------------------------------------ //
    // Bindings and memory                                                 //
    // ------------------------------------------------------------------ //

    fn load_binding(&mut self, binding: &Binding) -> Value {
        match binding {
            Binding::Local { var, .. } => self.builder.use_var(*var),
            Binding::Global(global) => {
                let gv = self.global_value(global.data_id);
                let ptr = self.pointer_ty();
                let addr = self.builder.ins().symbol_value(ptr, gv);
                let clif = repr_of(&global.ty)
                    .expect("global types are checked at declaration")
                    .clif(ptr)
                    .expect("globals are never void");
                self.builder
                    .ins()
                    .load(clif, MemFlagsData::trusted(), addr, 0)
            }
        }
    }

    fn store_global(&mut self, global: &GlobalInfo, value: Value) -> CodegenResult<()> {
        let gv = self.global_value(global.data_id);
        let ptr = self.pointer_ty();
        let addr = self.builder.ins().symbol_value(ptr, gv);
        self.builder
            .ins()
            .store(MemFlagsData::trusted(), value, addr, 0);
        Ok(())
    }

    /// Load a field or element whose declared type may be narrower than a
    /// register, widening it on the way in.
    fn load_at(&mut self, base: Value, offset: i32, ty: &Type) -> CodegenResult<Value> {
        let repr = repr_of(ty)?;
        let flags = MemFlagsData::trusted();
        Ok(match repr {
            Repr::Float => self.builder.ins().load(types::F64, flags, base, offset),
            Repr::Bool => self.builder.ins().load(types::I8, flags, base, offset),
            Repr::Ref => {
                let ptr = self.pointer_ty();
                self.builder.ins().load(ptr, flags, base, offset)
            }
            Repr::Int => match storage_size(ty) {
                8 => self.builder.ins().load(types::I64, flags, base, offset),
                4 => {
                    let narrow = self.builder.ins().load(types::I32, flags, base, offset);
                    self.extend_to_i64(narrow, ty)
                }
                2 => {
                    let narrow = self.builder.ins().load(types::I16, flags, base, offset);
                    self.extend_to_i64(narrow, ty)
                }
                _ => {
                    let narrow = self.builder.ins().load(types::I8, flags, base, offset);
                    self.extend_to_i64(narrow, ty)
                }
            },
            Repr::Void => return Err(CodegenError::internal("cannot load a value of type `void`")),
        })
    }

    fn store_at(&mut self, base: Value, offset: i32, ty: &Type, value: Value) -> CodegenResult<()> {
        let repr = repr_of(ty)?;
        let flags = MemFlagsData::trusted();
        let value = match repr {
            Repr::Int => match storage_size(ty) {
                8 => value,
                4 => self.builder.ins().ireduce(types::I32, value),
                2 => self.builder.ins().ireduce(types::I16, value),
                _ => self.builder.ins().ireduce(types::I8, value),
            },
            Repr::Void => {
                return Err(CodegenError::internal(
                    "cannot store a value of type `void`",
                ))
            }
            _ => value,
        };
        self.builder.ins().store(flags, value, base, offset);
        Ok(())
    }

    fn extend_to_i64(&mut self, narrow: Value, ty: &Type) -> Value {
        if is_unsigned(ty) {
            self.builder.ins().uextend(types::I64, narrow)
        } else {
            self.builder.ins().sextend(types::I64, narrow)
        }
    }

    /// Widen a register value into the uniform 8-byte cell used by arrays,
    /// enum payloads and channels.
    fn value_to_slot(&mut self, value: Value, ty: &Type) -> CodegenResult<Value> {
        Ok(match repr_of(ty)? {
            Repr::Int | Repr::Ref => value,
            Repr::Float => self
                .builder
                .ins()
                .bitcast(types::I64, MemFlagsData::new(), value),
            Repr::Bool => self.builder.ins().uextend(types::I64, value),
            Repr::Void => return Err(CodegenError::internal("cannot store a `void` in a slot")),
        })
    }

    /// The inverse of [`Self::value_to_slot`].
    fn slot_to_value(&mut self, slot: Value, ty: &Type) -> CodegenResult<Value> {
        Ok(match repr_of(ty)? {
            Repr::Int | Repr::Ref => slot,
            Repr::Float => self
                .builder
                .ins()
                .bitcast(types::F64, MemFlagsData::new(), slot),
            Repr::Bool => self.builder.ins().ireduce(types::I8, slot),
            Repr::Void => return Err(CodegenError::internal("cannot read a `void` from a slot")),
        })
    }

    /// Emit (and intern) a `LiraStr` in read-only data.
    ///
    /// String literals need no runtime construction: the whole object, header
    /// included, is laid out at compile time.
    fn string_constant(&mut self, text: &str) -> CodegenResult<Value> {
        let data_id = match self.l.strings.get(text) {
            Some(id) => *id,
            None => {
                let bytes = text.as_bytes();
                let mut image = Vec::with_capacity(24 + bytes.len() + 1);
                image.extend_from_slice(&(runtime::KIND_STRING as u32).to_le_bytes());
                image.extend_from_slice(&0u32.to_le_bytes()); // flags
                                                              // A negative refcount marks the object as static: it lives in
                                                              // the binary's read-only data and must never be freed.
                image.extend_from_slice(&(-1i64).to_le_bytes());
                image.extend_from_slice(&(bytes.len() as i64).to_le_bytes());
                image.extend_from_slice(bytes);
                image.push(0);

                let symbol = format!("lira__str__{}", self.l.next_string);
                self.l.next_string += 1;
                let mut description = DataDescription::new();
                description.define(image.into_boxed_slice());
                description.set_align(8);
                let id = self
                    .l
                    .module
                    .declare_data(&symbol, Linkage::Local, false, false)
                    .map_err(|e| CodegenError::internal(e.to_string()))?;
                self.l
                    .module
                    .define_data(id, &description)
                    .map_err(|e| CodegenError::internal(e.to_string()))?;
                self.l.strings.insert(text.to_string(), id);
                id
            }
        };
        let gv = self.global_value(data_id);
        let ptr = self.pointer_ty();
        Ok(self.builder.ins().symbol_value(ptr, gv))
    }
}

// ====================================================================== //
// Operators                                                               //
// ====================================================================== //

fn is_comparison(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
    )
}

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    fn lower_binary(
        &mut self,
        left: &Expression,
        op: BinaryOp,
        right: &Expression,
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        match op {
            // `&&` and `||` must not evaluate the right operand unless they have
            // to, so they get real control flow rather than a bitwise op.
            BinaryOp::And | BinaryOp::Or => {
                return self.lower_short_circuit(left, op, right).map(Some)
            }
            BinaryOp::NullCoalesce => return self.lower_null_coalesce(left, right, span).map(Some),
            _ => {}
        }

        let left_ty = self.ty_of(left)?;
        let right_ty = self.ty_of(right)?;

        // `"n = " + n` is the interpolation desugaring, so string `+` has to
        // accept a non-string operand and stringify it.
        if op == BinaryOp::Add
            && (matches!(left_ty, Type::String) || matches!(right_ty, Type::String))
        {
            let l = self.lower_to_string(left, &left_ty)?;
            let r = self.lower_to_string(right, &right_ty)?;
            return Ok(Some(self.call_rt_value("lira_rt_str_concat", &[l, r])?));
        }

        if matches!(left_ty, Type::String) && matches!(right_ty, Type::String) {
            let l = self.lower_expr_value(left, &Type::String)?;
            let r = self.lower_expr_value(right, &Type::String)?;
            return self.emit_string_comparison(l, r, op, span).map(Some);
        }

        let common = self.common_type(&left_ty, &right_ty, op, span)?;
        let l = self.lower_expr_value(left, &common)?;
        let r = self.lower_expr_value(right, &common)?;
        self.emit_arith(l, r, &common, op, span).map(Some)
    }

    /// Shared by compound assignment, where the left value is already in hand.
    fn binary_values(
        &mut self,
        current: Value,
        current_ty: &Type,
        op: BinaryOp,
        rhs: &Expression,
        span: &Span,
    ) -> CodegenResult<Value> {
        let rhs_ty = self.ty_of(rhs)?;
        if op == BinaryOp::Add && matches!(current_ty, Type::String) {
            let r = self.lower_to_string(rhs, &rhs_ty)?;
            return self.call_rt_value("lira_rt_str_concat", &[current, r]);
        }
        let common = self.common_type(current_ty, &rhs_ty, op, span)?;
        let l = self.coerce(current, current_ty, &common, span)?;
        let r = self.lower_expr_value(rhs, &common)?;
        self.emit_arith(l, r, &common, op, span)
    }

    fn common_type(
        &self,
        left: &Type,
        right: &Type,
        op: BinaryOp,
        span: &Span,
    ) -> CodegenResult<Type> {
        common_type(left, right, op, span)
    }
}

/// The type both operands are converted to before the operation runs.
fn common_type(left: &Type, right: &Type, op: BinaryOp, span: &Span) -> CodegenResult<Type> {
    {
        let left_repr = repr_of(left)?;
        let right_repr = repr_of(right)?;
        Ok(match (left_repr, right_repr) {
            (Repr::Float, _) | (_, Repr::Float) => Type::Float,
            (Repr::Int, Repr::Int) => Type::Int,
            (Repr::Bool, Repr::Bool) => Type::Bool,
            (Repr::Ref, Repr::Ref) if is_comparison(op) => {
                // Comparing two references compares identity; `null` on either
                // side is the common case.
                if matches!(left, Type::Null) {
                    right.clone()
                } else {
                    left.clone()
                }
            }
            _ => {
                return Err(CodegenError::unsupported_at(
                    format!(
                        "`{}` and `{}` cannot be combined by this operator in native code",
                        left.display_name(),
                        right.display_name()
                    ),
                    span,
                ))
            }
        })
    }
}

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    fn emit_arith(
        &mut self,
        l: Value,
        r: Value,
        ty: &Type,
        op: BinaryOp,
        span: &Span,
    ) -> CodegenResult<Value> {
        let repr = repr_of(ty)?;
        let unsigned = is_unsigned(ty) || matches!(repr, Repr::Bool);

        if is_comparison(op) {
            return Ok(match repr {
                Repr::Float => {
                    let cc = match op {
                        BinaryOp::Eq => FloatCC::Equal,
                        BinaryOp::Ne => FloatCC::NotEqual,
                        BinaryOp::Lt => FloatCC::LessThan,
                        BinaryOp::Le => FloatCC::LessThanOrEqual,
                        BinaryOp::Gt => FloatCC::GreaterThan,
                        _ => FloatCC::GreaterThanOrEqual,
                    };
                    self.builder.ins().fcmp(cc, l, r)
                }
                _ => {
                    let cc = match (op, unsigned) {
                        (BinaryOp::Eq, _) => IntCC::Equal,
                        (BinaryOp::Ne, _) => IntCC::NotEqual,
                        (BinaryOp::Lt, false) => IntCC::SignedLessThan,
                        (BinaryOp::Lt, true) => IntCC::UnsignedLessThan,
                        (BinaryOp::Le, false) => IntCC::SignedLessThanOrEqual,
                        (BinaryOp::Le, true) => IntCC::UnsignedLessThanOrEqual,
                        (BinaryOp::Gt, false) => IntCC::SignedGreaterThan,
                        (BinaryOp::Gt, true) => IntCC::UnsignedGreaterThan,
                        (_, false) => IntCC::SignedGreaterThanOrEqual,
                        (_, true) => IntCC::UnsignedGreaterThanOrEqual,
                    };
                    self.builder.ins().icmp(cc, l, r)
                }
            });
        }

        match repr {
            Repr::Float => Ok(match op {
                BinaryOp::Add => self.builder.ins().fadd(l, r),
                BinaryOp::Sub => self.builder.ins().fsub(l, r),
                BinaryOp::Mul => self.builder.ins().fmul(l, r),
                BinaryOp::Div => self.builder.ins().fdiv(l, r),
                _ => {
                    return Err(CodegenError::unsupported_at(
                        format!(
                            "`{:?}` on floats is not lowered by the native backend yet",
                            op
                        ),
                        span,
                    ))
                }
            }),
            Repr::Int => Ok(match op {
                BinaryOp::Add => self.builder.ins().iadd(l, r),
                BinaryOp::Sub => self.builder.ins().isub(l, r),
                BinaryOp::Mul => self.builder.ins().imul(l, r),
                // Division and modulo route through the runtime so a zero
                // divisor reports a Lira error instead of raising SIGFPE.
                BinaryOp::Div => self.call_rt_value("lira_rt_idiv", &[l, r])?,
                BinaryOp::Mod => self.call_rt_value("lira_rt_imod", &[l, r])?,
                BinaryOp::Pow => self.call_rt_value("lira_rt_ipow", &[l, r])?,
                BinaryOp::BitAnd => self.builder.ins().band(l, r),
                BinaryOp::BitOr => self.builder.ins().bor(l, r),
                BinaryOp::BitXor => self.builder.ins().bxor(l, r),
                BinaryOp::Shl => self.builder.ins().ishl(l, r),
                BinaryOp::Shr => self.builder.ins().sshr(l, r),
                BinaryOp::UShr => self.builder.ins().ushr(l, r),
                _ => {
                    return Err(CodegenError::unsupported_at(
                        format!("`{:?}` is not lowered by the native backend yet", op),
                        span,
                    ))
                }
            }),
            Repr::Bool => Ok(match op {
                BinaryOp::BitAnd => self.builder.ins().band(l, r),
                BinaryOp::BitOr => self.builder.ins().bor(l, r),
                BinaryOp::BitXor => self.builder.ins().bxor(l, r),
                _ => {
                    return Err(CodegenError::unsupported_at(
                        format!("`{:?}` on booleans is not supported", op),
                        span,
                    ))
                }
            }),
            _ => Err(CodegenError::unsupported_at(
                format!(
                    "`{:?}` is not defined on `{}` in native code",
                    op,
                    ty.display_name()
                ),
                span,
            )),
        }
    }

    fn emit_string_comparison(
        &mut self,
        l: Value,
        r: Value,
        op: BinaryOp,
        span: &Span,
    ) -> CodegenResult<Value> {
        Ok(match op {
            BinaryOp::Eq => self.call_rt_value("lira_rt_str_eq", &[l, r])?,
            BinaryOp::Ne => {
                let eq = self.call_rt_value("lira_rt_str_eq", &[l, r])?;
                self.builder.ins().icmp_imm_s(IntCC::Equal, eq, 0)
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let order = self.call_rt_value("lira_rt_str_cmp", &[l, r])?;
                let cc = match op {
                    BinaryOp::Lt => IntCC::SignedLessThan,
                    BinaryOp::Le => IntCC::SignedLessThanOrEqual,
                    BinaryOp::Gt => IntCC::SignedGreaterThan,
                    _ => IntCC::SignedGreaterThanOrEqual,
                };
                self.builder.ins().icmp_imm_s(cc, order, 0)
            }
            _ => {
                return Err(CodegenError::unsupported_at(
                    format!("`{:?}` is not defined on strings", op),
                    span,
                ))
            }
        })
    }

    fn lower_short_circuit(
        &mut self,
        left: &Expression,
        op: BinaryOp,
        right: &Expression,
    ) -> CodegenResult<Value> {
        let l = self.lower_condition(left)?;
        let rhs_block = self.builder.create_block();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, types::I8);

        // `a && b` skips `b` when `a` is false; `a || b` skips it when true.
        if op == BinaryOp::And {
            self.builder
                .ins()
                .brif(l, rhs_block, &[], merge, &[l.into()]);
        } else {
            self.builder
                .ins()
                .brif(l, merge, &[l.into()], rhs_block, &[]);
        }
        self.terminated = true;

        self.goto(rhs_block);
        let r = self.lower_condition(right)?;
        self.jump_to(merge, &[r]);

        self.goto(merge);
        Ok(self.builder.block_params(merge)[0])
    }

    fn lower_null_coalesce(
        &mut self,
        left: &Expression,
        right: &Expression,
        span: &Span,
    ) -> CodegenResult<Value> {
        let left_ty = self.ty_of(left)?;
        let _ = span;
        match left_ty.clone() {
            // The result is the unwrapped type: `get_null() ?? 0` is an `int`.
            Type::Optional(inner) => self.lower_coalesce(left, &left_ty, &inner, right),
            // A `null` literal is never present, so the right side always wins.
            Type::Null => {
                let right_ty = self.ty_of(right)?;
                self.lower_expr_value(right, &right_ty)
            }
            // A plain reference is nullable as it stands.
            ty if repr_of(&ty)?.is_ref() => self.lower_coalesce(left, &ty, &ty, right),
            // Anything else can never be null, so the left side always wins.
            ty => self.lower_expr_value(left, &ty),
        }
    }

    /// `a ?? b`, evaluating `b` only when `a` is null.
    ///
    /// `result_ty` is what the expression yields — for `int? ?? 0` that is
    /// `int`, so the present branch unwraps the box.
    fn lower_coalesce(
        &mut self,
        left: &Expression,
        left_ty: &Type,
        result_ty: &Type,
        right: &Expression,
    ) -> CodegenResult<Value> {
        let l = self.lower_expr_value(left, left_ty)?;
        let present = self.builder.create_block();
        let rhs_block = self.builder.create_block();
        let merge = self.builder.create_block();
        let clif = repr_of(result_ty)?
            .clif(self.pointer_ty())
            .ok_or_else(|| CodegenError::internal("`??` cannot produce a `void`"))?;
        self.builder.append_block_param(merge, clif);

        let is_null = self.builder.ins().icmp_imm_s(IntCC::Equal, l, 0);
        self.builder
            .ins()
            .brif(is_null, rhs_block, &[], present, &[]);
        self.terminated = true;

        self.goto(present);
        let unwrapped = self.coerce(l, left_ty, result_ty, &left.span)?;
        self.jump_to(merge, &[unwrapped]);

        self.goto(rhs_block);
        let r = self.lower_expr_value(right, result_ty)?;
        self.jump_to(merge, &[r]);

        self.goto(merge);
        Ok(self.builder.block_params(merge)[0])
    }

    fn lower_unary(
        &mut self,
        op: UnaryOp,
        operand: &Expression,
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        match op {
            UnaryOp::Neg => {
                let ty = self.ty_of(operand)?;
                let value = self.lower_expr_value(operand, &ty)?;
                Ok(Some(match repr_of(&ty)? {
                    Repr::Float => self.builder.ins().fneg(value),
                    Repr::Int => self.builder.ins().ineg(value),
                    _ => {
                        return Err(CodegenError::unsupported_at(
                            format!("cannot negate a `{}`", ty.display_name()),
                            span,
                        ))
                    }
                }))
            }
            UnaryOp::Not => {
                let value = self.lower_condition(operand)?;
                Ok(Some(self.builder.ins().icmp_imm_s(IntCC::Equal, value, 0)))
            }
            UnaryOp::BitNot => {
                let value = self.lower_expr_value(operand, &Type::Int)?;
                Ok(Some(self.builder.ins().bnot(value)))
            }
            UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec => {
                let ty = self.ty_of(operand)?;
                let old = self.lower_expr_value(operand, &ty)?;
                let delta = if matches!(op, UnaryOp::PreInc | UnaryOp::PostInc) {
                    1
                } else {
                    -1
                };
                let new = match repr_of(&ty)? {
                    Repr::Int => self.builder.ins().iadd_imm_s(old, delta),
                    Repr::Float => {
                        let step = self.builder.ins().f64const(delta as f64);
                        self.builder.ins().fadd(old, step)
                    }
                    _ => {
                        return Err(CodegenError::unsupported_at(
                            format!("cannot increment a `{}`", ty.display_name()),
                            span,
                        ))
                    }
                };
                self.assign_to(operand, new, span)?;
                Ok(Some(if matches!(op, UnaryOp::PreInc | UnaryOp::PreDec) {
                    new
                } else {
                    old
                }))
            }
        }
    }

    fn lower_to_string(&mut self, expr: &Expression, ty: &Type) -> CodegenResult<Value> {
        let value = self.lower_expr_value(expr, ty)?;
        self.value_to_string(value, ty, &expr.span)
    }

    fn value_to_string(&mut self, value: Value, ty: &Type, span: &Span) -> CodegenResult<Value> {
        // A boxed optional renders as its payload, or as "null" when absent.
        if let Type::Optional(inner) = ty {
            if optional_is_boxed(inner) {
                return self.optional_to_string(value, inner, span);
            }
        }
        // `null` and `string?` are both the string path: the runtime renders a
        // null pointer as "null".
        let ty = strip_optional(ty);
        Ok(match repr_of(ty)? {
            _ if matches!(ty, Type::String | Type::Null) => value,
            Repr::Int => self.call_rt_value("lira_rt_int_to_str", &[value])?,
            Repr::Float => self.call_rt_value("lira_rt_float_to_str", &[value])?,
            Repr::Bool => self.call_rt_value("lira_rt_bool_to_str", &[value])?,
            _ => {
                return Err(CodegenError::unsupported_at(
                    format!(
                        "the native backend cannot convert a `{}` to a string yet",
                        ty.display_name()
                    ),
                    span,
                ))
            }
        })
    }

    /// Render a boxed optional: its payload when present, "null" when not.
    fn optional_to_string(
        &mut self,
        value: Value,
        inner: &Type,
        span: &Span,
    ) -> CodegenResult<Value> {
        let ptr = self.pointer_ty();
        let present = self.builder.create_block();
        let absent = self.builder.create_block();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, ptr);

        let is_null = self.builder.ins().icmp_imm_s(IntCC::Equal, value, 0);
        self.builder.ins().brif(is_null, absent, &[], present, &[]);
        self.terminated = true;

        self.goto(present);
        let payload = self.unwrap_optional(value, inner)?;
        let rendered = self.value_to_string(payload, inner, span)?;
        self.jump_to(merge, &[rendered]);

        self.goto(absent);
        let null_text = self.string_constant("null")?;
        self.jump_to(merge, &[null_text]);

        self.goto(merge);
        Ok(self.builder.block_params(merge)[0])
    }

    fn lower_cast(
        &mut self,
        value: Value,
        from: &Type,
        to: &Type,
        span: &Span,
    ) -> CodegenResult<Value> {
        if matches!(to, Type::String) {
            return self.value_to_string(value, from, span);
        }
        let from_repr = repr_of(from)?;
        let to_repr = repr_of(to)?;
        Ok(match (from_repr, to_repr) {
            (a, b) if a == b => value,
            (Repr::Int, Repr::Float) => self.builder.ins().fcvt_from_sint(types::F64, value),
            // Saturating rather than trapping: an out-of-range cast clamps.
            (Repr::Float, Repr::Int) => self.builder.ins().fcvt_to_sint_sat(types::I64, value),
            (Repr::Bool, Repr::Int) => self.builder.ins().uextend(types::I64, value),
            _ => {
                return Err(CodegenError::unsupported_at(
                    format!(
                        "cannot cast `{}` to `{}` in native code",
                        from.display_name(),
                        to.display_name()
                    ),
                    span,
                ))
            }
        })
    }
}

// ====================================================================== //
// Calls                                                                   //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    fn lower_call(
        &mut self,
        callee: &Expression,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        match &callee.kind {
            ExpressionKind::Identifier(name) => {
                // A local or parameter holding a function value shadows
                // everything else and is called through its code pointer.
                if let Some(binding) = self.lookup(name) {
                    let ty = match &binding {
                        Binding::Local { ty, .. } => ty.clone(),
                        Binding::Global(global) => global.ty.clone(),
                    };
                    if matches!(ty, Type::Function { .. }) {
                        return self.lower_indirect_call(callee, &ty, args, span);
                    }
                }
                if let Some(result) = self.lower_builtin(name, args, span)? {
                    return Ok(result.into_value());
                }
                if self.l.funcs.contains_key(name.as_str()) {
                    return self.lower_user_call(&name.clone(), None, args, span);
                }
                // The checker lets `impl int { fn abs(self) }` be called as
                // `abs(-5)`, with the receiver as the first argument. The
                // standard library leans on this throughout.
                if let Some(first) = args.first() {
                    let receiver_ty = self.ty_of(&first.value)?;
                    if let Some(key) = self.impl_key_for(&receiver_ty, name) {
                        let self_value = self.lower_expr_value(&first.value, &receiver_ty)?;
                        return self.lower_user_call(&key, Some(self_value), &args[1..], span);
                    }
                }
                Err(CodegenError::unsupported_at(
                    format!("unknown function `{}`", name),
                    span,
                ))
            }
            ExpressionKind::Path { segments } => {
                let [type_name, member] = segments.as_slice() else {
                    return Err(CodegenError::unsupported_at(
                        "only `Type::member` paths are lowered by the native backend",
                        span,
                    ));
                };
                if self.l.layouts.enums.contains_key(type_name) {
                    return self
                        .lower_enum_construction(type_name, member, args, span)
                        .map(Some);
                }
                let key = fn_key(Some(type_name), member);
                self.lower_user_call(&key, None, args, span)
            }
            ExpressionKind::EnumVariant {
                enum_name,
                variant_name,
            } => self
                .lower_enum_construction(enum_name, variant_name, args, span)
                .map(Some),
            ExpressionKind::FieldAccess { object, field } => {
                self.lower_method_call(object, field, args, span)
            }
            // `(|x: int| x * x)(4)` and any other expression that evaluates to
            // a function value.
            _ => {
                let ty = self.ty_of(callee)?;
                if matches!(ty, Type::Function { .. }) {
                    return self.lower_indirect_call(callee, &ty, args, span);
                }
                Err(CodegenError::unsupported_at(
                    format!("`{}` is not callable", ty.display_name()),
                    span,
                ))
            }
        }
    }

    fn lower_method_call(
        &mut self,
        receiver: &Expression,
        method: &str,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        // `Counter.new()` and `Counter::new()` both reach here as a call on a
        // bare type name. A local variable of the same name wins.
        if let ExpressionKind::Identifier(name) = &receiver.kind {
            if self.lookup(name).is_none() && self.l.layouts.is_aggregate(name) {
                if self.l.layouts.enums.contains_key(name) {
                    return self
                        .lower_enum_construction(name, method, args, span)
                        .map(Some);
                }
                let key = fn_key(Some(name), method);
                return self.lower_user_call(&key, None, args, span);
            }
        }

        let receiver_ty = self.ty_of(receiver)?;
        match &receiver_ty {
            Type::Array(element_ty) => {
                let element_ty = (**element_ty).clone();
                // A user `impl [int]` / `impl array` method wins over nothing;
                // the three built-in operations stay built in.
                if let Some(key) = self.builtin_impl_key(&receiver_ty, method) {
                    let self_value = self.lower_expr_value(receiver, &receiver_ty)?;
                    return self.lower_user_call(&key, Some(self_value), args, span);
                }
                let array = self.lower_expr_value(receiver, &receiver_ty)?;
                return self.lower_array_method(array, &element_ty, method, args, span);
            }
            Type::String
            | Type::Int
            | Type::Float
            | Type::Bool
            | Type::Char
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Int64
            | Type::UInt8
            | Type::UInt16
            | Type::UInt32
            | Type::UInt64 => {
                if let Some(key) = self.builtin_impl_key(&receiver_ty, method) {
                    let self_value = self.lower_expr_value(receiver, &receiver_ty)?;
                    return self.lower_user_call(&key, Some(self_value), args, span);
                }
                if matches!(receiver_ty, Type::String) && method == "len" && args.is_empty() {
                    let value = self.lower_expr_value(receiver, &Type::String)?;
                    return Ok(Some(self.call_rt_value("lira_rt_str_len", &[value])?));
                }
                return Err(CodegenError::unsupported_at(
                    format!(
                        "`{}.{}` is not lowered by the native backend yet",
                        receiver_ty.display_name(),
                        method
                    ),
                    span,
                ));
            }
            Type::Struct(name) | Type::Enum(name) => {
                let key = fn_key(Some(name), method);
                if !self.l.funcs.contains_key(&key) {
                    return Err(CodegenError::unsupported_at(
                        format!("`{}` has no method `{}`", name, method),
                        span,
                    ));
                }
                let self_value = self.lower_expr_value(receiver, &receiver_ty)?;
                return self.lower_user_call(&key, Some(self_value), args, span);
            }
            _ => {}
        }

        Err(CodegenError::unsupported_at(
            format!(
                "calling `{}` on a `{}` is not lowered by the native backend yet",
                method,
                receiver_ty.display_name()
            ),
            span,
        ))
    }

    /// The registered key of a user `impl` method on a built-in type, if there
    /// is one. `impl [int]` and `impl array` are both accepted for arrays,
    /// matching how the checker resolves them.
    fn builtin_impl_key(&self, receiver_ty: &Type, method: &str) -> Option<String> {
        for type_name in builtin_impl_names(receiver_ty) {
            let key = fn_key(Some(&type_name), method);
            if self.l.funcs.contains_key(&key) {
                return Some(key);
            }
        }
        None
    }

    /// The key of an instance method callable on `receiver_ty`, whether the
    /// receiver is a built-in type or a user aggregate.
    fn impl_key_for(&self, receiver_ty: &Type, method: &str) -> Option<String> {
        let key = match receiver_ty {
            Type::Struct(name) | Type::Enum(name) | Type::Class(name) => fn_key(Some(name), method),
            other => return self.builtin_impl_key(other, method),
        };
        self.l.funcs.contains_key(&key).then_some(key)
    }

    fn lower_array_method(
        &mut self,
        array: Value,
        element_ty: &Type,
        method: &str,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        match (method, args.len()) {
            ("len", 0) => Ok(Some(self.call_rt_value("lira_rt_array_len", &[array])?)),
            ("push", 1) => {
                let value = self.lower_expr_value(&args[0].value, element_ty)?;
                let slot = self.value_to_slot(value, element_ty)?;
                self.call_rt("lira_rt_array_push", &[array, slot])?;
                Ok(None)
            }
            ("pop", 0) => {
                let slot = self.call_rt_value("lira_rt_array_pop", &[array])?;
                Ok(Some(self.slot_to_value(slot, element_ty)?))
            }
            _ => Err(CodegenError::unsupported_at(
                format!(
                    "`array.{}` is not lowered by the native backend yet",
                    method
                ),
                span,
            )),
        }
    }

    /// Call a user function or method, filling in defaults and reordering
    /// named arguments to match the declaration.
    fn lower_user_call(
        &mut self,
        key: &str,
        self_value: Option<Value>,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        let info = self.l.funcs.get(key).ok_or_else(|| {
            CodegenError::unsupported_at(format!("unknown function `{}`", key), span)
        })?;
        let func_id = info.func_id;
        let ret = info.ret.clone();
        let takes_self =
            info.owner.is_some() && info.params.first().is_some_and(|p| p.name == "self");
        let params: Vec<(String, Type, Option<Expression>)> = info
            .params
            .iter()
            .map(|p| (p.name.clone(), p.ty.clone(), p.default.clone()))
            .collect();

        let explicit = if takes_self {
            &params[1..]
        } else {
            &params[..]
        };
        if takes_self && self_value.is_none() {
            return Err(CodegenError::unsupported_at(
                format!("`{}` is an instance method and needs a receiver", key),
                span,
            ));
        }
        if !takes_self && self_value.is_some() {
            return Err(CodegenError::unsupported_at(
                format!("`{}` is a static method and takes no receiver", key),
                span,
            ));
        }

        let mut slots: Vec<Option<Value>> = vec![None; explicit.len()];
        let mut positional = 0usize;
        for arg in args {
            let index = match &arg.name {
                Some(name) => explicit
                    .iter()
                    .position(|(param, _, _)| param == name)
                    .ok_or_else(|| {
                        CodegenError::unsupported_at(
                            format!("`{}` has no parameter named `{}`", key, name),
                            &arg.span,
                        )
                    })?,
                None => {
                    let index = positional;
                    positional += 1;
                    if index >= explicit.len() {
                        return Err(CodegenError::unsupported_at(
                            format!("too many arguments for `{}`", key),
                            &arg.span,
                        ));
                    }
                    index
                }
            };
            let value = self.lower_expr_value(&arg.value, &explicit[index].1)?;
            slots[index] = Some(value);
        }

        let mut call_args = Vec::with_capacity(params.len());
        if let Some(value) = self_value {
            call_args.push(value);
        }
        for (index, (name, ty, default)) in explicit.iter().enumerate() {
            let value = match slots[index] {
                Some(value) => value,
                None => match default {
                    Some(default) => self.lower_expr_value(default, ty)?,
                    None => {
                        return Err(CodegenError::unsupported_at(
                            format!("missing argument `{}` for `{}`", name, key),
                            span,
                        ))
                    }
                },
            };
            call_args.push(value);
        }

        let func_ref = self.func_ref_by_id(func_id);
        let call = self.builder.ins().call(func_ref, &call_args);
        let results = self.builder.inst_results(call);
        let result = results.first().copied();
        Ok(if matches!(ret, Type::Void) {
            None
        } else {
            result
        })
    }

    /// Lower a call to one of the language's built-in functions.
    ///
    /// Returns `None` when `name` is not a builtin, so the caller can fall back
    /// to user functions.
    fn lower_builtin(
        &mut self,
        name: &str,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<BuiltinResult>> {
        // A program may define a function whose name matches a built-in —
        // `examples/stdlib_demo.li` writes its own `abs` — and the checker
        // resolves the call to that one. The backend has to agree, or it would
        // produce a value of the built-in's type where the rest of the program
        // expects the user function's.
        if self.l.funcs.contains_key(name) {
            return Ok(None);
        }

        let arity_error = |expected: usize| {
            CodegenError::unsupported_at(format!("`{}` takes {} argument(s)", name, expected), span)
        };

        let result = match name {
            "print" | "println" => {
                if args.len() != 1 {
                    return Err(arity_error(1));
                }
                let arg = &args[0].value;
                let arg_ty = self.ty_of(arg)?;
                let value = self.lower_expr_value(arg, &arg_ty)?;
                // A nullable reference prints like the reference it wraps.
                let ty = strip_optional(&arg_ty).clone();
                // The argument's static type picks the runtime entry point, so
                // there is no dispatch at run time.
                //
                // `print` and `println` both terminate the line: the VM's `Print`
                // opcode always appends a newline, and the examples' expected
                // output depends on it. The runtime keeps separate
                // newline-free entry points for when that is fixed.
                let (symbol, value) = match repr_of(&ty)? {
                    _ if matches!(ty, Type::String | Type::Null) => ("lira_rt_println_str", value),
                    Repr::Int => ("lira_rt_println_int", value),
                    Repr::Float => ("lira_rt_println_float", value),
                    Repr::Bool => ("lira_rt_println_bool", value),
                    // An optional renders through the string path, which knows
                    // how to say "null".
                    _ if matches!(arg_ty, Type::Optional(_)) => (
                        "lira_rt_println_str",
                        self.value_to_string(value, &arg_ty, span)?,
                    ),
                    _ => {
                        return Err(CodegenError::unsupported_at(
                            format!(
                                "the native backend cannot print a `{}` yet",
                                ty.display_name()
                            ),
                            span,
                        ))
                    }
                };
                self.call_rt(symbol, &[value])?;
                BuiltinResult::Void
            }

            "len" => {
                if args.len() != 1 {
                    return Err(arity_error(1));
                }
                let arg = &args[0].value;
                let ty = self.ty_of(arg)?;
                let value = self.lower_expr_value(arg, &ty)?;
                let symbol = match ty {
                    Type::String => "lira_rt_str_len",
                    Type::Array(_) | Type::Tuple(_) => "lira_rt_array_len",
                    Type::Map(_, _) => "lira_rt_map_len",
                    other => {
                        return Err(CodegenError::unsupported_at(
                            format!("`len` is not defined on `{}`", other.display_name()),
                            span,
                        ))
                    }
                };
                BuiltinResult::Value(self.call_rt_value(symbol, &[value])?)
            }

            "push" | "pop" => {
                let receiver = args.first().ok_or_else(|| arity_error(1))?;
                let receiver_ty = self.ty_of(&receiver.value)?;
                let Type::Array(element_ty) = receiver_ty.clone() else {
                    return Err(CodegenError::unsupported_at(
                        format!("`{}` expects an array", name),
                        span,
                    ));
                };
                let array = self.lower_expr_value(&receiver.value, &receiver_ty)?;
                let rest = &args[1..];
                match self.lower_array_method(array, &element_ty, name, rest, span)? {
                    Some(value) => BuiltinResult::Value(value),
                    None => BuiltinResult::Void,
                }
            }

            "chan" => {
                if args.len() > 1 {
                    return Err(arity_error(1));
                }
                let capacity = match args.first() {
                    Some(arg) => self.lower_expr_value(&arg.value, &Type::Int)?,
                    None => self.builder.ins().iconst(types::I64, 0),
                };
                BuiltinResult::Value(self.call_rt_value("lira_rt_chan_new", &[capacity])?)
            }

            "send" => {
                if args.len() != 2 {
                    return Err(arity_error(2));
                }
                let channel = self.lower_expr_value(&args[0].value, &Type::Any)?;
                let value_ty = self.ty_of(&args[1].value)?;
                let value = self.lower_expr_value(&args[1].value, &value_ty)?;
                let slot = self.value_to_slot(value, &value_ty)?;
                self.call_rt("lira_rt_chan_send", &[channel, slot])?;
                BuiltinResult::Void
            }

            "recv" => {
                if args.len() != 1 {
                    return Err(arity_error(1));
                }
                let channel = self.lower_expr_value(&args[0].value, &Type::Any)?;
                BuiltinResult::Value(self.call_rt_value("lira_rt_chan_recv", &[channel])?)
            }

            "close" => {
                if args.len() != 1 {
                    return Err(arity_error(1));
                }
                let channel = self.lower_expr_value(&args[0].value, &Type::Any)?;
                self.call_rt("lira_rt_chan_close", &[channel])?;
                BuiltinResult::Void
            }

            "fiber_yield" => {
                self.call_rt("lira_rt_yield", &[])?;
                BuiltinResult::Void
            }
            "fiber_id" => BuiltinResult::Value(self.call_rt_value("lira_rt_fiber_id", &[])?),

            // The native heap does not reclaim yet, so an explicit collection
            // has nothing to do. Accepting it keeps portable programs building.
            "collect" => BuiltinResult::Void,

            _ => {
                if let Some(value) = self.lower_math_builtin(name, args, span)? {
                    return Ok(Some(BuiltinResult::Value(value)));
                }
                let Some(builtin) = runtime::builtin(name) else {
                    return Ok(None);
                };
                if args.len() != builtin.params.len() {
                    return Err(arity_error(builtin.params.len()));
                }
                let mut values = Vec::with_capacity(args.len());
                for (arg, param) in args.iter().zip(builtin.params) {
                    values.push(self.lower_expr_value(&arg.value, &param.lira_type())?);
                }
                match self.call_rt(builtin.symbol, &values)? {
                    Some(value) => BuiltinResult::Value(value),
                    None => BuiltinResult::Void,
                }
            }
        };
        Ok(Some(result))
    }

    /// Math built-ins that lower to a single machine instruction rather than a
    /// runtime call.
    ///
    /// Returns `Ok(None)` when `name` is not one of them.
    fn lower_math_builtin(
        &mut self,
        name: &str,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        if !matches!(
            name,
            "sqrt" | "abs" | "floor" | "ceil" | "trunc" | "is_nan" | "is_infinite" | "is_finite"
        ) {
            return Ok(None);
        }
        let Some(arg) = args.first() else {
            return Ok(None);
        };
        if args.len() != 1 {
            return Ok(None);
        }
        let arg_ty = self.ty_of(&arg.value)?;

        // Every one of these is `(float) -> float` or `(float) -> bool` in the
        // checker, `abs` included. The standard library also declares
        // `impl int { fn abs(self) -> int }`, but a bare `abs(-5)` resolves to
        // the built-in, so an integer argument widens rather than taking an
        // integer path: the value produced here must have the type the rest of
        // the program was told to expect.
        let _ = arg_ty;
        let value = self.lower_expr_value(&arg.value, &Type::Float)?;
        let _ = span;
        Ok(Some(match name {
            "sqrt" => self.builder.ins().sqrt(value),
            "abs" => self.builder.ins().fabs(value),
            "floor" => self.builder.ins().floor(value),
            "ceil" => self.builder.ins().ceil(value),
            "trunc" => self.builder.ins().trunc(value),
            // NaN is the only value that compares unordered with itself.
            "is_nan" => self.builder.ins().fcmp(FloatCC::NotEqual, value, value),
            "is_infinite" | "is_finite" => {
                let magnitude = self.builder.ins().fabs(value);
                let infinity = self.builder.ins().f64const(f64::INFINITY);
                let cc = if name == "is_infinite" {
                    FloatCC::Equal
                } else {
                    FloatCC::LessThan
                };
                self.builder.ins().fcmp(cc, magnitude, infinity)
            }
            _ => unreachable!("guarded by the match above"),
        }))
    }
}

/// What a builtin produced: either a value or nothing at all.
enum BuiltinResult {
    Value(Value),
    Void,
}

impl BuiltinResult {
    fn into_value(self) -> Option<Value> {
        match self {
            BuiltinResult::Value(value) => Some(value),
            BuiltinResult::Void => None,
        }
    }
}

// ====================================================================== //
// Aggregates                                                              //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    fn alloc_object(&mut self, size: i32, kind: i64) -> CodegenResult<Value> {
        let size = self.builder.ins().iconst(types::I64, i64::from(size));
        let kind = self.builder.ins().iconst(types::I32, kind);
        self.call_rt_value("lira_rt_alloc", &[size, kind])
    }

    fn lower_struct_literal(
        &mut self,
        name: &str,
        fields: &[(String, Expression)],
        span: &Span,
    ) -> CodegenResult<Value> {
        let layout = self
            .l
            .layouts
            .structs
            .get(name)
            .ok_or_else(|| {
                CodegenError::unsupported_at(format!("unknown struct `{}`", name), span)
            })?
            .clone();

        let object = self.alloc_object(layout.size, runtime::KIND_STRUCT)?;
        let mut initialised = HashSet::new();
        for (field_name, expr) in fields {
            let field = layout.field(field_name).ok_or_else(|| {
                CodegenError::unsupported_at(
                    format!("`{}` has no field `{}`", name, field_name),
                    &expr.span,
                )
            })?;
            let field_ty = self.l.normalize(field.ty.clone());
            let offset = field.offset;
            let value = self.lower_expr_value(expr, &field_ty)?;
            self.store_at(object, offset, &field_ty, value)?;
            initialised.insert(field_name.clone());
        }
        // `lira_rt_alloc` zeroes, so an omitted field reads as 0/false/null
        // rather than as garbage; the checker is what rejects real omissions.
        Ok(object)
    }

    /// Resolve `object.field` to a base pointer, a byte offset and the field's
    /// type.
    fn field_address(
        &mut self,
        object: &Expression,
        field: &str,
        span: &Span,
    ) -> CodegenResult<(Value, i32, Type)> {
        let object_ty = self.ty_of(object)?;
        let (Type::Struct(name) | Type::Class(name)) = object_ty.clone() else {
            return Err(CodegenError::unsupported_at(
                format!(
                    "cannot read field `{}` from a `{}`",
                    field,
                    object_ty.display_name()
                ),
                span,
            ));
        };
        let layout = self
            .l
            .layouts
            .structs
            .get(&name)
            .ok_or_else(|| CodegenError::unsupported_at(format!("unknown type `{}`", name), span))?
            .clone();
        let field_layout = layout.field(field).ok_or_else(|| {
            CodegenError::unsupported_at(format!("`{}` has no field `{}`", name, field), span)
        })?;
        let offset = field_layout.offset;
        let field_ty = self.l.normalize(field_layout.ty.clone());
        let base = self.lower_expr_value(object, &object_ty)?;
        Ok((base, offset, field_ty))
    }

    fn lower_enum_construction(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Value> {
        let layout = self
            .l
            .layouts
            .enums
            .get(enum_name)
            .ok_or_else(|| {
                CodegenError::unsupported_at(format!("unknown enum `{}`", enum_name), span)
            })?
            .clone();
        let variant = layout.variant(variant_name).ok_or_else(|| {
            CodegenError::unsupported_at(
                format!("`{}` has no variant `{}`", enum_name, variant_name),
                span,
            )
        })?;
        if args.len() != variant.field_types.len() {
            return Err(CodegenError::unsupported_at(
                format!(
                    "`{}::{}` takes {} value(s)",
                    enum_name,
                    variant_name,
                    variant.field_types.len()
                ),
                span,
            ));
        }
        let tag = variant.tag;
        let field_types: Vec<Type> = variant
            .field_types
            .iter()
            .map(|ty| self.l.normalize(ty.clone()))
            .collect();

        let object = self.alloc_object(layout.size, runtime::KIND_ENUM)?;
        let tag_value = self.builder.ins().iconst(types::I64, tag);
        self.builder
            .ins()
            .store(MemFlagsData::trusted(), tag_value, object, ENUM_TAG_OFFSET);
        for (index, (arg, ty)) in args.iter().zip(field_types.iter()).enumerate() {
            let value = self.lower_expr_value(&arg.value, ty)?;
            let slot = self.value_to_slot(value, ty)?;
            let offset = ENUM_PAYLOAD_OFFSET + SLOT_SIZE * index as i32;
            self.builder
                .ins()
                .store(MemFlagsData::trusted(), slot, object, offset);
        }
        Ok(object)
    }

    fn lower_path(&mut self, segments: &[String], span: &Span) -> CodegenResult<Value> {
        let [type_name, member] = segments else {
            return Err(CodegenError::unsupported_at(
                "only `Type::member` paths are lowered by the native backend",
                span,
            ));
        };
        if self.l.layouts.enums.contains_key(type_name) {
            return self.lower_enum_construction(type_name, member, &[], span);
        }
        Err(CodegenError::unsupported_at(
            format!(
                "`{}::{}` is not a value the native backend can produce",
                type_name, member
            ),
            span,
        ))
    }

    // ------------------------------------------------------------------ //
    // Assignment                                                          //
    // ------------------------------------------------------------------ //

    fn assign_to(&mut self, target: &Expression, value: Value, span: &Span) -> CodegenResult<()> {
        match &target.kind {
            ExpressionKind::Identifier(name) => {
                let binding = self.lookup(name).ok_or_else(|| {
                    CodegenError::unsupported_at(format!("unknown name `{}`", name), span)
                })?;
                match binding {
                    Binding::Local { var, .. } => {
                        self.builder.def_var(var, value);
                        Ok(())
                    }
                    Binding::Global(global) => self.store_global(&global, value),
                }
            }
            ExpressionKind::FieldAccess { object, field } => {
                let (base, offset, field_ty) = self.field_address(object, field, span)?;
                self.store_at(base, offset, &field_ty, value)
            }
            ExpressionKind::Index { object, index } => {
                let object_ty = self.ty_of(object)?;
                match object_ty.clone() {
                    Type::Array(element_ty) => {
                        let array = self.lower_expr_value(object, &object_ty)?;
                        let index = self.lower_expr_value(index, &Type::Int)?;
                        let slot = self.value_to_slot(value, &element_ty)?;
                        self.call_rt("lira_rt_array_set", &[array, index, slot])?;
                        Ok(())
                    }
                    Type::Map(_, value_ty) => {
                        let value_ty = self.l.normalize(*value_ty);
                        let map = self.lower_expr_value(object, &object_ty)?;
                        let key = self.lower_expr_value(index, &Type::String)?;
                        let slot = self.value_to_slot(value, &value_ty)?;
                        self.call_rt("lira_rt_map_set", &[map, key, slot])?;
                        Ok(())
                    }
                    other => Err(CodegenError::unsupported_at(
                        format!(
                            "cannot assign through an index into a `{}`",
                            other.display_name()
                        ),
                        span,
                    )),
                }
            }
            _ => Err(CodegenError::unsupported_at(
                "this is not something the native backend can assign to",
                span,
            )),
        }
    }

    // ------------------------------------------------------------------ //
    // Conditional expressions                                             //
    // ------------------------------------------------------------------ //

    fn lower_if_expr(
        &mut self,
        condition: &Expression,
        then_expr: &Expression,
        else_expr: &Expression,
        result_ty: &Type,
    ) -> CodegenResult<Option<Value>> {
        let cond = self.lower_condition(condition)?;
        let then_block = self.builder.create_block();
        let else_block = self.builder.create_block();
        let merge = self.builder.create_block();
        let result_repr = repr_of(result_ty)?;
        let result_clif = result_repr.clif(self.pointer_ty());
        if let Some(clif) = result_clif {
            self.builder.append_block_param(merge, clif);
        }

        self.builder
            .ins()
            .brif(cond, then_block, &[], else_block, &[]);
        self.terminated = true;

        let mut merge_reached = false;
        for (block, expr) in [(then_block, then_expr), (else_block, else_expr)] {
            self.goto(block);
            let value = self.lower_expr_typed(expr, result_ty)?;
            if !self.terminated {
                merge_reached = true;
                match value {
                    Some(value) => self.jump_to(merge, &[value]),
                    None => self.jump_to(merge, &[]),
                }
            }
        }

        self.goto(merge);
        if !merge_reached {
            self.builder.ins().trap(unreachable_trap());
            self.terminated = true;
        }
        Ok(result_clif.map(|_| self.builder.block_params(merge)[0]))
    }
}

// ====================================================================== //
// Pattern matching                                                        //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Lower a `match` into a chain of tests.
    ///
    /// Each arm gets a "fail" block that the next arm starts from, so the arms
    /// are tried in source order. Enum arms compare the discriminant loaded from
    /// the object header; literal arms compare the value directly.
    fn lower_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        result_ty: &Type,
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        let subject_ty = self.ty_of(subject)?;
        let subject_value = self.lower_expr_value(subject, &subject_ty)?;

        let merge = self.builder.create_block();
        let result_repr = repr_of(result_ty)?;
        let result_clif = result_repr.clif(self.pointer_ty());
        if let Some(clif) = result_clif {
            self.builder.append_block_param(merge, clif);
        }

        let mut merge_reached = false;
        for arm in arms {
            let fail = self.builder.create_block();
            self.push_scope();
            self.test_pattern(&arm.pattern, subject_value, &subject_ty, fail)?;

            if let Some(guard) = &arm.guard {
                let body_block = self.builder.create_block();
                let ok = self.lower_condition(guard)?;
                self.builder.ins().brif(ok, body_block, &[], fail, &[]);
                self.terminated = true;
                self.goto(body_block);
            }

            let value = self.lower_expr_typed(&arm.body, result_ty)?;
            if !self.terminated {
                merge_reached = true;
                match value {
                    Some(value) => self.jump_to(merge, &[value]),
                    None => self.jump_to(merge, &[]),
                }
            }
            self.pop_scope();
            self.goto(fail);
        }

        // Falling past the last arm means the checker's exhaustiveness analysis
        // was not able to prove coverage. Report it rather than continue with an
        // undefined value.
        let message = self.string_constant("no match arm matched")?;
        self.call_rt("lira_rt_abort", &[message])?;
        self.builder.ins().trap(unreachable_trap());
        self.terminated = true;

        self.goto(merge);
        if !merge_reached {
            // Every arm returned or broke out, so nothing falls through. The
            // block still needs a terminator for the builder to finalise.
            self.builder.ins().trap(unreachable_trap());
            self.terminated = true;
        }
        let _ = span;
        Ok(result_clif.map(|_| self.builder.block_params(merge)[0]))
    }

    /// Emit the tests for one pattern, branching to `fail` on any mismatch, and
    /// bind the pattern's variables into the current scope.
    ///
    /// On return the builder sits in a block reached only when the pattern
    /// matched.
    fn test_pattern(
        &mut self,
        pattern: &Pattern,
        subject: Value,
        subject_ty: &Type,
        fail: Block,
    ) -> CodegenResult<()> {
        match &pattern.kind {
            PatternKind::Wildcard => Ok(()),

            PatternKind::Variable(name) => {
                self.declare_local(name, subject_ty.clone(), Some(subject))?;
                Ok(())
            }

            PatternKind::Binding { name, pattern } => {
                self.declare_local(name, subject_ty.clone(), Some(subject))?;
                self.test_pattern(pattern, subject, subject_ty, fail)
            }

            PatternKind::Literal(expr) => {
                let matched = self.compare_with_literal(subject, subject_ty, expr)?;
                self.branch_on(matched, fail);
                Ok(())
            }

            PatternKind::Range {
                start,
                end,
                inclusive,
            } => {
                let PatternKind::Literal(start_expr) = &start.kind else {
                    return Err(CodegenError::unsupported_at(
                        "range patterns need literal bounds",
                        &pattern.span,
                    ));
                };
                let PatternKind::Literal(end_expr) = &end.kind else {
                    return Err(CodegenError::unsupported_at(
                        "range patterns need literal bounds",
                        &pattern.span,
                    ));
                };
                let low = self.lower_expr_value(start_expr, subject_ty)?;
                let high = self.lower_expr_value(end_expr, subject_ty)?;
                let unsigned = is_unsigned(subject_ty);
                let ge = self.builder.ins().icmp(
                    if unsigned {
                        IntCC::UnsignedGreaterThanOrEqual
                    } else {
                        IntCC::SignedGreaterThanOrEqual
                    },
                    subject,
                    low,
                );
                self.branch_on(ge, fail);
                let cc = match (*inclusive, unsigned) {
                    (true, false) => IntCC::SignedLessThanOrEqual,
                    (true, true) => IntCC::UnsignedLessThanOrEqual,
                    (false, false) => IntCC::SignedLessThan,
                    (false, true) => IntCC::UnsignedLessThan,
                };
                let le = self.builder.ins().icmp(cc, subject, high);
                self.branch_on(le, fail);
                Ok(())
            }

            PatternKind::Or(alternatives) => {
                // Every alternative must succeed on its own, and the bindings
                // they introduce would have to agree; only binding-free
                // alternatives are lowered.
                for alternative in alternatives {
                    if pattern_binds(alternative) {
                        return Err(CodegenError::unsupported_at(
                            "an or-pattern that binds variables is not lowered by the native backend yet",
                            &pattern.span,
                        ));
                    }
                }
                let matched = self.builder.create_block();
                for alternative in alternatives {
                    let next = self.builder.create_block();
                    self.test_pattern(alternative, subject, subject_ty, next)?;
                    self.jump_to(matched, &[]);
                    self.goto(next);
                }
                // Every alternative missed.
                self.jump_to(fail, &[]);
                self.goto(matched);
                Ok(())
            }

            PatternKind::Constructor { name, fields } => {
                self.test_constructor(name, fields, subject, subject_ty, fail, &pattern.span)
            }

            PatternKind::Struct {
                name,
                fields,
                rest: _,
            } => {
                // `let { x, y } = point` leaves the name empty: the type comes
                // from the subject rather than from the pattern.
                let type_name = if name.is_empty() {
                    match subject_ty {
                        Type::Struct(subject_name) | Type::Class(subject_name) => {
                            subject_name.as_str()
                        }
                        other => {
                            return Err(CodegenError::unsupported_at(
                                format!(
                                    "a struct pattern cannot match a `{}`",
                                    other.display_name()
                                ),
                                &pattern.span,
                            ))
                        }
                    }
                } else {
                    name.as_str()
                };
                let layout = self
                    .l
                    .layouts
                    .structs
                    .get(type_name)
                    .ok_or_else(|| {
                        CodegenError::unsupported_at(
                            format!("unknown struct `{}` in pattern", type_name),
                            &pattern.span,
                        )
                    })?
                    .clone();
                for (field_name, sub_pattern) in fields {
                    let field = layout.field(field_name).ok_or_else(|| {
                        CodegenError::unsupported_at(
                            format!("`{}` has no field `{}`", type_name, field_name),
                            &pattern.span,
                        )
                    })?;
                    let field_ty = self.l.normalize(field.ty.clone());
                    let offset = field.offset;
                    let value = self.load_at(subject, offset, &field_ty)?;
                    self.test_pattern(sub_pattern, value, &field_ty, fail)?;
                }
                Ok(())
            }

            PatternKind::Tuple(elements) => {
                let Type::Tuple(element_types) = subject_ty else {
                    return Err(CodegenError::unsupported_at(
                        format!(
                            "a tuple pattern cannot match a `{}`",
                            subject_ty.display_name()
                        ),
                        &pattern.span,
                    ));
                };
                if elements.len() != element_types.len() {
                    return Err(CodegenError::unsupported_at(
                        format!(
                            "this pattern binds {} element(s), but the tuple has {}",
                            elements.len(),
                            element_types.len()
                        ),
                        &pattern.span,
                    ));
                }
                for (index, (sub_pattern, element_ty)) in
                    elements.iter().zip(element_types.iter()).enumerate()
                {
                    let element_ty = self.l.normalize(element_ty.clone());
                    let position = self.builder.ins().iconst(types::I64, index as i64);
                    let slot = self.call_rt_value("lira_rt_array_get", &[subject, position])?;
                    let value = self.slot_to_value(slot, &element_ty)?;
                    self.test_pattern(sub_pattern, value, &element_ty, fail)?;
                }
                Ok(())
            }
        }
    }

    fn test_constructor(
        &mut self,
        name: &str,
        fields: &[Pattern],
        subject: Value,
        subject_ty: &Type,
        fail: Block,
        span: &Span,
    ) -> CodegenResult<()> {
        // Patterns spell variants either fully (`Option::Some`) or bare
        // (`Some`); the enum comes from the subject's type either way.
        let variant_name = name.rsplit("::").next().unwrap_or(name);
        if let Type::Result { ok_type, err_type } = subject_ty {
            let (ok_type, err_type) = ((**ok_type).clone(), (**err_type).clone());
            return self.test_result_constructor(
                variant_name,
                fields,
                subject,
                (&ok_type, &err_type),
                fail,
                span,
            );
        }
        let Type::Enum(enum_name) = subject_ty else {
            return Err(CodegenError::unsupported_at(
                format!(
                    "`{}` is a variant pattern, but the subject is a `{}`",
                    name,
                    subject_ty.display_name()
                ),
                span,
            ));
        };
        let layout = self
            .l
            .layouts
            .enums
            .get(enum_name)
            .ok_or_else(|| {
                CodegenError::unsupported_at(format!("unknown enum `{}`", enum_name), span)
            })?
            .clone();
        let variant = layout.variant(variant_name).ok_or_else(|| {
            CodegenError::unsupported_at(
                format!("`{}` has no variant `{}`", enum_name, variant_name),
                span,
            )
        })?;
        if !fields.is_empty() && fields.len() != variant.field_types.len() {
            return Err(CodegenError::unsupported_at(
                format!(
                    "`{}::{}` binds {} value(s)",
                    enum_name,
                    variant_name,
                    variant.field_types.len()
                ),
                span,
            ));
        }
        let tag = variant.tag;
        let field_types: Vec<Type> = variant
            .field_types
            .iter()
            .map(|ty| self.l.normalize(ty.clone()))
            .collect();

        let actual = self.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            subject,
            ENUM_TAG_OFFSET,
        );
        let matched = self.builder.ins().icmp_imm_s(IntCC::Equal, actual, tag);
        self.branch_on(matched, fail);

        for (index, (sub_pattern, ty)) in fields.iter().zip(field_types.iter()).enumerate() {
            let offset = ENUM_PAYLOAD_OFFSET + SLOT_SIZE * index as i32;
            let slot =
                self.builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), subject, offset);
            let value = self.slot_to_value(slot, ty)?;
            self.test_pattern(sub_pattern, value, ty, fail)?;
        }
        Ok(())
    }

    /// Bind a pattern that is guaranteed to match, as in `let (a, b) = pair`.
    ///
    /// The checker has already established that the shapes line up, so the
    /// mismatch path is unreachable; it still needs a block, and reporting the
    /// failure beats leaving the bindings undefined.
    fn bind_irrefutable(
        &mut self,
        pattern: &Pattern,
        value: Value,
        ty: &Type,
    ) -> CodegenResult<()> {
        let fail = self.builder.create_block();
        self.test_pattern(pattern, value, ty, fail)?;
        let matched = self.builder.create_block();
        self.jump_to(matched, &[]);

        self.goto(fail);
        let message = self.string_constant("destructuring pattern did not match")?;
        self.call_rt("lira_rt_abort", &[message])?;
        self.builder.ins().trap(unreachable_trap());
        self.terminated = true;

        self.goto(matched);
        Ok(())
    }

    /// Continue in a fresh block when `condition` holds, jump to `fail` when it
    /// does not.
    fn branch_on(&mut self, condition: Value, fail: Block) {
        let ok = self.builder.create_block();
        self.builder.ins().brif(condition, ok, &[], fail, &[]);
        self.terminated = true;
        self.goto(ok);
    }

    fn compare_with_literal(
        &mut self,
        subject: Value,
        subject_ty: &Type,
        literal: &Expression,
    ) -> CodegenResult<Value> {
        if matches!(subject_ty, Type::String) {
            let expected = self.lower_expr_value(literal, &Type::String)?;
            return self.call_rt_value("lira_rt_str_eq", &[subject, expected]);
        }
        let expected = self.lower_expr_value(literal, subject_ty)?;
        Ok(match repr_of(subject_ty)? {
            Repr::Float => self.builder.ins().fcmp(FloatCC::Equal, subject, expected),
            Repr::Void => return Err(CodegenError::internal("cannot match against `void`")),
            _ => self.builder.ins().icmp(IntCC::Equal, subject, expected),
        })
    }
}

/// Result type of `receiver.method(...)`.
fn method_call_type(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    receiver: &Expression,
    method: &str,
) -> Option<Type> {
    // `Counter.new()` — a call on a bare type name.
    if let ExpressionKind::Identifier(name) = &receiver.kind {
        if resolve(name).is_none() && l.layouts.is_aggregate(name) {
            if l.layouts.enums.contains_key(name) {
                return Some(l.user_type(name));
            }
            return Some(l.funcs.get(&fn_key(Some(name), method))?.ret.clone());
        }
    }
    let receiver_ty = infer_or_checked_with(l, resolve, receiver)?;
    // A user `impl` block wins wherever one exists — including `impl int` and
    // `impl string`, which is how the standard library defines most of its
    // methods on primitive types.
    if let Some(ret) = impl_method_return(l, &receiver_ty, method) {
        return Some(ret);
    }
    Some(match receiver_ty {
        Type::Array(inner) => match method {
            "len" => Type::Int,
            "pop" => *inner,
            "push" => Type::Void,
            _ => return None,
        },
        Type::String if method == "len" => Type::Int,
        _ => return None,
    })
}

/// Return type of an instance method declared on `receiver_ty`, if one exists.
fn impl_method_return(l: &Lowerer<'_>, receiver_ty: &Type, method: &str) -> Option<Type> {
    let names = match receiver_ty {
        Type::Struct(name) | Type::Enum(name) | Type::Class(name) => vec![name.clone()],
        other => builtin_impl_names(other),
    };
    names
        .iter()
        .find_map(|type_name| l.funcs.get(&fn_key(Some(type_name), method)))
        .map(|info| info.ret.clone())
}

/// Type names an `impl` block can use for a built-in receiver, most specific
/// first.
fn builtin_impl_names(ty: &Type) -> Vec<String> {
    match ty {
        Type::Array(inner) => vec![format!("[{}]", inner.display_name()), "array".to_string()],
        other => vec![other.display_name()],
    }
}

/// The element type of an array `impl` name such as `[int]`.
///
/// Nests, so `impl [[int]]` — which the standard library uses — resolves to an
/// array of arrays rather than to a struct named `[int]`.
fn array_impl_element(name: &str) -> Option<Type> {
    let inner = name.strip_prefix('[')?.strip_suffix(']')?;
    if let Some(element) = array_impl_element(inner) {
        return Some(Type::Array(Box::new(element)));
    }
    layout::primitive_type(inner).or_else(|| Some(Type::Struct(inner.to_string())))
}

/// See through `T?` when `T` is already pointer-shaped, which is the only form
/// of optional the backend represents.
fn strip_optional(ty: &Type) -> &Type {
    match ty {
        Type::Optional(inner) if repr_of(inner).is_ok_and(|r| r.is_ref()) => inner,
        other => other,
    }
}

/// Drop the types that carry no information for code generation.
fn concrete(ty: Type) -> Option<Type> {
    match ty {
        Type::Any | Type::Unknown | Type::TypeVar(_) | Type::Void => None,
        other => Some(other),
    }
}

/// Whether a pattern introduces any variable bindings.
fn pattern_binds(pattern: &Pattern) -> bool {
    match &pattern.kind {
        PatternKind::Variable(_) | PatternKind::Binding { .. } => true,
        PatternKind::Wildcard | PatternKind::Literal(_) => false,
        PatternKind::Tuple(items) | PatternKind::Or(items) => items.iter().any(pattern_binds),
        PatternKind::Constructor { fields, .. } => fields.iter().any(pattern_binds),
        PatternKind::Struct { fields, .. } => {
            fields.iter().any(|(_, pattern)| pattern_binds(pattern))
        }
        PatternKind::Range { .. } => false,
    }
}

// ====================================================================== //
// spawn                                                                   //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Lower `spawn f(a, b)`.
    ///
    /// The scheduler can only start a fiber from a `void(*)(void*)`, so the
    /// arguments are evaluated now, boxed into a heap cell, and unpacked by a
    /// thunk generated for this call site (see `Lowerer::lower_spawn_thunk`).
    fn lower_spawn(&mut self, call: &Expression, span: &Span) -> CodegenResult<Value> {
        let ExpressionKind::Call { callee, args, .. } = &call.kind else {
            return Err(CodegenError::unsupported_at(
                "`spawn` needs a direct function call, as in `spawn worker(1)`",
                span,
            ));
        };
        let ExpressionKind::Identifier(name) = &callee.kind else {
            return Err(CodegenError::unsupported_at(
                "`spawn` can only start a named function in native code",
                span,
            ));
        };
        let info = self.l.funcs.get(name.as_str()).ok_or_else(|| {
            CodegenError::unsupported_at(format!("unknown function `{}`", name), span)
        })?;
        if info.owner.is_some() {
            return Err(CodegenError::unsupported_at(
                "`spawn` on a method is not lowered by the native backend yet",
                span,
            ));
        }
        let param_types: Vec<Type> = info.params.iter().map(|p| p.ty.clone()).collect();
        if args.len() != param_types.len() {
            return Err(CodegenError::unsupported_at(
                format!("`{}` takes {} argument(s)", name, param_types.len()),
                span,
            ));
        }
        if args.iter().any(|arg| arg.name.is_some()) {
            return Err(CodegenError::unsupported_at(
                "named arguments in `spawn` are not lowered by the native backend yet",
                span,
            ));
        }

        let env_size = HEADER_SIZE + SLOT_SIZE * param_types.len() as i32;
        let env = self.alloc_object(env_size, runtime::KIND_STRUCT)?;
        for (index, (arg, ty)) in args.iter().zip(param_types.iter()).enumerate() {
            let value = self.lower_expr_value(&arg.value, ty)?;
            let slot = self.value_to_slot(value, ty)?;
            let offset = HEADER_SIZE + SLOT_SIZE * index as i32;
            self.builder
                .ins()
                .store(MemFlagsData::trusted(), slot, env, offset);
        }

        let symbol = format!("lira__spawn__{}__{}", name, self.l.next_spawn);
        self.l.next_spawn += 1;
        let mut sig = Signature::new(self.l.call_conv);
        sig.params.push(AbiParam::new(self.l.pointer_ty));
        let func_id = self
            .l
            .module
            .declare_function(&symbol, Linkage::Local, &sig)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        self.l.spawns.push(PendingSpawn {
            symbol,
            func_id,
            callee: name.clone(),
            arg_types: param_types,
        });

        let thunk = self.func_ref_by_id(func_id);
        let ptr = self.pointer_ty();
        let thunk_addr = self.builder.ins().func_addr(ptr, thunk);
        self.call_rt_value("lira_rt_spawn", &[thunk_addr, env])
    }
}

// ====================================================================== //
// Fallback type inference                                                 //
// ====================================================================== //

/// Work out an expression's type without the checker's tables.
///
/// The checker deliberately skips the bodies of methods declared inside
/// `struct`, `class` and `impl` blocks — it only records member references there
/// for the LSP — so `expr_types` is empty for every expression in a method.
/// Bytecode does not care, because it is dynamically typed at run time; native
/// code does. Everything needed is already in hand: the names the caller can
/// resolve, the struct and enum layouts, and every declared signature.
fn infer_with(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    expr: &Expression,
) -> Option<Type> {
    Some(match &expr.kind {
        ExpressionKind::IntLiteral(_) => Type::Int,
        ExpressionKind::FloatLiteral(_) => Type::Float,
        ExpressionKind::StringLiteral(_) => Type::String,
        ExpressionKind::CharLiteral(_) => Type::Char,
        ExpressionKind::BoolLiteral(_) => Type::Bool,
        ExpressionKind::Null => Type::Null,

        ExpressionKind::Identifier(name) => resolve(name)?,

        // `object?.field` yields the field type made optional.
        ExpressionKind::OptionalAccess { object, field } => {
            let object_ty = infer_or_checked_with(l, resolve, object)?;
            if matches!(object_ty, Type::Null) {
                return Some(Type::Null);
            }
            let inner = match &object_ty {
                Type::Optional(inner) => (**inner).clone(),
                other => other.clone(),
            };
            let (Type::Struct(name) | Type::Class(name)) = inner else {
                return None;
            };
            let layout = l.layouts.structs.get(&name)?;
            Type::Optional(Box::new(l.normalize(layout.field(field)?.ty.clone())))
        }

        ExpressionKind::FieldAccess { object, field } => {
            match infer_or_checked_with(l, resolve, object)? {
                Type::Struct(name) | Type::Class(name) => {
                    let layout = l.layouts.structs.get(&name)?;
                    l.normalize(layout.field(field)?.ty.clone())
                }
                Type::Enum(_) if field == "__enum" || field == "__variant" => Type::String,
                _ => return None,
            }
        }

        ExpressionKind::Index { object, .. } => match infer_or_checked_with(l, resolve, object)? {
            Type::Array(inner) => *inner,
            _ => return None,
        },

        ExpressionKind::Array(elements) => {
            let first = elements.first()?;
            Type::Array(Box::new(infer_or_checked_with(l, resolve, first)?))
        }

        ExpressionKind::StructLiteral {
            name: Some(name), ..
        } => l.user_type(name),

        ExpressionKind::EnumVariant { enum_name, .. } => l.user_type(enum_name),

        ExpressionKind::Path { segments } => {
            let [type_name, _] = segments.as_slice() else {
                return None;
            };
            if !l.layouts.enums.contains_key(type_name) {
                return None;
            }
            l.user_type(type_name)
        }

        ExpressionKind::Call { callee, args, .. } => match &callee.kind {
            // A user function of the same name wins over a built-in, so look for
            // one before matching the built-in names.
            ExpressionKind::Identifier(name) if l.funcs.contains_key(name.as_str()) => {
                l.funcs[name.as_str()].ret.clone()
            }
            ExpressionKind::Identifier(name) => match name.as_str() {
                "print" | "println" | "send" | "close" | "fiber_yield" | "collect" => Type::Void,
                "len" | "fiber_id" | "recv" => Type::Int,
                "push" => Type::Void,
                // `pop` hands back an element, not the checker's `T?`: the
                // native runtime reports an empty array instead of null.
                "pop" => match infer_or_checked_with(l, resolve, &args.first()?.value)? {
                    Type::Array(inner) => *inner,
                    _ => return None,
                },
                "chan" => Type::Any,
                _ => match l.funcs.get(name.as_str()) {
                    Some(info) => info.ret.clone(),
                    // `abs(-5)` invokes `impl int { fn abs(self) }` with the
                    // receiver passed positionally; the checker allows it and
                    // the standard library leans on it.
                    None => match runtime::builtin(name) {
                        Some(builtin) => builtin.ret.lira_type(),
                        None => {
                            let receiver = infer_or_checked_with(l, resolve, &args.first()?.value)?;
                            impl_method_return(l, &receiver, name)?
                        }
                    },
                },
            },
            ExpressionKind::EnumVariant { enum_name, .. } => l.user_type(enum_name),
            ExpressionKind::Path { segments } => {
                let [type_name, member] = segments.as_slice() else {
                    return None;
                };
                if l.layouts.enums.contains_key(type_name) {
                    l.user_type(type_name)
                } else {
                    l.funcs.get(&fn_key(Some(type_name), member))?.ret.clone()
                }
            }
            // `self.sum()` parses as a call on a field access rather than as a
            // method call; the lowering treats them the same way.
            ExpressionKind::FieldAccess { object, field } => {
                method_call_type(l, resolve, object, field)?
            }
            _ => return None,
        },

        ExpressionKind::MethodCall {
            receiver, method, ..
        } => method_call_type(l, resolve, receiver, method)?,

        ExpressionKind::Binary { left, op, right } => {
            if is_comparison(*op) || matches!(op, BinaryOp::And | BinaryOp::Or) {
                Type::Bool
            } else {
                let left_ty = infer_or_checked_with(l, resolve, left)?;
                let right_ty = infer_or_checked_with(l, resolve, right)?;
                if *op == BinaryOp::Add
                    && (matches!(left_ty, Type::String) || matches!(right_ty, Type::String))
                {
                    Type::String
                } else {
                    common_type(&left_ty, &right_ty, *op, &expr.span).ok()?
                }
            }
        }

        ExpressionKind::Unary { op, operand } => match op {
            UnaryOp::Not => Type::Bool,
            _ => infer_or_checked_with(l, resolve, operand)?,
        },

        ExpressionKind::Assign { target, .. } | ExpressionKind::CompoundAssign { target, .. } => {
            infer_or_checked_with(l, resolve, target)?
        }

        // For a conditional the checker frequently records `any`, which is
        // no use to native code; prefer the first branch that has a real type.
        ExpressionKind::IfExpr {
            then_expr,
            else_expr,
            ..
        } => infer_or_checked_with(l, resolve, then_expr)
            .and_then(concrete)
            .or_else(|| infer_or_checked_with(l, resolve, else_expr).and_then(concrete))?,

        ExpressionKind::Match { arms, .. } => arms.iter().find_map(|arm: &MatchArm| {
            infer_or_checked_with(l, resolve, &arm.body).and_then(concrete)
        })?,

        ExpressionKind::Cast { type_expr, .. } => l.normalize(layout::type_of_ann(type_expr)),

        ExpressionKind::Range { .. } => l.user_type(layout::RANGE_TYPE),

        ExpressionKind::Lambda { params, body } => Type::Function {
            params: params
                .iter()
                .map(|param| l.normalize(layout::type_of_ann(&param.type_ann)))
                .collect(),
            return_type: Box::new(
                infer_or_checked_with(l, resolve, body)
                    .and_then(concrete)
                    .unwrap_or(Type::Void),
            ),
            required_params: params.len(),
        },

        ExpressionKind::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| infer_or_checked_with(l, resolve, element))
                .collect::<Option<Vec<_>>>()?,
        ),

        // A spawn yields the new fiber's id.
        ExpressionKind::Spawn(_) => Type::Int,
        // A block's value is its trailing expression, or what it returns — the
        // second shape is how a lambda with a body block gives back a value.
        ExpressionKind::Block(block) => match block.statements.last().map(|s| &s.kind) {
            Some(StatementKind::Expression(expr)) | Some(StatementKind::Return(Some(expr))) => {
                infer_or_checked_with(l, resolve, expr)?
            }
            _ => Type::Void,
        },

        _ => return None,
    })
}

/// Whatever the checker recorded, falling back to [`infer_with`].
///
/// A name the caller resolved always wins: the checker erases enum payloads and
/// pattern bindings to `any`, and native code needs the real type. A recorded
/// `any` is likewise a starting point rather than an answer — structural
/// inference is tried first, and `any` only stands if that fails.
fn infer_or_checked_with(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    expr: &Expression,
) -> Option<Type> {
    if let ExpressionKind::Identifier(name) = &expr.kind {
        if let Some(ty) = resolve(name) {
            return Some(ty);
        }
    }
    match l.sema.expr_types.get(&expr.id) {
        Some(ty) if !matches!(ty, Type::Unknown | Type::TypeVar(_)) => {
            let recorded = l.normalize(ty.clone());
            if is_uninformative(&recorded) {
                infer_with(l, resolve, expr).or(Some(recorded))
            } else {
                Some(recorded)
            }
        }
        _ => infer_with(l, resolve, expr),
    }
}

/// Whether a recorded type says nothing native code can act on.
///
/// `any` is the obvious one. `any?` is the same story one level in: the checker
/// records `Optional(Any)` for every `?.`, so taking it at face value would lose
/// the field's real type.
fn is_uninformative(ty: &Type) -> bool {
    match ty {
        Type::Any => true,
        Type::Optional(inner) => is_uninformative(inner),
        _ => false,
    }
}

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Type of a name visible from this function: a local binding, else a global.
    fn binding_type(&self, name: &str) -> Option<Type> {
        match self.lookup(name)? {
            Binding::Local { ty, .. } => Some(ty),
            Binding::Global(global) => Some(global.ty),
        }
    }

    fn infer_or_checked(&self, expr: &Expression) -> Option<Type> {
        infer_or_checked_with(self.l, &|name| self.binding_type(name), expr)
    }
}

// ====================================================================== //
// Enum reflection                                                         //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Lower `value.__enum` and `value.__variant`.
    ///
    /// The bytecode VM stores these as ordinary string fields on the object.
    /// Native enums carry only a discriminant, so `__enum` folds to a constant
    /// and `__variant` becomes a comparison chain over the tag — the strings
    /// themselves live in read-only data, shared across every use.
    ///
    /// Returns `Ok(None)` when this is not an enum reflection access, so the
    /// caller can fall through to ordinary field lowering.
    fn lower_enum_reflection(
        &mut self,
        object: &Expression,
        field: &str,
    ) -> CodegenResult<Option<Value>> {
        if field != "__enum" && field != "__variant" {
            return Ok(None);
        }
        let Ok(Type::Enum(enum_name)) = self.ty_of(object) else {
            return Ok(None);
        };
        if field == "__enum" {
            return self.string_constant(&enum_name).map(Some);
        }

        let layout = self
            .l
            .layouts
            .enums
            .get(&enum_name)
            .ok_or_else(|| {
                CodegenError::unsupported_at(format!("unknown enum `{}`", enum_name), &object.span)
            })?
            .clone();
        let subject = self.lower_expr_value(object, &Type::Enum(enum_name))?;
        let tag = self.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            subject,
            ENUM_TAG_OFFSET,
        );

        let ptr = self.pointer_ty();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, ptr);
        for variant in &layout.variants {
            let name = self.string_constant(&variant.name)?;
            let matches = self
                .builder
                .ins()
                .icmp_imm_s(IntCC::Equal, tag, variant.tag);
            let next = self.builder.create_block();
            self.builder
                .ins()
                .brif(matches, merge, &[name.into()], next, &[]);
            self.terminated = true;
            self.goto(next);
        }
        // Unreachable for a well-formed value, but the block still needs an edge.
        let unknown = self.string_constant("<unknown>")?;
        self.jump_to(merge, &[unknown]);

        self.goto(merge);
        Ok(Some(self.builder.block_params(merge)[0]))
    }
}

// ====================================================================== //
// Ranges                                                                  //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Byte offsets of `Range`'s `start`, `end` and `inclusive` fields.
    fn range_layout(&self, span: &Span) -> CodegenResult<(i32, i32, i32)> {
        if !self.l.layouts.range_layout_is_usable() {
            return Err(CodegenError::unsupported_at(
                "this program declares its own `Range`, so `a..b` cannot be lowered",
                span,
            ));
        }
        let layout = self
            .l
            .layouts
            .structs
            .get(layout::RANGE_TYPE)
            .ok_or_else(|| CodegenError::internal("the built-in `Range` layout is missing"))?;
        Ok((
            layout.field("start").expect("checked above").offset,
            layout.field("end").expect("checked above").offset,
            layout.field("inclusive").expect("checked above").offset,
        ))
    }

    /// Lower `a..b` to the built-in `Range` object, matching what the bytecode
    /// backend builds so `r.start` / `r.end` / `r.inclusive` read the same.
    ///
    /// A range written directly as a `for` subject never reaches here — that
    /// case lowers to a counted loop with no object at all.
    fn lower_range_value(
        &mut self,
        start: Option<&Expression>,
        end: Option<&Expression>,
        inclusive: bool,
        span: &Span,
    ) -> CodegenResult<Value> {
        let (start_offset, end_offset, inclusive_offset) = self.range_layout(span)?;
        let size = self
            .l
            .layouts
            .structs
            .get(layout::RANGE_TYPE)
            .expect("checked by range_layout")
            .size;

        let object = self.alloc_object(size, runtime::KIND_STRUCT)?;
        // An omitted bound stays at the zero `lira_rt_alloc` already wrote.
        if let Some(start) = start {
            let value = self.lower_expr_value(start, &Type::Int)?;
            self.store_at(object, start_offset, &Type::Int, value)?;
        }
        if let Some(end) = end {
            let value = self.lower_expr_value(end, &Type::Int)?;
            self.store_at(object, end_offset, &Type::Int, value)?;
        }
        let flag = self.builder.ins().iconst(types::I8, i64::from(inclusive));
        self.store_at(object, inclusive_offset, &Type::Bool, flag)?;
        Ok(object)
    }

    /// A counted loop whose inclusivity is only known at run time, which is the
    /// case when the range arrived as a value rather than as literal syntax.
    fn lower_dynamic_range_loop(
        &mut self,
        variable: &str,
        start: Value,
        end: Value,
        inclusive: Value,
        body: &AstBlock,
    ) -> CodegenResult<bool> {
        self.push_scope();
        let counter = self.declare_local(variable, Type::Int, Some(start))?;

        let header = self.builder.create_block();
        let body_block = self.builder.create_block();
        let step = self.builder.create_block();
        let exit = self.builder.create_block();

        self.jump_to(header, &[]);
        self.goto(header);
        let current = self.builder.use_var(counter);
        let below = self.builder.ins().icmp(IntCC::SignedLessThan, current, end);
        let at_end = self.builder.ins().icmp(IntCC::Equal, current, end);
        // `i < end || (inclusive && i == end)`
        let at_end_counts = self.builder.ins().band(at_end, inclusive);
        let more = self.builder.ins().bor(below, at_end_counts);
        self.builder.ins().brif(more, body_block, &[], exit, &[]);
        self.terminated = true;

        self.goto(body_block);
        self.loops.push(LoopFrame {
            continue_to: step,
            exit,
            exit_used: true,
        });
        let terminated = self.lower_block(body)?;
        self.loops.pop();
        if !terminated {
            self.jump_to(step, &[]);
        }

        self.goto(step);
        let current = self.builder.use_var(counter);
        let next = self.builder.ins().iadd_imm_s(current, 1);
        self.builder.def_var(counter, next);
        self.jump_to(header, &[]);

        self.goto(exit);
        self.pop_scope();
        Ok(false)
    }
}

// ====================================================================== //
// Lambdas and closures                                                    //
// ====================================================================== //

/// The names an expression reads that it does not itself bind.
///
/// A lambda captures exactly these. Scopes are tracked properly rather than
/// collecting every identifier and subtracting: a name declared partway through
/// the body shadows an outer one only from its declaration onward.
fn free_variables(expr: &Expression, bound: &[String]) -> Vec<String> {
    let mut collector = FreeVars {
        scopes: vec![bound.to_vec()],
        found: Vec::new(),
    };
    collector.visit_expr(expr);
    collector.found
}

struct FreeVars {
    scopes: Vec<Vec<String>>,
    found: Vec<String>,
}

impl FreeVars {
    fn is_bound(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope.iter().any(|bound| bound == name))
    }

    fn bind(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(name.to_string());
        }
    }

    fn use_name(&mut self, name: &str) {
        if !self.is_bound(name) && !self.found.iter().any(|seen| seen == name) {
            self.found.push(name.to_string());
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern) {
        match &pattern.kind {
            PatternKind::Variable(name) => self.bind(name),
            PatternKind::Binding { name, pattern } => {
                self.bind(name);
                self.bind_pattern(pattern);
            }
            PatternKind::Tuple(items) | PatternKind::Or(items) => {
                for item in items {
                    self.bind_pattern(item);
                }
            }
            PatternKind::Constructor { fields, .. } => {
                for field in fields {
                    self.bind_pattern(field);
                }
            }
            PatternKind::Struct { fields, .. } => {
                for (_, field) in fields {
                    self.bind_pattern(field);
                }
            }
            PatternKind::Wildcard | PatternKind::Literal(_) | PatternKind::Range { .. } => {}
        }
    }

    fn visit_block(&mut self, block: &AstBlock) {
        self.scopes.push(Vec::new());
        for stmt in &block.statements {
            self.visit_stmt(stmt);
        }
        self.scopes.pop();
    }

    fn visit_stmt(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::VarDecl {
                pattern,
                initializer,
                ..
            } => {
                // The initialiser is evaluated before the name comes into scope.
                if let Some(init) = initializer {
                    self.visit_expr(init);
                }
                self.bind_pattern(pattern);
            }
            StatementKind::ConstDecl {
                name, initializer, ..
            } => {
                self.visit_expr(initializer);
                self.bind(name);
            }
            StatementKind::Expression(expr) | StatementKind::Return(Some(expr)) => {
                self.visit_expr(expr)
            }
            StatementKind::Break(Some(expr)) => self.visit_expr(expr),
            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.visit_expr(condition);
                self.visit_block(then_branch);
                if let Some(block) = else_branch {
                    self.visit_block(block);
                }
            }
            StatementKind::While { condition, body } => {
                self.visit_expr(condition);
                self.visit_block(body);
            }
            StatementKind::Loop { body } => self.visit_block(body),
            StatementKind::For {
                variable,
                iterable,
                body,
            } => {
                self.visit_expr(iterable);
                self.scopes.push(vec![variable.clone()]);
                for stmt in &body.statements {
                    self.visit_stmt(stmt);
                }
                self.scopes.pop();
            }
            StatementKind::Block(block) => self.visit_block(block),
            _ => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::Identifier(name) => self.use_name(name),

            ExpressionKind::Lambda { params, body } => {
                // A nested lambda's own parameters are bound inside it; anything
                // else it reads is free here too, and is captured twice — once
                // into this closure, once into the inner one.
                self.scopes
                    .push(params.iter().map(|p| p.name.clone()).collect());
                self.visit_expr(body);
                self.scopes.pop();
            }

            ExpressionKind::Match { subject, arms } => {
                self.visit_expr(subject);
                for arm in arms {
                    self.scopes.push(Vec::new());
                    self.bind_pattern(&arm.pattern);
                    if let Some(guard) = &arm.guard {
                        self.visit_expr(guard);
                    }
                    self.visit_expr(&arm.body);
                    self.scopes.pop();
                }
            }

            ExpressionKind::Binary { left, right, .. } => {
                self.visit_expr(left);
                self.visit_expr(right);
            }
            ExpressionKind::Unary { operand, .. } => self.visit_expr(operand),
            ExpressionKind::Call { callee, args, .. } => {
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(&arg.value);
                }
            }
            ExpressionKind::MethodCall { receiver, args, .. } => {
                self.visit_expr(receiver);
                for arg in args {
                    self.visit_expr(&arg.value);
                }
            }
            ExpressionKind::FieldAccess { object, .. }
            | ExpressionKind::OptionalAccess { object, .. } => self.visit_expr(object),
            ExpressionKind::Index { object, index } => {
                self.visit_expr(object);
                self.visit_expr(index);
            }
            ExpressionKind::Array(items) | ExpressionKind::Tuple(items) => {
                for item in items {
                    self.visit_expr(item);
                }
            }
            ExpressionKind::Map(pairs) => {
                for (key, value) in pairs {
                    self.visit_expr(key);
                    self.visit_expr(value);
                }
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.visit_expr(value);
                }
            }
            ExpressionKind::IfExpr {
                condition,
                then_expr,
                else_expr,
            } => {
                self.visit_expr(condition);
                self.visit_expr(then_expr);
                self.visit_expr(else_expr);
            }
            ExpressionKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.visit_expr(start);
                }
                if let Some(end) = end {
                    self.visit_expr(end);
                }
            }
            ExpressionKind::Cast { expr, .. } | ExpressionKind::TypeCheck { expr, .. } => {
                self.visit_expr(expr)
            }
            ExpressionKind::Assign { target, value } => {
                self.visit_expr(target);
                self.visit_expr(value);
            }
            ExpressionKind::CompoundAssign { target, value, .. } => {
                self.visit_expr(target);
                self.visit_expr(value);
            }
            ExpressionKind::Block(block) => self.visit_block(block),
            ExpressionKind::Spawn(inner) | ExpressionKind::Try(inner) => self.visit_expr(inner),
            ExpressionKind::Select(arms) => {
                for arm in arms {
                    match &arm.kind {
                        lirac::ast::SelectArmKind::Recv { channel, .. } => self.visit_expr(channel),
                        lirac::ast::SelectArmKind::Send { value, channel } => {
                            self.visit_expr(value);
                            self.visit_expr(channel);
                        }
                        lirac::ast::SelectArmKind::Default => {}
                    }
                    self.visit_expr(&arm.body);
                }
            }
            ExpressionKind::IntLiteral(_)
            | ExpressionKind::FloatLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::CharLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::Null
            | ExpressionKind::EnumVariant { .. }
            | ExpressionKind::Path { .. } => {}
        }
    }
}

impl<'a> Lowerer<'a> {
    /// The Cranelift signature of a callable function value.
    ///
    /// Every one takes its own closure object as the first argument, so a
    /// lambda that captures and one that does not are called identically.
    fn closure_signature(&self, params: &[Type], ret: &Type) -> CodegenResult<Signature> {
        let mut sig = Signature::new(self.call_conv);
        sig.params.push(AbiParam::new(self.pointer_ty));
        for param in params {
            let clif = repr_of(param)?.clif(self.pointer_ty).ok_or_else(|| {
                CodegenError::unsupported("a function parameter cannot be `void`")
            })?;
            sig.params.push(AbiParam::new(clif));
        }
        if let Some(clif) = repr_of(ret)?.clif(self.pointer_ty) {
            sig.returns.push(AbiParam::new(clif));
        }
        Ok(sig)
    }

    /// Emit the lifted body of one lambda.
    fn lower_lambda_body(&mut self, pending: &PendingLambda) -> CodegenResult<()> {
        let sig = self.closure_signature(
            &pending
                .params
                .iter()
                .map(|p| p.ty.clone())
                .collect::<Vec<_>>(),
            &pending.ret,
        )?;

        let frontend_config = self.module.target_config();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let env = builder.block_params(entry)[0];

            let ret_ty = pending.ret.clone();
            let mut gen = FuncGen::new(self, builder, ret_ty.clone());
            gen.push_scope();

            // Captures were copied into the object when the closure was built;
            // read them back into ordinary locals.
            for (index, (name, ty)) in pending.captures.iter().enumerate() {
                let offset = CLOSURE_CAPTURES_OFFSET + SLOT_SIZE * index as i32;
                let slot = gen
                    .builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), env, offset);
                let value = gen.slot_to_value(slot, ty)?;
                gen.declare_local(name, ty.clone(), Some(value))?;
            }
            for (index, param) in pending.params.iter().enumerate() {
                let value = gen.builder.block_params(entry)[index + 1];
                gen.declare_local(&param.name, param.ty.clone(), Some(value))?;
            }

            let value = gen.lower_expr_typed(&pending.body, &ret_ty)?;
            if !gen.terminated {
                match value {
                    Some(value) if !matches!(ret_ty, Type::Void) => {
                        gen.builder.ins().return_(&[value]);
                    }
                    _ => {
                        gen.builder.ins().return_(&[]);
                    }
                }
            }
            gen.pop_scope();
            gen.builder.seal_all_blocks();
            gen.builder.finalize(frontend_config);
        }

        self.module
            .define_function(pending.func_id, &mut ctx)
            .map_err(|e| CodegenError::internal(format!("{}: {}", pending.symbol, e)))?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    /// Emit the `(env, args...)` wrapper that lets a named function be used as
    /// a function value. The environment is ignored.
    fn lower_fn_wrapper(&mut self, pending: &PendingFnWrapper) -> CodegenResult<()> {
        let info = self
            .funcs
            .get(&pending.target)
            .ok_or_else(|| CodegenError::internal(format!("`{}` is missing", pending.target)))?;
        let target_id = info.func_id;
        let param_types: Vec<Type> = info.params.iter().map(|p| p.ty.clone()).collect();
        let ret = info.ret.clone();
        let sig = self.closure_signature(&param_types, &ret)?;

        let frontend_config = self.module.target_config();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let args: Vec<Value> = builder.block_params(entry)[1..].to_vec();

            let target_ref = self.module.declare_func_in_func(target_id, builder.func);
            let call = builder.ins().call(target_ref, &args);
            let results = builder.inst_results(call).to_vec();
            builder.ins().return_(&results);

            builder.seal_all_blocks();
            builder.finalize(frontend_config);
        }

        self.module
            .define_function(pending.func_id, &mut ctx)
            .map_err(|e| CodegenError::internal(format!("{}: {}", pending.symbol, e)))?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }
}

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Build a closure object for a lambda.
    ///
    /// Captures are copied by value at this point, matching the bytecode VM's
    /// `MakeClosure`, so a closure that outlives the frame it was built in keeps
    /// working — which is the whole point of `make_adder(5)`.
    fn lower_lambda(
        &mut self,
        params: &[Parameter],
        body: &Expression,
        recorded: Option<&Type>,
        span: &Span,
    ) -> CodegenResult<Value> {
        let (param_types, ret) = self.lambda_signature(params, body, recorded, span)?;
        let param_infos: Vec<ParamInfo> = params
            .iter()
            .zip(param_types.iter())
            .map(|(param, ty)| ParamInfo {
                name: param.name.clone(),
                ty: ty.clone(),
                default: None,
            })
            .collect();

        // Anything the body reads that it does not bind itself, and that names a
        // local here, travels with the closure.
        let bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        let mut captures = Vec::new();
        for name in free_variables(body, &bound) {
            if let Some(Binding::Local { ty, .. }) = self.lookup(&name) {
                captures.push((name, ty));
            }
        }

        let symbol = format!("lira__lambda__{}", self.l.next_lambda);
        self.l.next_lambda += 1;
        let sig = self.l.closure_signature(&param_types, &ret)?;
        let func_id = self
            .l
            .module
            .declare_function(&symbol, Linkage::Local, &sig)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        self.l.lambdas.push(PendingLambda {
            symbol,
            func_id,
            params: param_infos,
            ret,
            captures: captures.clone(),
            body: body.clone(),
        });

        let size = CLOSURE_CAPTURES_OFFSET + SLOT_SIZE * captures.len() as i32;
        let closure = self.alloc_object(size, runtime::KIND_STRUCT)?;
        let code_ref = self.func_ref_by_id(func_id);
        let ptr = self.pointer_ty();
        let code = self.builder.ins().func_addr(ptr, code_ref);
        self.builder
            .ins()
            .store(MemFlagsData::trusted(), code, closure, CLOSURE_CODE_OFFSET);
        let count = self.builder.ins().iconst(types::I64, captures.len() as i64);
        self.builder.ins().store(
            MemFlagsData::trusted(),
            count,
            closure,
            CLOSURE_COUNT_OFFSET,
        );
        for (index, (name, ty)) in captures.iter().enumerate() {
            let binding = self
                .lookup(name)
                .ok_or_else(|| CodegenError::internal(format!("captured `{}` vanished", name)))?;
            let value = self.load_binding(&binding);
            let slot = self.value_to_slot(value, ty)?;
            let offset = CLOSURE_CAPTURES_OFFSET + SLOT_SIZE * index as i32;
            self.builder
                .ins()
                .store(MemFlagsData::trusted(), slot, closure, offset);
        }
        Ok(closure)
    }

    /// Parameter and return types of a lambda, from the checker where it
    /// recorded them and from the annotations otherwise.
    fn lambda_signature(
        &self,
        params: &[Parameter],
        body: &Expression,
        recorded: Option<&Type>,
        span: &Span,
    ) -> CodegenResult<(Vec<Type>, Type)> {
        let annotated: Vec<Type> = params
            .iter()
            .map(|param| self.l.normalize(layout::type_of_ann(&param.type_ann)))
            .collect();

        // Prefer the checker's type for the lambda as a whole. Inferring the
        // return from the body alone is wrong for a block body: the block
        // expression itself is recorded as `void`, while the lambda returns
        // whatever its `return` produces.
        let ret = match recorded {
            Some(Type::Function { return_type, .. }) => self.l.normalize((**return_type).clone()),
            _ => self
                .infer_or_checked(body)
                .and_then(concrete)
                .unwrap_or(Type::Void),
        };

        for (param, ty) in params.iter().zip(annotated.iter()) {
            if matches!(ty, Type::Any | Type::Unknown) {
                return Err(CodegenError::unsupported_at(
                    format!(
                        "the native backend needs a type annotation on lambda parameter `{}`",
                        param.name
                    ),
                    span,
                ));
            }
        }
        Ok((annotated, ret))
    }

    /// A named function used as a value: a closure object with no captures,
    /// pointing at a wrapper that ignores the environment.
    ///
    /// The object is emitted into read-only data with a relocation, so taking a
    /// function's value costs nothing at run time.
    fn function_value(&mut self, key: &str) -> CodegenResult<Value> {
        let data_id = match self.l.fn_values.get(key) {
            Some(id) => *id,
            None => {
                let symbol = format!("lira__fnval__{}", self.l.next_lambda);
                self.l.next_lambda += 1;
                let info =
                    self.l.funcs.get(key).ok_or_else(|| {
                        CodegenError::internal(format!("unknown function `{}`", key))
                    })?;
                let param_types: Vec<Type> = info.params.iter().map(|p| p.ty.clone()).collect();
                let ret = info.ret.clone();
                let sig = self.l.closure_signature(&param_types, &ret)?;
                let wrapper_id = self
                    .l
                    .module
                    .declare_function(&symbol, Linkage::Local, &sig)
                    .map_err(|e| CodegenError::internal(e.to_string()))?;
                self.l.fn_wrappers.push(PendingFnWrapper {
                    symbol: symbol.clone(),
                    func_id: wrapper_id,
                    target: key.to_string(),
                });

                // header, code pointer (relocated), capture count
                let mut image = Vec::with_capacity(CLOSURE_CAPTURES_OFFSET as usize);
                image.extend_from_slice(&(runtime::KIND_STRUCT as u32).to_le_bytes());
                image.extend_from_slice(&0u32.to_le_bytes());
                image.extend_from_slice(&(-1i64).to_le_bytes()); // static, never freed
                image.extend_from_slice(&0i64.to_le_bytes()); // code, filled by the reloc
                image.extend_from_slice(&0i64.to_le_bytes()); // no captures

                let mut description = DataDescription::new();
                description.define(image.into_boxed_slice());
                description.set_align(8);
                let code_ref = self
                    .l
                    .module
                    .declare_func_in_data(wrapper_id, &mut description);
                description.write_function_addr(CLOSURE_CODE_OFFSET as u32, code_ref);

                let data_symbol = format!("{}__value", symbol);
                let id = self
                    .l
                    .module
                    .declare_data(&data_symbol, Linkage::Local, false, false)
                    .map_err(|e| CodegenError::internal(e.to_string()))?;
                self.l
                    .module
                    .define_data(id, &description)
                    .map_err(|e| CodegenError::internal(e.to_string()))?;
                self.l.fn_values.insert(key.to_string(), id);
                id
            }
        };
        let gv = self.global_value(data_id);
        let ptr = self.pointer_ty();
        Ok(self.builder.ins().symbol_value(ptr, gv))
    }

    /// Call a function value: load its code pointer and call through it.
    fn lower_indirect_call(
        &mut self,
        callee: &Expression,
        fn_ty: &Type,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        let Type::Function {
            params,
            return_type,
            ..
        } = fn_ty
        else {
            return Err(CodegenError::unsupported_at(
                format!("`{}` is not callable", fn_ty.display_name()),
                span,
            ));
        };
        let params: Vec<Type> = params.iter().map(|t| self.l.normalize(t.clone())).collect();
        let ret = self.l.normalize((**return_type).clone());
        if args.len() != params.len() {
            return Err(CodegenError::unsupported_at(
                format!("this function takes {} argument(s)", params.len()),
                span,
            ));
        }

        let closure = self.lower_expr_value(callee, fn_ty)?;
        let sig = self.l.closure_signature(&params, &ret)?;
        let sig_ref = self.builder.import_signature(sig);
        let ptr = self.pointer_ty();
        let code =
            self.builder
                .ins()
                .load(ptr, MemFlagsData::trusted(), closure, CLOSURE_CODE_OFFSET);

        let mut call_args = Vec::with_capacity(args.len() + 1);
        call_args.push(closure);
        for (arg, ty) in args.iter().zip(params.iter()) {
            call_args.push(self.lower_expr_value(&arg.value, ty)?);
        }
        let call = self.builder.ins().call_indirect(sig_ref, code, &call_args);
        let results = self.builder.inst_results(call);
        let result = results.first().copied();
        Ok(if matches!(ret, Type::Void) {
            None
        } else {
            result
        })
    }
}

// ====================================================================== //
// Optionals                                                               //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Turn a `T` into a `T?`.
    ///
    /// A reference is already nullable and passes straight through. A scalar
    /// goes into a one-slot box, because every bit pattern of an `i64` is a
    /// valid `int` and none of them can mean "none".
    fn wrap_optional(
        &mut self,
        value: Value,
        from: &Type,
        inner: &Type,
        span: &Span,
    ) -> CodegenResult<Value> {
        if !optional_is_boxed(inner) {
            return self.coerce(value, from, inner, span);
        }
        let payload = self.coerce(value, from, inner, span)?;
        let slot = self.value_to_slot(payload, inner)?;
        let box_object =
            self.alloc_object(OPTIONAL_SLOT_OFFSET + SLOT_SIZE, runtime::KIND_STRUCT)?;
        self.builder.ins().store(
            MemFlagsData::trusted(),
            slot,
            box_object,
            OPTIONAL_SLOT_OFFSET,
        );
        Ok(box_object)
    }

    /// Read the payload out of a `T?` that is known to be present.
    ///
    /// A null here is a bug in the program rather than in the lowering — the
    /// checker allows the conversion only where the value has been tested — so
    /// it reports rather than reading through a null pointer.
    fn unwrap_optional(&mut self, value: Value, inner: &Type) -> CodegenResult<Value> {
        if !optional_is_boxed(inner) {
            return Ok(value);
        }
        let present = self.builder.create_block();
        let missing = self.builder.create_block();
        let is_null = self.builder.ins().icmp_imm_s(IntCC::Equal, value, 0);
        self.builder.ins().brif(is_null, missing, &[], present, &[]);
        self.terminated = true;

        self.goto(missing);
        let message = self.string_constant("unwrapped a null optional")?;
        self.call_rt("lira_rt_abort", &[message])?;
        self.builder.ins().trap(unreachable_trap());
        self.terminated = true;

        self.goto(present);
        let slot = self.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            value,
            OPTIONAL_SLOT_OFFSET,
        );
        self.slot_to_value(slot, inner)
    }

    /// `expr?` — return early from the enclosing function when `expr` is
    /// missing, otherwise carry on with the value it holds.
    ///
    /// Works for both `T?` and `Result`: the first propagates null, the second
    /// propagates the `Err` variant unchanged.
    fn lower_try(&mut self, inner_expr: &Expression, span: &Span) -> CodegenResult<Value> {
        let subject_ty = self.ty_of(inner_expr)?;
        let subject = self.lower_expr_value(inner_expr, &subject_ty)?;

        match &subject_ty {
            Type::Optional(inner) => {
                let inner = (**inner).clone();
                let present = self.builder.create_block();
                let missing = self.builder.create_block();
                let is_null = self.builder.ins().icmp_imm_s(IntCC::Equal, subject, 0);
                self.builder.ins().brif(is_null, missing, &[], present, &[]);
                self.terminated = true;

                // Propagate the absence: the enclosing function returns null.
                self.goto(missing);
                let ret_ty = self.return_ty.clone();
                if matches!(ret_ty, Type::Void) {
                    self.builder.ins().return_(&[]);
                } else {
                    let null = self.zero_of(repr_of(&ret_ty)?);
                    self.builder.ins().return_(&[null]);
                }
                self.terminated = true;

                self.goto(present);
                self.unwrap_optional(subject, &inner)
            }

            Type::Result { ok_type, .. } => {
                let ok_type = (**ok_type).clone();
                let ok_present = self.builder.create_block();
                let is_err = self.builder.create_block();
                let tag = self.builder.ins().load(
                    types::I64,
                    MemFlagsData::trusted(),
                    subject,
                    ENUM_TAG_OFFSET,
                );
                let is_ok = self.builder.ins().icmp_imm_s(IntCC::Equal, tag, 0);
                self.builder.ins().brif(is_ok, ok_present, &[], is_err, &[]);
                self.terminated = true;

                // Hand the `Err` back to the caller untouched.
                self.goto(is_err);
                self.builder.ins().return_(&[subject]);
                self.terminated = true;

                self.goto(ok_present);
                let slot = self.builder.ins().load(
                    types::I64,
                    MemFlagsData::trusted(),
                    subject,
                    ENUM_PAYLOAD_OFFSET,
                );
                self.slot_to_value(slot, &ok_type)
            }

            other => Err(CodegenError::unsupported_at(
                format!(
                    "`?` needs an optional or a `Result`, not a `{}`",
                    other.display_name()
                ),
                span,
            )),
        }
    }

    /// `object?.field` — the field when `object` is present, null when it is not.
    fn lower_optional_access(
        &mut self,
        object: &Expression,
        field: &str,
        span: &Span,
    ) -> CodegenResult<Value> {
        let object_ty = self.ty_of(object)?;
        // `null?.name` is null, whatever the field would have been.
        if matches!(object_ty, Type::Null) {
            self.lower_expr(object)?;
            return Ok(self.zero_of(Repr::Ref));
        }
        let inner = match &object_ty {
            Type::Optional(inner) => (**inner).clone(),
            // Chaining on a plain reference is allowed; it can still be null.
            other => other.clone(),
        };
        if optional_is_boxed(&inner) {
            return Err(CodegenError::unsupported_at(
                "`?.` needs something with fields on the left",
                span,
            ));
        }

        let subject = self.lower_expr_value(object, &object_ty)?;
        let (field_ty, offset) = {
            let (Type::Struct(name) | Type::Class(name)) = &inner else {
                return Err(CodegenError::unsupported_at(
                    format!("`{}` has no fields", inner.display_name()),
                    span,
                ));
            };
            let layout = self
                .l
                .layouts
                .structs
                .get(name)
                .ok_or_else(|| {
                    CodegenError::unsupported_at(format!("unknown type `{}`", name), span)
                })?
                .clone();
            let field_layout = layout.field(field).ok_or_else(|| {
                CodegenError::unsupported_at(format!("`{}` has no field `{}`", name, field), span)
            })?;
            (
                self.l.normalize(field_layout.ty.clone()),
                field_layout.offset,
            )
        };

        let result_ty = Type::Optional(Box::new(field_ty.clone()));
        let clif = repr_of(&result_ty)?
            .clif(self.pointer_ty())
            .ok_or_else(|| CodegenError::internal("`?.` cannot produce a `void`"))?;
        let present = self.builder.create_block();
        let absent = self.builder.create_block();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, clif);

        let is_null = self.builder.ins().icmp_imm_s(IntCC::Equal, subject, 0);
        self.builder.ins().brif(is_null, absent, &[], present, &[]);
        self.terminated = true;

        self.goto(present);
        let value = self.load_at(subject, offset, &field_ty)?;
        let wrapped = self.wrap_optional(value, &field_ty, &field_ty, span)?;
        self.jump_to(merge, &[wrapped]);

        self.goto(absent);
        let null = self.zero_of(Repr::Ref);
        self.jump_to(merge, &[null]);

        self.goto(merge);
        Ok(self.builder.block_params(merge)[0])
    }
}

// ====================================================================== //
// Result                                                                  //
// ====================================================================== //

/// Discriminants of the built-in `Result`. `Ok` is 0 so a success test is a
/// comparison against zero.
const RESULT_OK_TAG: i64 = 0;
const RESULT_ERR_TAG: i64 = 1;

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Lower `Result::Ok(x)` / `Result::Err(e)` when the expected type says what
    /// the payload is.
    ///
    /// Returns `Ok(None)` when the call is not a `Result` construction, so the
    /// caller can carry on with ordinary lowering.
    fn lower_result_construction(
        &mut self,
        callee: &Expression,
        args: &[Argument],
        expected: &Type,
    ) -> CodegenResult<Option<Value>> {
        if !matches!(expected, Type::Result { .. }) {
            return Ok(None);
        }
        let variant = match &callee.kind {
            ExpressionKind::EnumVariant {
                enum_name,
                variant_name,
            } if enum_name == layout::RESULT_TYPE => variant_name,
            ExpressionKind::Path { segments } => match segments.as_slice() {
                [enum_name, variant_name] if enum_name == layout::RESULT_TYPE => variant_name,
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        self.lower_result_variant(variant, args.first(), expected, &callee.span)
            .map(Some)
    }

    fn lower_result_variant(
        &mut self,
        variant: &str,
        payload: Option<&Argument>,
        expected: &Type,
        span: &Span,
    ) -> CodegenResult<Value> {
        let Type::Result { ok_type, err_type } = expected else {
            return Err(CodegenError::unsupported_at(
                "a `Result` value needs a `Result<T, E>` context here",
                span,
            ));
        };
        let (tag, payload_ty) = match variant {
            "Ok" => (RESULT_OK_TAG, (**ok_type).clone()),
            "Err" => (RESULT_ERR_TAG, (**err_type).clone()),
            other => {
                return Err(CodegenError::unsupported_at(
                    format!("`Result` has no variant `{}`", other),
                    span,
                ))
            }
        };

        let object = self.alloc_object(ENUM_PAYLOAD_OFFSET + SLOT_SIZE, runtime::KIND_ENUM)?;
        let tag_value = self.builder.ins().iconst(types::I64, tag);
        self.builder
            .ins()
            .store(MemFlagsData::trusted(), tag_value, object, ENUM_TAG_OFFSET);
        if let Some(payload) = payload {
            let value = self.lower_expr_value(&payload.value, &payload_ty)?;
            let slot = self.value_to_slot(value, &payload_ty)?;
            self.builder
                .ins()
                .store(MemFlagsData::trusted(), slot, object, ENUM_PAYLOAD_OFFSET);
        }
        Ok(object)
    }

    /// Match `Result::Ok(x)` / `Result::Err(e)`, binding the payload at the type
    /// the `Result<T, E>` declares for that side.
    fn test_result_constructor(
        &mut self,
        variant: &str,
        fields: &[Pattern],
        subject: Value,
        payloads: (&Type, &Type),
        fail: Block,
        span: &Span,
    ) -> CodegenResult<()> {
        let (ok_type, err_type) = payloads;
        let (tag, payload_ty) = match variant {
            "Ok" => (RESULT_OK_TAG, ok_type.clone()),
            "Err" => (RESULT_ERR_TAG, err_type.clone()),
            other => {
                return Err(CodegenError::unsupported_at(
                    format!("`Result` has no variant `{}`", other),
                    span,
                ))
            }
        };
        let actual = self.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            subject,
            ENUM_TAG_OFFSET,
        );
        let matched = self.builder.ins().icmp_imm_s(IntCC::Equal, actual, tag);
        self.branch_on(matched, fail);

        if let Some(sub_pattern) = fields.first() {
            let slot = self.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                subject,
                ENUM_PAYLOAD_OFFSET,
            );
            let value = self.slot_to_value(slot, &payload_ty)?;
            self.test_pattern(sub_pattern, value, &payload_ty, fail)?;
        }
        Ok(())
    }
}

// ====================================================================== //
// select                                                                  //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Lower `select { ... }`.
    ///
    /// Each arm is tried in source order with the non-blocking channel
    /// operations. With a `_` arm, a full pass that finds nothing ready falls
    /// into it. Without one, the fiber yields and tries again; the runtime
    /// reports a deadlock if a whole sweep of the run queue goes by with no
    /// channel activity, so a select that can never become ready fails loudly
    /// rather than spinning forever.
    fn lower_select(&mut self, arms: &[SelectArm], span: &Span) -> CodegenResult<()> {
        let default_arm = arms
            .iter()
            .find(|arm| matches!(arm.kind, SelectArmKind::Default));
        let channel_arms: Vec<&SelectArm> = arms
            .iter()
            .filter(|arm| !matches!(arm.kind, SelectArmKind::Default))
            .collect();

        // `try_recv` writes through a pointer, so it needs somewhere to write.
        let received = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            SLOT_SIZE as u32,
            3,
        ));

        let retry = self.builder.create_block();
        let merge = self.builder.create_block();
        self.jump_to(retry, &[]);
        self.goto(retry);

        let mut merge_reached = false;
        for arm in &channel_arms {
            let body_block = self.builder.create_block();
            let next = self.builder.create_block();

            match &arm.kind {
                SelectArmKind::Recv { variable, channel } => {
                    let channel_value = self.lower_expr_value(channel, &Type::Any)?;
                    let ptr = self.pointer_ty();
                    let out = self.builder.ins().stack_addr(ptr, received, 0);
                    let ok = self.call_rt_value("lira_rt_chan_try_recv", &[channel_value, out])?;
                    self.builder.ins().brif(ok, body_block, &[], next, &[]);
                    self.terminated = true;

                    self.goto(body_block);
                    self.push_scope();
                    if let Some(variable) = variable {
                        // The received value's type is whatever the arm body
                        // does with it; channels carry uniform slots.
                        let ty = self.select_binding_type(&arm.body, variable);
                        let ptr = self.pointer_ty();
                        let slot = self.builder.ins().stack_load(ptr, types::I64, received, 0);
                        let value = self.slot_to_value(slot, &ty)?;
                        self.declare_local(variable, ty, Some(value))?;
                    }
                }
                SelectArmKind::Send { value, channel } => {
                    let channel_value = self.lower_expr_value(channel, &Type::Any)?;
                    let value_ty = self.ty_of(value)?;
                    let sent = self.lower_expr_value(value, &value_ty)?;
                    let slot = self.value_to_slot(sent, &value_ty)?;
                    let ok = self.call_rt_value("lira_rt_chan_try_send", &[channel_value, slot])?;
                    self.builder.ins().brif(ok, body_block, &[], next, &[]);
                    self.terminated = true;

                    self.goto(body_block);
                    self.push_scope();
                }
                SelectArmKind::Default => unreachable!("filtered out above"),
            }

            self.lower_expr_discard(&arm.body)?;
            if !self.terminated {
                merge_reached = true;
                self.jump_to(merge, &[]);
            }
            self.pop_scope();
            self.goto(next);
        }

        // Nothing was ready.
        match default_arm {
            Some(arm) => {
                self.lower_expr_discard(&arm.body)?;
                if !self.terminated {
                    merge_reached = true;
                    self.jump_to(merge, &[]);
                }
            }
            None => {
                if channel_arms.is_empty() {
                    return Err(CodegenError::unsupported_at(
                        "`select` needs at least one arm",
                        span,
                    ));
                }
                self.call_rt("lira_rt_select_block", &[])?;
                self.jump_to(retry, &[]);
            }
        }

        self.goto(merge);
        if !merge_reached {
            // Every arm returned or broke out.
            self.builder.ins().trap(unreachable_trap());
            self.terminated = true;
        }
        Ok(())
    }

    /// The type to bind a `select` receive to.
    ///
    /// Channels carry uniform slots, so the value's type is not recorded
    /// anywhere; the checker types `recv` as `any`. An `int` is the only thing
    /// the slot can be read back as without more information, which matches what
    /// `recv(ch)` does.
    fn select_binding_type(&self, _body: &Expression, _variable: &str) -> Type {
        Type::Int
    }
}
