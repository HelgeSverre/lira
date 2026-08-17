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
    expand_or_pattern, Argument, BinaryOp, Block as AstBlock, Expression, ExpressionKind, MatchArm,
    Parameter, Pattern, PatternKind, Program, SelectArm, SelectArmKind, Span, Statement,
    StatementKind, UnaryOp,
};
use lirac::checker::Type;
use lirac::ids::NodeId;
use lirac::sema::SemanticTables;

use crate::abi::{is_unsigned, optional_is_boxed, repr_of, Repr};
use crate::error::{CodegenError, CodegenResult};
use crate::layout::{
    self, mangle, sanitise_symbol, storage_size, substitute, LayoutMap, CLASS_VTABLE_OFFSET,
    CLOSURE_CAPTURES_OFFSET, CLOSURE_CODE_OFFSET, CLOSURE_COUNT_OFFSET, ENUM_PAYLOAD_OFFSET,
    ENUM_TAG_OFFSET, HEADER_SIZE, OPTIONAL_SLOT_OFFSET, SLOT_SIZE,
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
    /// Only meaningful for a receiver. A mutable struct receiver is passed
    /// through so its field writes update the caller; all other struct
    /// arguments are value boundaries and are copied at the call site.
    is_mutable: bool,
}

/// A top-level `let`/`var`/`const`, which functions may reference by name.
#[derive(Clone)]
struct GlobalInfo {
    data_id: DataId,
    ty: Type,
}

/// A generic function or method, kept as a template until a call site fixes its
/// type arguments.
struct GenericFn {
    /// The type that owns it, for a method.
    owner: Option<String>,
    name: String,
    /// The method's own parameters first, then the owner's.
    type_params: Vec<String>,
    /// How many of `type_params` came from the surrounding `impl<T>`.
    owner_param_count: usize,
    params: Vec<Parameter>,
    return_type: Option<lirac::ast::TypeExpr>,
    body: AstBlock,
    span: Span,
}

/// An instantiation waiting to be lowered.
struct PendingInstance {
    key: String,
    template: usize,
    bindings: HashMap<String, Type>,
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

/// A concrete implementation adapter for one interface method.  The adapter
/// has the interface ABI (boxed receiver plus interface-declared arguments)
/// and translates those values to the concrete implementation ABI.
struct PendingInterfaceThunk {
    symbol: String,
    func_id: FuncId,
    source_ty: Type,
    method: layout::InterfaceMethodLayout,
    impl_key: String,
}

/// How a pending spawn thunk invokes its already-evaluated callable.
///
/// The scheduler only knows about a `void(*)(void*)` entry point.  The spawn
/// site therefore records enough ABI information for the thunk to perform the
/// ordinary direct, virtual, or closure call after it has resumed on the
/// child fiber's stack.
enum SpawnTarget {
    Direct {
        key: String,
    },
    Virtual {
        class: String,
        method: String,
        key: String,
    },
    Interface {
        interface: String,
        method: String,
        slot: usize,
        params: Vec<Type>,
        ret: Type,
    },
    Indirect {
        params: Vec<Type>,
        ret: Type,
    },
}

/// A `spawn f(...)` site, queued while lowering the enclosing function and
/// emitted afterwards as a `LiraFiberEntry` thunk.
struct PendingSpawn {
    symbol: String,
    func_id: FuncId,
    target: SpawnTarget,
    /// Types of values captured into the heap-allocated environment.  For an
    /// indirect call the closure is the first slot; direct and virtual calls
    /// store the receiver first when one is required.
    env_types: Vec<Type>,
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
    /// Layout-aware recursive struct copy helpers, declared lazily by value
    /// boundaries and emitted after the caller that discovered them.
    copy_helpers: HashMap<String, FuncId>,
    pending_copy_helpers: Vec<String>,
    /// Immutable interface metadata and concrete witnesses, emitted lazily.
    interface_specs: HashMap<String, DataId>,
    interface_witnesses: HashMap<String, DataId>,
    pending_interface_thunks: Vec<PendingInterfaceThunk>,
    /// Closure objects standing in for named functions, one per function.
    fn_values: HashMap<String, DataId>,
    /// Virtual method tables, one per class.
    vtables: HashMap<String, DataId>,
    /// Generic function and method templates, indexed by `generic_index`.
    generic_fns: Vec<GenericFn>,
    /// Template lookup by `owner::name`, or `name` for a free function.
    generic_index: HashMap<String, usize>,
    /// Instantiations already declared, and those still to lower.
    instances: HashMap<String, usize>,
    pending_instances: Vec<PendingInstance>,
    /// Generic aggregates whose methods have already been instantiated.
    instantiated_types: HashSet<String>,
    /// Concrete generic aggregate name -> template name and type arguments.
    /// Method-level monomorphisation needs the owner's arguments at a later
    /// call site (`Box$int.map<U>` still carries `T = int`).
    type_instances: HashMap<String, (String, Vec<Type>)>,
    /// Type parameter bindings in force while lowering an instantiation.
    bindings: HashMap<String, Type>,
    next_spawn: usize,
    next_spawn_temp: usize,
    next_string: usize,
    next_lambda: usize,
}

impl<'a> Lowerer<'a> {
    /// Declare a reusable helper for copying one concrete value-struct layout.
    /// Helpers all use the same `(source, context) -> destination` ABI, which
    /// lets recursive and mutually recursive layouts call one another without
    /// inlining a finite recursion depth into every boundary.
    fn ensure_copy_helper(&mut self, name: &str) -> CodegenResult<FuncId> {
        if let Some(func_id) = self.copy_helpers.get(name) {
            return Ok(*func_id);
        }
        let mut sig = Signature::new(self.call_conv);
        sig.params.push(AbiParam::new(self.pointer_ty));
        sig.params.push(AbiParam::new(self.pointer_ty));
        sig.returns.push(AbiParam::new(self.pointer_ty));
        let symbol = format!("lira__copy__{}", sanitise_symbol(name));
        let func_id = self
            .module
            .declare_function(&symbol, Linkage::Local, &sig)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        self.copy_helpers.insert(name.to_string(), func_id);
        self.pending_copy_helpers.push(name.to_string());
        Ok(func_id)
    }

    /// Intern a static Lira string without needing a function builder.  This
    /// is used by interface metadata, whose relocations are assembled as data
    /// rather than loaded from generated code.
    fn ensure_string_data(&mut self, text: &str) -> CodegenResult<DataId> {
        if let Some(id) = self.strings.get(text) {
            return Ok(*id);
        }
        let bytes = text.as_bytes();
        let mut image = Vec::with_capacity(24 + bytes.len() + 1);
        image.extend_from_slice(&(runtime::KIND_STRING as u32).to_le_bytes());
        image.extend_from_slice(&0u32.to_le_bytes());
        image.extend_from_slice(&(-1i64).to_le_bytes());
        image.extend_from_slice(&(bytes.len() as i64).to_le_bytes());
        image.extend_from_slice(bytes);
        image.push(0);
        let symbol = format!("lira__str__{}", self.next_string);
        self.next_string += 1;
        let mut description = DataDescription::new();
        description.define(image.into_boxed_slice());
        description.set_align(8);
        let id = self
            .module
            .declare_data(&symbol, Linkage::Local, false, false)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        self.module
            .define_data(id, &description)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        self.strings.insert(text.to_owned(), id);
        Ok(id)
    }

    /// Emit the immutable specification for an interface declaration.
    fn ensure_interface_spec(&mut self, name: &str) -> CodegenResult<DataId> {
        if let Some(id) = self.interface_specs.get(name) {
            return Ok(*id);
        }
        let interface = self
            .layouts
            .interfaces
            .get(name)
            .cloned()
            .ok_or_else(|| CodegenError::unsupported(format!("unknown interface `{name}`")))?;
        let mut methods = DataDescription::new();
        methods.define(vec![0u8; interface.methods.len() * 16].into_boxed_slice());
        methods.set_align(8);
        let methods_symbol = format!("lira__interface_methods__{}", sanitise_symbol(name));
        let methods_id = self
            .module
            .declare_data(&methods_symbol, Linkage::Local, false, false)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        for (index, method) in interface.methods.iter().enumerate() {
            let method_name = self.ensure_string_data(&method.name)?;
            let signature = self.ensure_string_data(&interface_method_signature(method))?;
            let name_ref = self.module.declare_data_in_data(method_name, &mut methods);
            methods.write_data_addr((index * 16) as u32, name_ref, 0);
            let signature_ref = self.module.declare_data_in_data(signature, &mut methods);
            methods.write_data_addr((index * 16 + 8) as u32, signature_ref, 0);
        }
        self.module
            .define_data(methods_id, &methods)
            .map_err(|e| CodegenError::internal(e.to_string()))?;

        let mut spec = DataDescription::new();
        let mut spec_image = vec![0u8; 16];
        spec_image[..8].copy_from_slice(&(interface.methods.len() as u64).to_le_bytes());
        spec.define(spec_image.into_boxed_slice());
        spec.set_align(8);
        let methods_ref = self.module.declare_data_in_data(methods_id, &mut spec);
        spec.write_data_addr(8, methods_ref, 0);
        let symbol = format!("lira__interface_spec__{}", sanitise_symbol(name));
        let id = self
            .module
            .declare_data(&symbol, Linkage::Local, false, false)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        self.module
            .define_data(id, &spec)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        self.interface_specs.insert(name.to_owned(), id);
        Ok(id)
    }

    fn interface_impl_key(&self, source: &Type, method: &str) -> Option<String> {
        let source = self.normalize(source.clone());
        match source {
            Type::Class(mut name) | Type::Struct(mut name) => loop {
                let key = fn_key(Some(&name), method);
                if self.funcs.contains_key(&key) {
                    return Some(key);
                }
                let parent = self
                    .layouts
                    .structs
                    .get(&name)
                    .and_then(|layout| layout.parent.clone())?;
                name = parent;
            },
            other => {
                self.builtin_impl_names_for(&other, method)
                    .or_else(|| match (&other, method) {
                        (Type::String, "len") => Some("@intrinsic:string.len".to_owned()),
                        (Type::Array(_), "len") => Some("@intrinsic:array.len".to_owned()),
                        (Type::Array(_), "push") => Some("@intrinsic:array.push".to_owned()),
                        (Type::Array(_), "pop") => Some("@intrinsic:array.pop".to_owned()),
                        _ => None,
                    })
            }
        }
    }

    fn builtin_impl_names_for(&self, ty: &Type, method: &str) -> Option<String> {
        builtin_impl_names(ty).into_iter().find_map(|name| {
            let key = fn_key(Some(&name), method);
            self.funcs.contains_key(&key).then_some(key)
        })
    }

    fn ensure_interface_witness(&mut self, source: &Type, target: &str) -> CodegenResult<DataId> {
        let source = self.normalize(source.clone());
        let key = format!("{}=>{}", interface_type_key(&source), target);
        if let Some(id) = self.interface_witnesses.get(&key) {
            return Ok(*id);
        }
        let interface = self
            .layouts
            .interfaces
            .get(target)
            .cloned()
            .ok_or_else(|| CodegenError::unsupported(format!("unknown interface `{target}`")))?;
        let spec_id = self.ensure_interface_spec(target)?;
        let mut thunk_ids = Vec::with_capacity(interface.methods.len());
        for method in &interface.methods {
            let impl_key = if let Type::Interface(source_name) = &source {
                let source_interface =
                    self.layouts.interfaces.get(source_name).ok_or_else(|| {
                        CodegenError::unsupported(format!("unknown interface `{source_name}`"))
                    })?;
                if source_interface.method(&method.name).is_none() {
                    return Err(CodegenError::unsupported(format!(
                        "interface `{source_name}` has no method `{}`",
                        method.name
                    )));
                }
                String::new()
            } else {
                let Some(impl_key) = self.interface_impl_key(&source, &method.name) else {
                    return Err(CodegenError::unsupported(format!(
                        "type `{}` has no implementation for interface method `{}.{}`",
                        source.display_name(),
                        target,
                        method.name
                    )));
                };
                if !impl_key.starts_with("@intrinsic:") && !self.funcs.contains_key(&impl_key) {
                    return Err(CodegenError::internal(format!(
                        "interface implementation `{impl_key}` vanished"
                    )));
                }
                impl_key
            };
            let mut sig = Signature::new(self.call_conv);
            sig.params.push(AbiParam::new(self.pointer_ty));
            for param in method.params.iter().skip(1) {
                let clif = repr_of(&param.ty)?.clif(self.pointer_ty).ok_or_else(|| {
                    CodegenError::unsupported("interface method parameter cannot be `void`")
                })?;
                sig.params.push(AbiParam::new(clif));
            }
            if let Some(clif) = repr_of(&method_return(&method.signature))?.clif(self.pointer_ty) {
                sig.returns.push(AbiParam::new(clif));
            }
            let symbol = format!(
                "lira__interface_thunk__{}__{}__{}",
                sanitise_symbol(&interface_type_key(&source)),
                sanitise_symbol(target),
                sanitise_symbol(&method.name)
            );
            let func_id = self
                .module
                .declare_function(&symbol, Linkage::Local, &sig)
                .map_err(|e| CodegenError::internal(e.to_string()))?;
            self.pending_interface_thunks.push(PendingInterfaceThunk {
                symbol,
                func_id,
                source_ty: source.clone(),
                method: method.clone(),
                impl_key,
            });
            thunk_ids.push(func_id);
        }

        let mut witness = DataDescription::new();
        let payload_kind = interface_payload_kind(&source)?;
        let mut witness_image = vec![0u8; 16 + thunk_ids.len() * 8];
        witness_image[8..12].copy_from_slice(&payload_kind.to_le_bytes());
        witness_image[12..16].copy_from_slice(&(thunk_ids.len() as u32).to_le_bytes());
        witness.define(witness_image.into_boxed_slice());
        witness.set_align(8);
        let spec_ref = self.module.declare_data_in_data(spec_id, &mut witness);
        witness.write_data_addr(0, spec_ref, 0);
        for (index, func_id) in thunk_ids.iter().enumerate() {
            let func_ref = self.module.declare_func_in_data(*func_id, &mut witness);
            witness.write_function_addr((16 + index * 8) as u32, func_ref);
        }
        let symbol = format!(
            "lira__interface_witness__{}__{}",
            sanitise_symbol(&interface_type_key(&source)),
            sanitise_symbol(target)
        );
        let id = self
            .module
            .declare_data(&symbol, Linkage::Local, false, false)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        self.module
            .define_data(id, &witness)
            .map_err(|e| CodegenError::internal(e.to_string()))?;
        self.interface_witnesses.insert(key, id);
        Ok(id)
    }

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
            copy_helpers: HashMap::new(),
            pending_copy_helpers: Vec::new(),
            interface_specs: HashMap::new(),
            interface_witnesses: HashMap::new(),
            pending_interface_thunks: Vec::new(),
            fn_values: HashMap::new(),
            vtables: HashMap::new(),
            generic_fns: Vec::new(),
            generic_index: HashMap::new(),
            instances: HashMap::new(),
            pending_instances: Vec::new(),
            instantiated_types: HashSet::new(),
            type_instances: HashMap::new(),
            bindings: HashMap::new(),
            next_spawn: 0,
            next_spawn_temp: 0,
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
            let owner = owner.map(|name| self.layouts.canonical_impl_owner(&name));
            if !decl.type_params.is_empty() || decl.owner_type_params.is_some() {
                continue;
            }
            self.lower_function(owner.as_deref(), decl)?;
        }

        let entry_id = self.lower_entry(program)?;

        // These are discovered while lowering, and lowering one can discover
        // more: a lambda body may itself contain a lambda or a spawn.
        loop {
            if let Some(pending) = self.pending_instances.pop() {
                self.lower_instance(&pending)?;
                continue;
            }
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
            if let Some(name) = self.pending_copy_helpers.pop() {
                self.lower_copy_helper(&name)?;
                continue;
            }
            if let Some(pending) = self.pending_interface_thunks.pop() {
                self.lower_interface_thunk(&pending)?;
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
            let owner = owner.map(|name| self.layouts.canonical_impl_owner(&name));
            // A generic declaration has no single body to emit: it becomes a
            // template, and each concrete use adds an instantiation.
            if !decl.type_params.is_empty() || decl.owner_type_params.is_some() {
                let mut type_params = decl.type_params.to_vec();
                if let Some(owner_params) = decl.owner_type_params {
                    for param in owner_params {
                        if !type_params.iter().any(|p| p.name == param.name) {
                            type_params.push(param.clone());
                        }
                    }
                }
                let owner_param_count = type_params.len() - decl.type_params.len();
                let index = self.generic_fns.len();
                self.generic_fns.push(GenericFn {
                    owner: owner.clone(),
                    name: decl.name.to_string(),
                    type_params: type_params.iter().map(|p| p.name.clone()).collect(),
                    owner_param_count,
                    params: decl.params.to_vec(),
                    return_type: decl.return_type.cloned(),
                    body: decl.body.clone(),
                    span: decl.span.clone(),
                });
                self.generic_index
                    .insert(fn_key(owner.as_deref(), decl.name), index);
                continue;
            }

            let key = fn_key(owner.as_deref(), decl.name);
            let symbol = format!("lira__{}", sanitise_symbol(&key));
            if self.funcs.contains_key(&key) {
                return Err(CodegenError::unsupported_at(
                    format!("`{}` is defined more than once", key),
                    decl.span,
                ));
            }

            let mut params = Vec::with_capacity(decl.params.len());
            for param in decl.params {
                let ty = if is_receiver(&param.name) {
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
                    self.resolve_ann(&param.type_ann, &HashSet::new())?
                };
                params.push(ParamInfo {
                    name: param.name.clone(),
                    ty,
                    default: param.default.clone(),
                    is_mutable: param.is_mutable,
                });
            }
            let ret = match decl.return_type {
                Some(t) => self.resolve_ann(t, &HashSet::new())?,
                // The checker and bytecode VM deliberately make an omitted
                // return annotation dynamic.  Treating it as `void` silently
                // discarded `return value` in otherwise valid untyped
                // functions.
                None => Type::Any,
            };

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
                        self.refined_binding_type(pattern.id)
                            .as_ref()
                            .or_else(|| self.sema.pattern_types.get(&pattern.id)),
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
                    let ty =
                        self.materialize_declared_type(ty, initializer.as_ref(), &stmt.span)?;
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
                    let ty = self.materialize_declared_type(ty, Some(initializer), &stmt.span)?;
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
        let annotated = annotation
            .map(|ann| self.normalize(layout::type_of_ann(ann)))
            .and_then(|ty| {
                if matches!(ty, Type::Any) {
                    Some(ty)
                } else {
                    self.native_concrete(ty)
                }
            });
        if annotated.is_some() {
            return annotated;
        }

        let resolve = |name: &str| known.get(name).cloned();
        if initializer.is_some_and(|init| dynamic_any_expression_with(self, &resolve, init)) {
            return Some(Type::Any);
        }

        // The checker uses the source spelling `Range` for range expressions,
        // but native lowering needs the compiler-private layout so a user
        // `struct Range` remains distinct. Preserve that structural type for
        // an unannotated range initializer instead of letting the checker's
        // binding table reintroduce the source name.
        if initializer.is_some_and(|init| matches!(init.kind, ExpressionKind::Range { .. })) {
            return initializer
                .and_then(|init| infer_or_checked_with(self, &resolve, init))
                .and_then(|ty| self.native_concrete(ty));
        }

        pattern_ty
            .cloned()
            .map(|t| self.normalize(t))
            .and_then(|ty| self.native_concrete(ty))
            .or_else(|| {
                stmt_ty
                    .cloned()
                    .map(|t| self.normalize(t))
                    .and_then(|ty| self.native_concrete(ty))
            })
            .or_else(|| {
                initializer
                    .and_then(|init| infer_or_checked_with(self, &resolve, init))
                    .and_then(|ty| self.native_concrete(ty))
            })
            .or_else(|| {
                // `let ch = chan(5)` has no better answer than `any`. That is
                // still pointer-shaped and storable; an operation that needs a
                // sharper type fails later, at the use, with a clearer message.
                match initializer.and_then(|init| infer_or_checked_with(self, &resolve, init)) {
                    Some(Type::Any) => Some(Type::Any),
                    _ => None,
                }
            })
    }

    /// The checker's final type for a declaration. This can be sharper than
    /// the initializer's recorded type: later `push`/`send` operations refine
    /// an empty array or channel even though its initializer was originally
    /// checked as `[unknown]` / `Channel<unknown>`.
    fn refined_binding_type(&self, declaration: NodeId) -> Option<Type> {
        let symbol = self.sema.symbol_refs.get(&declaration)?;
        let ty = self
            .sema
            .symbols
            .get(symbol)
            .map(|entry| self.normalize(entry.ty.clone()))?;
        self.native_concrete(ty)
    }

    /// A checker binding is usable by native lowering only once it names a
    /// concrete representation. In particular, generic aggregates are recorded
    /// by the checker under their erased template name (`Box`/`Opt`); accepting
    /// that spelling here would defer the failure until field or pattern code.
    fn native_concrete(&self, ty: Type) -> Option<Type> {
        concrete(ty).filter(|ty| !is_uninformative(self, ty))
    }

    /// Ensure a generic aggregate inferred from a top-level literal has a
    /// concrete layout and a recorded owner-to-arguments mapping before later
    /// declarations use it. Function-local literals are materialized by
    /// `lower_struct_literal`/`lower_enum_construction`; globals are lowered
    /// after this declaration pass, so they need the same eager step here.
    fn materialize_declared_type(
        &mut self,
        ty: Type,
        initializer: Option<&Expression>,
        span: &Span,
    ) -> CodegenResult<Type> {
        let (Type::Struct(name) | Type::Class(name) | Type::Enum(name)) = &ty else {
            return Ok(ty);
        };
        if self.layouts.structs.contains_key(name) || self.layouts.enums.contains_key(name) {
            return Ok(ty);
        }
        let Some(template) = self.generic_template_name(name).map(str::to_owned) else {
            return Ok(ty);
        };
        let Some(initializer) = initializer else {
            return Ok(ty);
        };
        let args = match (&initializer.kind, self.layouts.generics.get(&template)) {
            (
                ExpressionKind::StructLiteral {
                    name: Some(literal_name),
                    fields,
                },
                Some(_),
            ) if literal_name == &template => {
                let resolve = |name: &str| self.globals.get(name).map(|global| global.ty.clone());
                generic_literal_args(self, &resolve, &template, fields)
            }
            _ => None,
        };
        let Some(args) = args else {
            return Ok(ty);
        };
        self.instantiate_type(&template, &args, span)
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
        if self.layouts.interfaces.contains_key(name) {
            Type::Interface(name.to_string())
        } else if self.layouts.enums.contains_key(name) {
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
        // Inside an instantiation, a type parameter stands for a concrete type.
        let ty = if self.bindings.is_empty() {
            ty
        } else {
            substitute(&ty, &self.bindings)
        };
        match ty {
            Type::Result { ok_type, err_type } => Type::Result {
                ok_type: Box::new(self.normalize(*ok_type)),
                err_type: Box::new(self.normalize(*err_type)),
            },
            Type::Struct(name) | Type::Class(name) | Type::Enum(name) => {
                if let Some(instance) = self.sema.generic_type_instances.get(&name) {
                    let args: Vec<Type> = instance
                        .args
                        .iter()
                        .cloned()
                        .map(|arg| self.normalize(arg))
                        .collect();
                    self.user_type(&layout::mangle(&instance.base_name, &args))
                } else {
                    self.user_type(&name)
                }
            }
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

/// Whether a parameter name is a method receiver. Lira accepts both spellings.
fn is_receiver(name: &str) -> bool {
    name == "self" || name == "this"
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
    type_params: &'p [lirac::ast::TypeParam],
    /// Type parameters of the surrounding `impl<T> ...`, if any.
    owner_type_params: Option<&'p [lirac::ast::TypeParam]>,
    params: &'p [Parameter],
    return_type: Option<&'p lirac::ast::TypeExpr>,
    body: &'p AstBlock,
    span: &'p Span,
}

/// Gather every function and method in declaration order, including those
/// nested inside `struct`, `class` and `impl` bodies.
fn collect_function_decls(program: &Program) -> Vec<(Option<String>, FnDeclRef<'_>)> {
    let mut out = Vec::new();
    collect_decls_in(&program.statements, None, None, &mut out);
    out
}

fn collect_decls_in<'p>(
    statements: &'p [Statement],
    owner: Option<&str>,
    owner_type_params: Option<&'p [lirac::ast::TypeParam]>,
    out: &mut Vec<(Option<String>, FnDeclRef<'p>)>,
) {
    for stmt in statements {
        match &stmt.kind {
            StatementKind::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                ..
            } => out.push((
                owner.map(|o| o.to_string()),
                FnDeclRef {
                    name,
                    type_params,
                    owner_type_params: owner_type_params.filter(|p| !p.is_empty()),
                    params,
                    return_type: return_type.as_ref(),
                    body,
                    span: &stmt.span,
                },
            )),
            StatementKind::StructDecl {
                name,
                type_params,
                methods,
                ..
            } => collect_decls_in(
                methods,
                Some(name),
                (!type_params.is_empty()).then_some(type_params.as_slice()),
                out,
            ),
            StatementKind::ClassDecl { name, methods, .. } => {
                collect_decls_in(methods, Some(name), None, out)
            }
            StatementKind::ImplDecl {
                type_name,
                type_params,
                methods,
                ..
            } => collect_decls_in(
                methods,
                Some(type_name),
                (!type_params.is_empty()).then_some(type_params.as_slice()),
                out,
            ),
            StatementKind::Block(block) => {
                collect_decls_in(&block.statements, owner, owner_type_params, out)
            }
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
        self.lower_function_as(&key, decl)
    }

    /// Lower a function body under an explicit registration key, which is how an
    /// instantiation reuses the template's AST.
    fn lower_function_as(&mut self, key: &str, decl: FnDeclRef<'_>) -> CodegenResult<()> {
        let info = self
            .funcs
            .get(key)
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
                // The parser normalises a `this` parameter to `self`, but the
                // body keeps whichever spelling was written. Bind both.
                if is_receiver(name) {
                    gen.alias_receiver(name);
                }
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
                    (Type::Any, None) => {
                        // An unannotated function is dynamically typed.  Just
                        // like the bytecode VM, falling off its end produces
                        // `null`; requiring an explicit value here rejected
                        // ordinary effect-only `fn main()` and worker bodies.
                        let null = gen.call_rt_value("lira_rt_any_null", &[])?;
                        gen.builder.ins().return_(&[null]);
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
            .map_err(|error| {
                CodegenError::internal(format!("{symbol}: {error}\n{}", ctx.func.display()))
            })?;
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

            let pointer_ty = self.pointer_ty;
            let mut gen = FuncGen::new(self, builder, Type::Void);
            gen.push_scope();
            // Data cells live in the generated image rather than on a fiber
            // stack. Register every cell once before user code can allocate so
            // a global is a tracing root even when it is its sole owner.
            let global_slots: Vec<DataId> = gen.l.globals.values().map(|g| g.data_id).collect();
            for data_id in global_slots {
                let gv = gen.global_value(data_id);
                let slot = gen.builder.ins().symbol_value(pointer_ty, gv);
                gen.call_rt("lira_gc_register_root_slot", &[slot])?;
            }
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
            .map_err(|error| {
                CodegenError::internal(format!("{ENTRY_SYMBOL}: {error}\n{}", ctx.func.display()))
            })?;
        self.module.clear_context(&mut ctx);
        Ok(func_id)
    }

    /// Emit the `LiraFiberEntry` thunk for one `spawn` site.
    ///
    /// Native code cannot hand the scheduler a partially applied call, so the
    /// arguments are boxed into a heap cell at the spawn site and unpacked here.
    fn lower_spawn_thunk(&mut self, pending: &PendingSpawn) -> CodegenResult<()> {
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
            let pointer_ty = self.pointer_ty;

            let mut gen = FuncGen::new(self, builder, Type::Void);
            let mut captured = Vec::with_capacity(pending.env_types.len());
            for (index, ty) in pending.env_types.iter().enumerate() {
                let offset = HEADER_SIZE + SLOT_SIZE * index as i32;
                let slot = gen
                    .builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), env, offset);
                captured.push(gen.slot_to_value(slot, ty)?);
            }

            match &pending.target {
                SpawnTarget::Direct { key } => {
                    let func_id = gen
                        .l
                        .funcs
                        .get(key)
                        .ok_or_else(|| {
                            CodegenError::internal(format!("spawn target `{}` is missing", key))
                        })?
                        .func_id;
                    let func_ref = gen.func_ref_by_id(func_id);
                    let call = gen.builder.ins().call(func_ref, &captured);
                    let _ = gen.builder.inst_results(call);
                }
                SpawnTarget::Indirect { params, ret } => {
                    let closure = captured.first().copied().ok_or_else(|| {
                        CodegenError::internal("indirect spawn has no closure slot")
                    })?;
                    let call_args = &captured[1..];
                    if call_args.len() != params.len() {
                        return Err(CodegenError::internal(
                            "indirect spawn argument count changed after lowering",
                        ));
                    }
                    let sig = gen.l.closure_signature(params, ret)?;
                    let sig_ref = gen.builder.import_signature(sig);
                    let code = gen.builder.ins().load(
                        pointer_ty,
                        MemFlagsData::trusted(),
                        closure,
                        CLOSURE_CODE_OFFSET,
                    );
                    let mut invoke_args = Vec::with_capacity(call_args.len() + 1);
                    invoke_args.push(closure);
                    invoke_args.extend_from_slice(call_args);
                    let call = gen.builder.ins().call_indirect(sig_ref, code, &invoke_args);
                    let _ = gen.builder.inst_results(call);
                }
                SpawnTarget::Virtual { class, method, key } => {
                    let receiver = captured.first().copied().ok_or_else(|| {
                        CodegenError::internal("virtual spawn has no receiver slot")
                    })?;
                    let layout = gen.l.layouts.structs.get(class).ok_or_else(|| {
                        CodegenError::internal(format!("unknown class `{}`", class))
                    })?;
                    let slot = layout.vtable_slot(method).ok_or_else(|| {
                        CodegenError::internal(format!("`{}` has no method `{}`", class, method))
                    })?;
                    let info = gen.l.funcs.get(key).ok_or_else(|| {
                        CodegenError::internal(format!("spawn method `{}` is missing", key))
                    })?;
                    let params = info.params.clone();
                    let ret = info.ret.clone();
                    let mut call_sig = Signature::new(gen.l.call_conv);
                    for param in &params {
                        call_sig.params.push(AbiParam::new(
                            repr_of(&param.ty)?.clif(gen.pointer_ty()).ok_or_else(|| {
                                CodegenError::internal("a method parameter cannot be void")
                            })?,
                        ));
                    }
                    if let Some(ret) = repr_of(&ret)?.clif(gen.pointer_ty()) {
                        call_sig.returns.push(AbiParam::new(ret));
                    }
                    let sig_ref = gen.builder.import_signature(call_sig);
                    let vtable = gen.builder.ins().load(
                        pointer_ty,
                        MemFlagsData::trusted(),
                        receiver,
                        CLASS_VTABLE_OFFSET,
                    );
                    let code = gen.builder.ins().load(
                        pointer_ty,
                        MemFlagsData::trusted(),
                        vtable,
                        slot as i32 * SLOT_SIZE,
                    );
                    let call = gen.builder.ins().call_indirect(sig_ref, code, &captured);
                    let _ = gen.builder.inst_results(call);
                }
                SpawnTarget::Interface {
                    interface,
                    method,
                    slot,
                    params,
                    ret,
                } => {
                    let receiver = captured.first().copied().ok_or_else(|| {
                        CodegenError::internal("interface spawn has no receiver slot")
                    })?;
                    if captured.len() != params.len() {
                        return Err(CodegenError::internal(format!(
                            "spawned interface method `{interface}.{method}` argument count changed after lowering"
                        )));
                    }
                    let mut call_sig = Signature::new(gen.l.call_conv);
                    for param in params {
                        call_sig.params.push(AbiParam::new(
                            repr_of(param)?.clif(gen.pointer_ty()).ok_or_else(|| {
                                CodegenError::internal(
                                    "an interface method parameter cannot be void",
                                )
                            })?,
                        ));
                    }
                    if let Some(ret) = repr_of(ret)?.clif(gen.pointer_ty()) {
                        call_sig.returns.push(AbiParam::new(ret));
                    }
                    let sig_ref = gen.builder.import_signature(call_sig);
                    let method_index = gen.builder.ins().iconst(types::I32, *slot as i64);
                    let code = gen.call_rt_value(
                        "lira_rt_interface_method_slot",
                        &[receiver, method_index],
                    )?;
                    let call = gen.builder.ins().call_indirect(sig_ref, code, &captured);
                    let _ = gen.builder.inst_results(call);
                }
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
    fn is_value_struct_type(&self, ty: &Type) -> bool {
        let ty = self.l.normalize(ty.clone());
        let (Type::Struct(name) | Type::Class(name)) = ty else {
            return false;
        };
        // The compiler-created range has struct-shaped storage, but it is a
        // reference-like runtime object. It is intentionally excluded from
        // value-copy helpers; source code cannot name this private type.
        if name == layout::RANGE_TYPE {
            return false;
        }
        self.l
            .layouts
            .structs
            .get(&name)
            .is_some_and(|layout| !layout.is_class)
    }

    fn is_copyable_value_type(&self, ty: &Type) -> bool {
        match self.l.normalize(ty.clone()) {
            Type::Tuple(_) => true,
            Type::Optional(inner) => self.is_copyable_value_type(&inner),
            ty => self.is_value_struct_type(&ty),
        }
    }

    fn method_receiver_value(
        &mut self,
        receiver: &Expression,
        receiver_ty: &Type,
        key: &str,
    ) -> CodegenResult<Value> {
        let mutable = self
            .l
            .funcs
            .get(key)
            .and_then(|info| info.params.first())
            .is_some_and(|param| param.is_mutable)
            || self
                .l
                .generic_index
                .get(key)
                .and_then(|index| self.l.generic_fns.get(*index))
                .and_then(|template| template.params.first())
                .is_some_and(|param| param.is_mutable);
        if mutable && self.is_value_struct_type(receiver_ty) {
            self.lower_lvalue_value(receiver)
        } else {
            self.lower_expr_value(receiver, receiver_ty)
        }
    }

    fn method_receiver_for_key(
        &mut self,
        receiver: &Expression,
        receiver_ty: &Type,
        key: &str,
    ) -> CodegenResult<Option<Value>> {
        let takes_self = self.l.funcs.get(key).is_some_and(|info| {
            info.owner.is_some()
                && info
                    .params
                    .first()
                    .is_some_and(|param| is_receiver(&param.name))
        }) || self
            .l
            .generic_index
            .get(key)
            .and_then(|index| self.l.generic_fns.get(*index))
            .is_some_and(|template| {
                template
                    .params
                    .first()
                    .is_some_and(|param| is_receiver(&param.name))
            });
        if takes_self {
            Ok(Some(self.method_receiver_value(
                receiver,
                receiver_ty,
                key,
            )?))
        } else {
            Ok(None)
        }
    }

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

    /// Make both spellings of the receiver resolve to the same binding.
    fn alias_receiver(&mut self, declared: &str) {
        let other = if declared == "self" { "this" } else { "self" };
        let Some(binding) = self.lookup(declared) else {
            return;
        };
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(other.to_string(), binding);
        }
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
                        Some(ann) => self.l.resolve_ann(ann, &HashSet::new())?,
                        None => self.ty_of(init)?,
                    };
                    let value = self.lower_expr_value(init, &ty)?;
                    self.bind_irrefutable(pattern, value, &ty)?;
                    return Ok(false);
                };

                let declared = match type_ann {
                    Some(ann) => Some(self.l.resolve_ann(ann, &HashSet::new())?),
                    None => None,
                };
                let (ty, value) = match initializer {
                    Some(init) => {
                        let ty = match declared.clone() {
                            Some(annotated) => annotated,
                            None => {
                                let inferred = self.ty_of(init)?;
                                if matches!(inferred, Type::Any) {
                                    inferred
                                } else if matches!(init.kind, ExpressionKind::Range { .. }) {
                                    // A range expression has a compiler-private
                                    // native type; do not replace it with the
                                    // checker's source-level `Range` spelling.
                                    inferred
                                } else {
                                    self.l.refined_binding_type(pattern.id).unwrap_or(inferred)
                                }
                            }
                        };
                        let mut value = self.lower_expr_typed(init, &ty)?.ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("`{}` is initialised with a value of type `void`", name),
                                &stmt.span,
                            )
                        })?;
                        if matches!(ty, Type::Any) && matches!(self.ty_of(init)?, Type::Any) {
                            value = self.copy_any_boundary(value)?;
                        }
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
                        let mut value = self.lower_expr_typed(expr, &ret_ty)?;
                        if matches!(ret_ty, Type::Any) && matches!(self.ty_of(expr)?, Type::Any) {
                            if let Some(raw) = value {
                                value = Some(self.copy_any_boundary(raw)?);
                            }
                        }
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
                        if matches!(self.return_ty, Type::Any) {
                            let null = self.call_rt_value("lira_rt_any_null", &[])?;
                            self.builder.ins().return_(&[null]);
                        } else {
                            self.builder.ins().return_(&[]);
                        }
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
                // Loops are statement-valued in the native backend (as they
                // are in the bytecode implementation), so a break value is
                // evaluated for its side effects and then discarded.  It is
                // important to lower it before closing the current block:
                // expressions such as `break println("done")` must run once.
                if let Some(value) = value {
                    self.lower_expr_discard(value)?;
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

        // Strings iterate over Unicode scalar values.  The runtime helper
        // returns the scalar at a scalar index and -1 at end-of-string, so we
        // must not use `str_len` here: the native string length is a byte
        // count, while the VM and the language count UTF-8 scalars.
        if matches!(iter_ty, Type::String) {
            let string = self.lower_expr_value(iterable, &iter_ty)?;
            return self.lower_string_loop(variable, string, body);
        }

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

        if matches!(iter_ty, Type::Any) {
            let value = self.lower_expr_value(iterable, &Type::Any)?;
            return self.lower_any_loop(variable, value, body);
        }

        if let Type::Tuple(elements) = iter_ty.clone() {
            // A heterogeneous tuple must not be read through one guessed slot
            // type. Box it once with its complete tuple descriptor and let the
            // Any runtime decode each position independently.
            if elements.is_empty() || elements.windows(2).any(|pair| pair[0] != pair[1]) {
                let tuple = self.lower_expr_value(iterable, &iter_ty)?;
                let boxed = self.box_any(tuple, &iter_ty, span)?;
                return self.lower_any_loop(variable, boxed, body);
            }
            let element_ty = elements.first().cloned().unwrap_or(Type::Any);
            let tuple = self.lower_expr_value(iterable, &iter_ty)?;
            return self.lower_indexed_loop(variable, tuple, element_ty, body);
        }

        let Type::Array(element_ty) = iter_ty.clone() else {
            return Err(CodegenError::unsupported_at(
                format!(
                    "cannot iterate a value of type `{}`; expected an array, string, tuple, or range",
                    iter_ty.display_name()
                ),
                span,
            ));
        };
        // A still-unconstrained empty array has no element representation to
        // observe. Use `int` for the dead loop variable so the loop can be
        // emitted; a preceding `push` refines the binding in the checker and
        // therefore never reaches this path.
        let element_ty = if matches!(element_ty.as_ref(), Type::Unknown | Type::TypeVar(_)) {
            Type::Int
        } else {
            *element_ty
        };
        let array = self.lower_expr_value(iterable, &iter_ty)?;
        self.lower_indexed_loop(variable, array, element_ty, body)
    }

    /// Lower a statically represented array or homogeneous tuple. Both use
    /// the same uniform slot ABI, but the tuple path is only selected after
    /// checker/codegen has proved one element type for every slot.
    fn lower_indexed_loop(
        &mut self,
        variable: &str,
        array: Value,
        element_ty: Type,
        body: &AstBlock,
    ) -> CodegenResult<bool> {
        let len = self.call_rt_value("lira_rt_array_len", &[array])?;
        let zero = self.builder.ins().iconst(types::I64, 0);

        self.push_scope();
        let index = self.declare_local("__lira_index", Type::Int, Some(zero))?;
        let element = self.declare_local(variable, element_ty.clone(), None)?;

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
        let value = self.copy_value_boundary(value, &element_ty)?;
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

    /// Lower an erased `any` iterable. Strings use scalar-indexed decoding;
    /// arrays and tuples use their exact descriptor through `any_array_at`.
    /// The initial type checks prevent scalar/object pointers from being
    /// reinterpreted as aggregate storage and make invalid iteration fail at
    /// the runtime boundary with the native runtime's deterministic panic.
    fn lower_any_loop(
        &mut self,
        variable: &str,
        value: Value,
        body: &AstBlock,
    ) -> CodegenResult<bool> {
        self.push_scope();
        let mode = self.declare_local("__lira_any_mode", Type::Bool, None)?;
        let zero_index = self.builder.ins().iconst(types::I64, 0);
        let index = self.declare_local("__lira_any_index", Type::Int, Some(zero_index))?;
        let element = self.declare_local(variable, Type::Any, None)?;

        let string_kind = self
            .builder
            .ins()
            .iconst(types::I64, RuntimeKind::String as i64);
        let is_string = self.call_rt_value("lira_rt_any_is", &[value, string_kind])?;
        let string_path = self.builder.create_block();
        let aggregate_check = self.builder.create_block();
        self.builder
            .ins()
            .brif(is_string, string_path, &[], aggregate_check, &[]);

        self.goto(aggregate_check);
        let array_kind = self
            .builder
            .ins()
            .iconst(types::I64, RuntimeKind::Array as i64);
        let is_array = self.call_rt_value("lira_rt_any_is", &[value, array_kind])?;
        let tuple_kind = self
            .builder
            .ins()
            .iconst(types::I64, RuntimeKind::Tuple as i64);
        let is_tuple = self.call_rt_value("lira_rt_any_is", &[value, tuple_kind])?;
        let is_aggregate = self.builder.ins().bor(is_array, is_tuple);
        let aggregate_path = self.builder.create_block();
        let invalid_path = self.builder.create_block();
        let header = self.builder.create_block();
        let string_header = self.builder.create_block();
        let string_element = self.builder.create_block();
        let aggregate_header = self.builder.create_block();
        let aggregate_element = self.builder.create_block();
        let body_block = self.builder.create_block();
        let step = self.builder.create_block();
        let exit = self.builder.create_block();
        self.builder
            .append_block_param(body_block, self.pointer_ty());
        self.builder
            .ins()
            .brif(is_aggregate, aggregate_path, &[], invalid_path, &[]);

        self.goto(invalid_path);
        let message = self.string_constant(
            "cannot iterate dynamic value; expected an array, string, tuple, or range",
        )?;
        self.call_rt("lira_rt_abort", &[message])?;
        self.builder.ins().trap(unreachable_trap());

        // One loop index and one binder are shared by both run-time shapes.
        // The body receives the correctly boxed value as a block parameter;
        // it therefore never observes a string pointer or an untyped slot.
        self.goto(string_path);
        let string_mode = self.builder.ins().iconst(types::I8, 1);
        self.builder.def_var(mode, string_mode);
        self.jump_to(header, &[]);

        self.goto(aggregate_path);
        let aggregate_mode = self.builder.ins().iconst(types::I8, 0);
        self.builder.def_var(mode, aggregate_mode);
        self.jump_to(header, &[]);
        self.goto(header);
        let mode_value = self.builder.use_var(mode);
        self.builder
            .ins()
            .brif(mode_value, string_header, &[], aggregate_header, &[]);

        self.goto(string_header);
        let string = self.call_rt_value("lira_rt_any_unbox_string", &[value])?;
        let current_index = self.builder.use_var(index);
        let current = self.call_rt_value("lira_rt_str_char_code", &[string, current_index])?;
        let has_character =
            self.builder
                .ins()
                .icmp_imm_s(IntCC::SignedGreaterThanOrEqual, current, 0);
        self.builder
            .ins()
            .brif(has_character, string_element, &[], exit, &[]);

        self.goto(string_element);
        let boxed = self.call_rt_value("lira_rt_any_box_int", &[current])?;
        self.jump_to(body_block, &[boxed]);

        self.goto(aggregate_header);
        let current_index = self.builder.use_var(index);
        let len = self.call_rt_value("lira_rt_any_len", &[value])?;
        let has_element = self
            .builder
            .ins()
            .icmp(IntCC::SignedLessThan, current_index, len);
        self.builder
            .ins()
            .brif(has_element, aggregate_element, &[], exit, &[]);

        self.goto(aggregate_element);
        let current_index = self.builder.use_var(index);
        let boxed = self.call_rt_value("lira_rt_any_array_at", &[value, current_index])?;
        let boxed = self.copy_any_boundary(boxed)?;
        self.jump_to(body_block, &[boxed]);

        self.goto(body_block);
        let bound = self.builder.block_params(body_block)[0];
        self.builder.def_var(element, bound);
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
        let current_index = self.builder.use_var(index);
        let next_index = self.builder.ins().iadd_imm_s(current_index, 1);
        self.builder.def_var(index, next_index);
        self.jump_to(header, &[]);

        self.goto(exit);
        self.pop_scope();
        Ok(false)
    }

    /// Lower `for ch in string` using scalar-indexed UTF-8 iteration.
    fn lower_string_loop(
        &mut self,
        variable: &str,
        string: Value,
        body: &AstBlock,
    ) -> CodegenResult<bool> {
        self.push_scope();
        let zero = self.builder.ins().iconst(types::I64, 0);
        let index = self.declare_local("__lira_string_index", Type::Int, Some(zero))?;
        let character = self.declare_local(variable, Type::Char, None)?;

        let header = self.builder.create_block();
        let body_block = self.builder.create_block();
        let step = self.builder.create_block();
        let exit = self.builder.create_block();

        self.jump_to(header, &[]);
        self.goto(header);
        let current_index = self.builder.use_var(index);
        let current = self.call_rt_value("lira_rt_str_char_code", &[string, current_index])?;
        self.builder.def_var(character, current);
        let more = self
            .builder
            .ins()
            .icmp_imm_s(IntCC::SignedGreaterThanOrEqual, current, 0);
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
        let current_index = self.builder.use_var(index);
        let next_index = self.builder.ins().iadd_imm_s(current_index, 1);
        self.builder.def_var(index, next_index);
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
        if matches!(self.ty_of(expr)?, Type::Any) {
            let value = self.lower_expr_value(expr, &Type::Any)?;
            return self.call_rt_value("lira_rt_any_truthy", &[value]);
        }
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
        let inferred = self.infer_or_checked(expr);
        if self.dynamic_any_expression(expr) {
            return Ok(Type::Any);
        }
        inferred.map(Ok).unwrap_or_else(|| {
            // Fall through to the shared path so the error carries a span and
            // the literal special cases still apply.
            self.l.ty_of(expr)
        })
    }

    /// Whether an expression's result is a boxed dynamic value even when the
    /// checker's structural inference has guessed a concrete scalar. The
    /// checker intentionally permits an unannotated function to return an
    /// erased index result, but that result must stay `Any` when it feeds a
    /// later operation; treating its pointer as an `int` is type confusion.
    fn dynamic_any_expression(&self, expr: &Expression) -> bool {
        dynamic_any_expression_with(self.l, &|name| self.binding_type(name), expr)
    }

    /// Lower an expression and coerce the result to `expected`.
    fn lower_expr_typed(
        &mut self,
        expr: &Expression,
        expected: &Type,
    ) -> CodegenResult<Option<Value>> {
        if let ExpressionKind::Select(arms) = &expr.kind {
            return self.lower_select(arms, expected, &expr.span);
        }

        // In statement position the value is thrown away, and a `match` whose
        // arms are statements has none to give.
        if matches!(expected, Type::Void) {
            self.lower_expr_discard(expr)?;
            return Ok(None);
        }

        // The declaration of an initially homogeneous array may be widened to
        // `[any]` by a later dynamic `push`.  Lower its literal using that final
        // element type so every slot is boxed consistently; lowering it first
        // as `[int]` and merely reinterpreting the array pointer would leave a
        // mixture of raw integers and `LiraAny*` values in one array.
        if let (ExpressionKind::Array(elements), Type::Array(element_ty)) = (&expr.kind, expected) {
            return self.lower_array_literal(elements, element_ty).map(Some);
        }

        // An erased function return is a real `LiraAny*`, even when the
        // checker's first branch happens to be a concrete scalar. Lower
        // conditional, match, and block expressions against the dynamic
        // result type so every arm boxes its own representation; otherwise a
        // mixed `if`/`match` can either reject a valid source or reinterpret a
        // string/reference arm as the first arm's integer register.
        match &expr.kind {
            ExpressionKind::IfExpr {
                condition,
                then_expr,
                else_expr,
            } if matches!(expected, Type::Any) => {
                return self.lower_if_expr(condition, then_expr, else_expr, expected);
            }
            ExpressionKind::Match { subject, arms } if matches!(expected, Type::Any) => {
                return self.lower_match(subject, arms, expected, &expr.span);
            }
            ExpressionKind::Block(block) if !matches!(expected, Type::Unknown) => {
                let (value, terminated) = self.lower_block_value(block, Some(expected))?;
                self.terminated = terminated;
                return Ok(value);
            }
            _ => {}
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
                    if let ExpressionKind::EnumVariant {
                        enum_name,
                        variant_name,
                    } = &callee.kind
                    {
                        if let Some(value) = self.lower_expected_enum_construction(
                            enum_name,
                            variant_name,
                            args,
                            expected,
                            &expr.span,
                        )? {
                            return Ok(Some(value));
                        }
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
                ExpressionKind::EnumVariant {
                    enum_name,
                    variant_name,
                } => {
                    if let Some(value) = self.lower_expected_enum_construction(
                        enum_name,
                        variant_name,
                        &[],
                        expected,
                        &expr.span,
                    )? {
                        return Ok(Some(value));
                    }
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

        let actual = if matches!(expected, Type::Any) && self.lowers_dynamic_any(expr)? {
            // The checker sometimes infers a concrete result for an operation
            // whose operands are erased. The lowering path still returns a
            // real `LiraAny*`; treating that pointer as an `int` and boxing it
            // again would expose the pointer address to user code.
            Type::Any
        } else {
            self.ty_of(expr)?
        };
        let Some(value) = self.lower_expr(expr)? else {
            return Ok(None);
        };
        self.check_value_type(value, &actual, &expr.span)?;
        Ok(Some(self.coerce(value, &actual, expected, &expr.span)?))
    }

    fn lowers_dynamic_any(&self, expr: &Expression) -> CodegenResult<bool> {
        match &expr.kind {
            ExpressionKind::Binary { left, op, right } => {
                if is_comparison(*op) || matches!(op, BinaryOp::And | BinaryOp::Or) {
                    return Ok(false);
                }
                let left_ty = self.ty_of(left)?;
                let right_ty = self.ty_of(right)?;
                Ok(
                    (matches!(left_ty, Type::Any) || matches!(right_ty, Type::Any))
                        && !(matches!(op, BinaryOp::Add)
                            && (matches!(left_ty, Type::String)
                                || matches!(right_ty, Type::String))),
                )
            }
            ExpressionKind::Index { object, .. } | ExpressionKind::FieldAccess { object, .. } => {
                Ok(matches!(self.ty_of(object)?, Type::Any))
            }
            ExpressionKind::Call { callee, .. } => {
                if let ExpressionKind::Identifier(name) = &callee.kind {
                    return Ok(self
                        .l
                        .funcs
                        .get(name)
                        .is_some_and(|info| matches!(info.ret, Type::Any)));
                }
                Ok(false)
            }
            ExpressionKind::MethodCall {
                receiver, method, ..
            } => Ok(matches!(self.ty_of(receiver)?, Type::Any) && method == "pop"),
            _ => Ok(false),
        }
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
            ExpressionKind::Select(arms) => {
                self.lower_select(arms, &Type::Void, &expr.span)?;
            }
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
        if let (Type::Optional(inner), Type::Interface(_)) = (from, to) {
            let unwrapped = self.unwrap_optional(value, inner)?;
            return self.coerce(unwrapped, inner, to, span);
        }
        if let Type::Interface(target) = to {
            if from == to {
                return Ok(value);
            }
            if matches!(from, Type::Any) {
                return self.unbox_any(value, to, span);
            }
            let source = self.l.normalize(from.clone());
            let witness = self.l.ensure_interface_witness(&source, target)?;
            let value = if self.is_copyable_value_type(&source) {
                self.copy_value_boundary(value, &source)?
            } else {
                value
            };
            let payload = self.value_to_slot(value, &source)?;
            let pointer_ty = self.pointer_ty();
            let witness_global = self.global_value(witness);
            let witness = self.builder.ins().symbol_value(pointer_ty, witness_global);
            return self.call_rt_value("lira_rt_interface_new", &[payload, witness]);
        }

        // `Any` has a real, tagged representation.  Conversions at a typed /
        // dynamic boundary must box or validate the payload before the generic
        // representation check below: both a string and an Any are pointers,
        // but they are emphatically not interchangeable pointers.
        if matches!(to, Type::Any) && !matches!(from, Type::Any) {
            return self.box_any(value, from, span);
        }
        if matches!(from, Type::Any) && !matches!(to, Type::Any) {
            return self.unbox_any(value, to, span);
        }

        // Both sides use the pointer ABI, but a non-class struct is still a
        // value. `lower_expr_typed` is the common path for declarations,
        // assignments, returns, field reads, and collection extraction, so
        // copying here gives those boundaries one consistent rule. Lvalue
        // address computation deliberately bypasses this path.
        if self.is_copyable_value_type(from) && self.is_copyable_value_type(to) {
            return self.copy_value_boundary(value, from);
        }

        if let (Type::Optional(from_inner), Type::Optional(to_inner)) = (from, to) {
            if from_inner == to_inner {
                return Ok(value);
            }
            return self.coerce_optional(value, from_inner, to_inner, span);
        }

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

    fn box_any(&mut self, value: Value, from: &Type, span: &Span) -> CodegenResult<Value> {
        let value = if self.is_copyable_value_type(from) {
            self.copy_value_boundary(value, from)?
        } else {
            value
        };
        let symbol = match from {
            Type::Null => return self.call_rt_value("lira_rt_any_null", &[]),
            Type::Bool => "lira_rt_any_box_bool",
            ty if matches!(repr_of(ty)?, Repr::Int) => "lira_rt_any_box_int",
            Type::Float => "lira_rt_any_box_float",
            Type::String => "lira_rt_any_box_string",
            Type::Array(_) | Type::Tuple(_) => "lira_rt_any_box_array_typed",
            Type::Map(_, _) => "lira_rt_any_box_map_typed",
            Type::Function { .. } => "lira_rt_any_box_function_typed",
            Type::Channel(_) => "lira_rt_any_box_channel_typed",
            Type::Interface(_) => "lira_rt_any_box_interface",
            Type::Struct(_) | Type::Class(_) => "lira_rt_any_box_object_typed",
            Type::Enum(_) | Type::Result { .. } => "lira_rt_any_box_object_typed",
            Type::Optional(_) => "lira_rt_any_box_optional",
            ty if matches!(repr_of(ty)?, Repr::Ref) => "lira_rt_any_box_ref",
            _ => {
                return Err(CodegenError::unsupported_at(
                    format!("cannot box `{}` as `any`", from.display_name()),
                    span,
                ))
            }
        };
        match from {
            Type::Array(_)
            | Type::Tuple(_)
            | Type::Map(_, _)
            | Type::Struct(_)
            | Type::Class(_)
            | Type::Enum(_)
            | Type::Result { .. }
            | Type::Optional(_) => {
                let descriptor = self.any_type_descriptor(from);
                let descriptor = self.string_constant(&descriptor)?;
                self.call_rt_value(symbol, &[value, descriptor])
            }
            Type::Function { .. } | Type::Channel(_) => {
                let descriptor = self.any_type_descriptor(from);
                let descriptor = self.string_constant(&descriptor)?;
                self.call_rt_value(symbol, &[value, descriptor])
            }
            Type::Interface(_) => self.call_rt_value(symbol, &[value]),
            _ => self.call_rt_value(symbol, &[value]),
        }
    }

    fn copy_any_boundary(&mut self, value: Value) -> CodegenResult<Value> {
        self.call_rt_value("lira_rt_any_copy", &[value])
    }

    /// Descriptor for a typed aggregate crossing an `any` boundary. The
    /// runtime keeps this as an immutable compiler-data string and uses it to
    /// decode the existing uniform slots without cloning the aggregate.
    fn any_type_descriptor(&self, ty: &Type) -> String {
        self.any_type_descriptor_with_stack(ty, &mut HashMap::new())
    }

    fn any_type_descriptor_with_stack(
        &self,
        ty: &Type,
        active_aggregates: &mut HashMap<String, usize>,
    ) -> String {
        match ty {
            Type::Bool => "b".to_owned(),
            Type::Int | Type::Int8 | Type::Int16 | Type::Int32 | Type::Int64 => "i".to_owned(),
            Type::UInt8 | Type::UInt16 | Type::UInt32 | Type::UInt64 | Type::Char => "u".to_owned(),
            Type::Float => "f".to_owned(),
            Type::String => "s".to_owned(),
            Type::Array(inner) => format!(
                "a({})",
                self.any_type_descriptor_with_stack(inner, active_aggregates)
            ),
            Type::Map(key, value) => format!(
                "m({},{})",
                self.any_type_descriptor_with_stack(key, active_aggregates),
                self.any_type_descriptor_with_stack(value, active_aggregates)
            ),
            Type::Tuple(elements) => {
                let elements = elements
                    .iter()
                    .map(|element| self.any_type_descriptor_with_stack(element, active_aggregates))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("t({})", elements)
            }
            Type::Function {
                params,
                return_type,
                ..
            } => {
                let params = params
                    .iter()
                    .map(|param| self.any_type_descriptor_with_stack(param, active_aggregates))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "F({};{})",
                    params,
                    self.any_type_descriptor_with_stack(return_type, active_aggregates)
                )
            }
            Type::Channel(inner) => format!(
                "c({})",
                self.any_type_descriptor_with_stack(inner, active_aggregates)
            ),
            Type::Struct(name) | Type::Class(name) => {
                let object_kind = if self
                    .l
                    .layouts
                    .structs
                    .get(name)
                    .is_some_and(|layout| layout.is_class)
                {
                    'C'
                } else {
                    'S'
                };
                let active_depth = active_aggregates.get(name).copied().unwrap_or(0);
                if active_depth >= 8 {
                    // A recursive field still holds a valid object pointer,
                    // but expanding a cycle forever would make the descriptor
                    // (and every dynamic render/index operation) unbounded.
                    // Mark the finite recursive boundary separately from an
                    // opaque foreign reference so rendering can preserve the
                    // aggregate shape with `{...}`.
                    return "x".to_owned();
                }
                active_aggregates.insert(name.clone(), active_depth + 1);
                let descriptor = self
                    .l
                    .layouts
                    .structs
                    .get(name)
                    .map(|layout| {
                        let fields = layout
                            .fields
                            .iter()
                            .map(|field| {
                                let field_ty = self.l.normalize(field.ty.clone());
                                format!(
                                    "{}@{}/{}:{}",
                                    field.name,
                                    field.offset,
                                    field.size,
                                    self.any_type_descriptor_with_stack(
                                        &field_ty,
                                        active_aggregates,
                                    )
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("{}({}|{})", object_kind, name, fields)
                    })
                    .unwrap_or_else(|| format!("{}({}|)", object_kind, name));
                if active_depth == 0 {
                    active_aggregates.remove(name);
                } else {
                    active_aggregates.insert(name.clone(), active_depth);
                }
                descriptor
            }
            Type::Enum(name) => {
                let active_depth = active_aggregates.get(name).copied().unwrap_or(0);
                if active_depth >= 8 {
                    return "x".to_owned();
                }
                active_aggregates.insert(name.clone(), active_depth + 1);
                let descriptor = self
                    .l
                    .layouts
                    .enums
                    .get(name)
                    .map(|layout| {
                        let variants = layout
                            .variants
                            .iter()
                            .map(|variant| {
                                let payloads = variant
                                    .field_types
                                    .iter()
                                    .map(|field| {
                                        self.any_type_descriptor_with_stack(
                                            &self.l.normalize(field.clone()),
                                            active_aggregates,
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join(",");
                                format!("{}@{}({})", variant.name, variant.tag, payloads)
                            })
                            .collect::<Vec<_>>()
                            .join(";");
                        format!("e({};{})", name, variants)
                    })
                    .unwrap_or_else(|| format!("e({};)", name));
                if active_depth == 0 {
                    active_aggregates.remove(name);
                } else {
                    active_aggregates.insert(name.clone(), active_depth);
                }
                descriptor
            }
            Type::Result { ok_type, err_type } => format!(
                "R({},{})",
                self.any_type_descriptor_with_stack(ok_type, active_aggregates),
                self.any_type_descriptor_with_stack(err_type, active_aggregates)
            ),
            Type::Optional(inner) => format!(
                "q({})",
                self.any_type_descriptor_with_stack(inner, active_aggregates)
            ),
            Type::Interface(name) => format!("I({name})"),
            Type::Null
            | Type::Any
            | Type::Unknown
            | Type::TypeVar(_)
            | Type::TypeParam(_)
            | Type::Void => "y".to_owned(),
        }
    }

    fn unbox_any(&mut self, value: Value, to: &Type, span: &Span) -> CodegenResult<Value> {
        if let Type::Interface(target) = to {
            return self.unbox_any_interface(value, target, span);
        }
        let symbol = match to {
            Type::Bool => "lira_rt_any_unbox_bool",
            ty if matches!(repr_of(ty)?, Repr::Int) => "lira_rt_any_unbox_int",
            Type::Float => "lira_rt_any_unbox_float",
            Type::String => "lira_rt_any_unbox_string",
            Type::Function { .. } => "lira_rt_any_unbox_function_typed",
            Type::Channel(_) => "lira_rt_any_unbox_channel_typed",
            Type::Optional(_) => "lira_rt_any_unbox_optional",
            // Arrays, tuples, and maps carry complete descriptors in the Any
            // wrapper. Validate them before interpreting their uniform slots.
            Type::Array(_) | Type::Tuple(_) => "lira_rt_any_unbox_object_typed",
            Type::Map(_, _) => "lira_rt_any_unbox_map",
            Type::Struct(_) | Type::Class(_) | Type::Enum(_) | Type::Result { .. } => {
                "lira_rt_any_unbox_object_typed"
            }
            ty if matches!(repr_of(ty)?, Repr::Ref) => "lira_rt_any_unbox_ref",
            _ => {
                return Err(CodegenError::unsupported_at(
                    format!("cannot use an `any` value as `{}`", to.display_name()),
                    span,
                ))
            }
        };
        if matches!(
            to,
            Type::Struct(_) | Type::Class(_) | Type::Enum(_) | Type::Result { .. }
        ) {
            let descriptor = self.any_type_descriptor(to);
            let descriptor = self.string_constant(&descriptor)?;
            let value = self.call_rt_value(symbol, &[value, descriptor])?;
            return if self.is_value_struct_type(to) {
                self.copy_value_boundary(value, to)
            } else {
                Ok(value)
            };
        }
        if matches!(to, Type::Function { .. } | Type::Channel(_)) {
            let descriptor = self.any_type_descriptor(to);
            let descriptor = self.string_constant(&descriptor)?;
            return self.call_rt_value(symbol, &[value, descriptor]);
        }
        if matches!(to, Type::Array(_) | Type::Tuple(_) | Type::Map(_, _)) {
            let descriptor = self.any_type_descriptor(to);
            let descriptor = self.string_constant(&descriptor)?;
            return self.call_rt_value(symbol, &[value, descriptor]);
        }
        if matches!(to, Type::Optional(_)) {
            let descriptor = self.any_type_descriptor(to);
            let descriptor = self.string_constant(&descriptor)?;
            let value = self.call_rt_value(symbol, &[value, descriptor])?;
            return if self.is_copyable_value_type(to) {
                self.copy_value_boundary(value, to)
            } else {
                Ok(value)
            };
        }
        self.call_rt_value(symbol, &[value])
    }

    /// Recover the small set of erased concrete forms for which this target
    /// interface has a native witness.  Interface Any values use the checked
    /// runtime helper; raw string/array values are unboxed only after a tag
    /// probe, so they cannot be mistaken for an interface pointer.
    fn compatible_interface_sources(&self, target: &str) -> Vec<String> {
        let mut sources: Vec<String> = self
            .l
            .sema
            .interface_implementations
            .get(target)
            .into_iter()
            .flatten()
            .filter_map(|ty| match ty {
                Type::Interface(name) => Some(name.clone()),
                _ => None,
            })
            .collect();
        if self.l.layouts.interfaces.contains_key(target)
            && !sources.iter().any(|source| source == target)
        {
            sources.push(target.to_owned());
        }
        sources.sort();
        sources.dedup();
        sources
    }

    /// Test an interface box against the finite, checker-approved set of
    /// source interface descriptors that satisfy `target`.
    ///
    /// Pointer identity is intentional. Runtime method-table equality cannot
    /// by itself model parameter contravariance, covariant results, or the
    /// per-position default contract enforced by the checker.
    fn known_interface_implements(
        &mut self,
        interface_value: Value,
        target: &str,
    ) -> CodegenResult<Value> {
        let actual_spec = self.call_rt_value("lira_rt_interface_spec", &[interface_value])?;
        let sources = self.compatible_interface_sources(target);
        if sources.is_empty() {
            return Ok(self.builder.ins().iconst(types::I8, 0));
        }

        let pointer_ty = self.pointer_ty();
        let miss = self.builder.create_block();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, types::I8);
        for (index, source) in sources.iter().enumerate() {
            let matched = self.builder.create_block();
            let next = if index + 1 == sources.len() {
                miss
            } else {
                self.builder.create_block()
            };
            let source_spec = self.l.ensure_interface_spec(source)?;
            let source_spec_global = self.global_value(source_spec);
            let source_spec = self
                .builder
                .ins()
                .symbol_value(pointer_ty, source_spec_global);
            let is_match = self
                .builder
                .ins()
                .icmp(IntCC::Equal, actual_spec, source_spec);
            self.builder.ins().brif(is_match, matched, &[], next, &[]);
            self.terminated = true;

            self.goto(matched);
            let yes = self.builder.ins().iconst(types::I8, 1);
            self.jump_to(merge, &[yes]);
            self.goto(next);
        }
        let no = self.builder.ins().iconst(types::I8, 0);
        self.jump_to(merge, &[no]);
        self.goto(merge);
        Ok(self.builder.block_params(merge)[0])
    }

    fn unbox_any_interface(
        &mut self,
        value: Value,
        target: &str,
        span: &Span,
    ) -> CodegenResult<Value> {
        let target_spec = self.l.ensure_interface_spec(target)?;
        let pointer_ty = self.pointer_ty();
        let target_spec_global = self.global_value(target_spec);
        let target_spec_value = self
            .builder
            .ins()
            .symbol_value(pointer_ty, target_spec_global);
        let implementations = self
            .l
            .sema
            .interface_implementations
            .get(target)
            .cloned()
            .unwrap_or_default();
        let supports_bool = implementations
            .iter()
            .any(|candidate| matches!(candidate, Type::Bool));
        let mut integer_sources: Vec<Type> = implementations
            .iter()
            .filter(|candidate| is_erased_integer_type(candidate))
            .cloned()
            .collect();
        integer_sources.sort_by_key(Type::display_name);
        integer_sources.dedup();
        let integer_source = (integer_sources.len() == 1).then(|| integer_sources[0].clone());
        let supports_float = implementations
            .iter()
            .any(|candidate| matches!(candidate, Type::Float));
        let supports_string = implementations
            .iter()
            .any(|candidate| matches!(candidate, Type::String));
        let mut array_sources: Vec<Type> = implementations
            .iter()
            .filter_map(|candidate| match candidate {
                Type::Array(element) => Some(Type::Array(element.clone())),
                _ => None,
            })
            .collect();
        array_sources.sort_by_key(Type::display_name);
        array_sources.dedup();
        let mut object_sources: Vec<Type> = implementations
            .iter()
            .filter(|candidate| matches!(candidate, Type::Struct(_) | Type::Class(_)))
            .cloned()
            .collect();
        object_sources.sort_by_key(Type::display_name);
        object_sources.dedup();
        let interface_sources = self.compatible_interface_sources(target);

        let interface_block = self.builder.create_block();
        let bool_probe = self.builder.create_block();
        let bool_value = self.builder.create_block();
        let integer_probe = self.builder.create_block();
        let integer_value = self.builder.create_block();
        let float_probe = self.builder.create_block();
        let float_value = self.builder.create_block();
        let string_probe = self.builder.create_block();
        let string_value = self.builder.create_block();
        let array_probe = self.builder.create_block();
        let array_value = self.builder.create_block();
        let object_probe = self.builder.create_block();
        let object_value = self.builder.create_block();
        let error_block = self.builder.create_block();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, pointer_ty);

        let interface_kind = self.builder.ins().iconst(types::I64, 11);
        let is_interface = self.call_rt_value("lira_rt_any_is", &[value, interface_kind])?;
        self.builder
            .ins()
            .brif(is_interface, interface_block, &[], bool_probe, &[]);
        self.terminated = true;

        self.goto(interface_block);
        let interface_value = self.call_rt_value("lira_rt_any_unbox_ref", &[value])?;
        let actual_spec = self.call_rt_value("lira_rt_interface_spec", &[interface_value])?;
        if interface_sources.is_empty() {
            self.jump_to(error_block, &[]);
        } else {
            for (index, source) in interface_sources.iter().enumerate() {
                let matched = self.builder.create_block();
                let next = if index + 1 == interface_sources.len() {
                    error_block
                } else {
                    self.builder.create_block()
                };
                let source_spec = self.l.ensure_interface_spec(source)?;
                let source_spec_global = self.global_value(source_spec);
                let source_spec = self
                    .builder
                    .ins()
                    .symbol_value(pointer_ty, source_spec_global);
                let is_match = self
                    .builder
                    .ins()
                    .icmp(IntCC::Equal, actual_spec, source_spec);
                self.builder.ins().brif(is_match, matched, &[], next, &[]);
                self.terminated = true;

                self.goto(matched);
                let adapted = self.coerce(
                    interface_value,
                    &Type::Interface(source.clone()),
                    &Type::Interface(target.to_owned()),
                    span,
                )?;
                self.jump_to(merge, &[adapted]);
                if next != error_block {
                    self.goto(next);
                }
            }
        }

        self.goto(bool_probe);
        let bool_kind = self.builder.ins().iconst(types::I64, 1);
        let is_bool = self.call_rt_value("lira_rt_any_is", &[value, bool_kind])?;
        self.builder
            .ins()
            .brif(is_bool, bool_value, &[], integer_probe, &[]);
        self.terminated = true;

        self.goto(bool_value);
        if supports_bool {
            let raw = self.call_rt_value("lira_rt_any_unbox_bool", &[value])?;
            let adapted =
                self.coerce(raw, &Type::Bool, &Type::Interface(target.to_owned()), span)?;
            self.jump_to(merge, &[adapted]);
        } else {
            self.jump_to(error_block, &[]);
        }

        self.goto(integer_probe);
        let integer_kind = self.builder.ins().iconst(types::I64, 2);
        let is_integer = self.call_rt_value("lira_rt_any_is", &[value, integer_kind])?;
        self.builder
            .ins()
            .brif(is_integer, integer_value, &[], float_probe, &[]);
        self.terminated = true;

        self.goto(integer_value);
        if let Some(integer_source) = integer_source {
            let raw = self.call_rt_value("lira_rt_any_unbox_int", &[value])?;
            let adapted = self.coerce(
                raw,
                &integer_source,
                &Type::Interface(target.to_owned()),
                span,
            )?;
            self.jump_to(merge, &[adapted]);
        } else {
            self.jump_to(error_block, &[]);
        }

        self.goto(float_probe);
        let float_kind = self.builder.ins().iconst(types::I64, 3);
        let is_float = self.call_rt_value("lira_rt_any_is", &[value, float_kind])?;
        self.builder
            .ins()
            .brif(is_float, float_value, &[], string_probe, &[]);
        self.terminated = true;

        self.goto(float_value);
        if supports_float {
            let raw = self.call_rt_value("lira_rt_any_unbox_float", &[value])?;
            let adapted =
                self.coerce(raw, &Type::Float, &Type::Interface(target.to_owned()), span)?;
            self.jump_to(merge, &[adapted]);
        } else {
            self.jump_to(error_block, &[]);
        }

        self.goto(string_probe);
        let string_kind = self.builder.ins().iconst(types::I64, 4);
        let is_string = self.call_rt_value("lira_rt_any_is", &[value, string_kind])?;
        self.builder
            .ins()
            .brif(is_string, string_value, &[], array_probe, &[]);
        self.terminated = true;

        self.goto(string_value);
        if supports_string {
            let string = self.call_rt_value("lira_rt_any_unbox_string", &[value])?;
            let string = self.coerce(
                string,
                &Type::String,
                &Type::Interface(target.to_owned()),
                span,
            )?;
            self.jump_to(merge, &[string]);
        } else {
            self.jump_to(error_block, &[]);
        }

        self.goto(array_probe);
        let array_kind = self.builder.ins().iconst(types::I64, 5);
        let is_array = self.call_rt_value("lira_rt_any_is", &[value, array_kind])?;
        self.builder
            .ins()
            .brif(is_array, array_value, &[], object_probe, &[]);
        self.terminated = true;

        self.goto(array_value);
        if array_sources.is_empty() {
            self.jump_to(error_block, &[]);
        } else {
            for (index, array_source) in array_sources.iter().enumerate() {
                let matched = self.builder.create_block();
                let next = if index + 1 == array_sources.len() {
                    error_block
                } else {
                    self.builder.create_block()
                };
                let descriptor = self.any_type_descriptor(array_source);
                let descriptor = self.string_constant(&descriptor)?;
                let is_match = self.call_rt_value("lira_rt_any_is_typed", &[value, descriptor])?;
                self.builder.ins().brif(is_match, matched, &[], next, &[]);
                self.terminated = true;

                self.goto(matched);
                let array = self.call_rt_value("lira_rt_any_unbox_array", &[value])?;
                let array = self.coerce(
                    array,
                    array_source,
                    &Type::Interface(target.to_owned()),
                    span,
                )?;
                self.jump_to(merge, &[array]);
                if next != error_block {
                    self.goto(next);
                }
            }
        }

        self.goto(object_probe);
        let object_kind = self.builder.ins().iconst(types::I64, 6);
        let is_object = self.call_rt_value("lira_rt_any_is", &[value, object_kind])?;
        self.builder
            .ins()
            .brif(is_object, object_value, &[], error_block, &[]);
        self.terminated = true;

        self.goto(object_value);
        if object_sources.is_empty() {
            self.jump_to(error_block, &[]);
        } else {
            for (index, object_source) in object_sources.iter().enumerate() {
                let matched = self.builder.create_block();
                let next = if index + 1 == object_sources.len() {
                    error_block
                } else {
                    self.builder.create_block()
                };
                let descriptor_text = self.any_type_descriptor(object_source);
                let descriptor = self.string_constant(&descriptor_text)?;
                let is_match = self.call_rt_value("lira_rt_any_is_typed", &[value, descriptor])?;
                self.builder.ins().brif(is_match, matched, &[], next, &[]);
                self.terminated = true;

                self.goto(matched);
                let object =
                    self.call_rt_value("lira_rt_any_unbox_object_typed", &[value, descriptor])?;
                let adapted = self.coerce(
                    object,
                    object_source,
                    &Type::Interface(target.to_owned()),
                    span,
                )?;
                self.jump_to(merge, &[adapted]);
                if next != error_block {
                    self.goto(next);
                }
            }
        }

        self.goto(error_block);
        let invalid =
            self.call_rt_value("lira_rt_any_unbox_interface", &[value, target_spec_value])?;
        self.jump_to(merge, &[invalid]);

        self.goto(merge);
        Ok(self.builder.block_params(merge)[0])
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
                    let explicit: Vec<Type> = type_args
                        .iter()
                        .map(|t| self.l.resolve_ann(t, &HashSet::new()))
                        .collect::<CodegenResult<_>>()?;
                    match &callee.kind {
                        // `foo::<int>(...)` names its instantiation directly.
                        ExpressionKind::Identifier(name) => {
                            if let Some(result) = self
                                .lower_generic_call(name, None, None, args, &explicit, &expr.span)?
                            {
                                return Ok(result);
                            }
                        }
                        // The parser represents `object.method::<T>(...)` as
                        // a Call whose callee is a FieldAccess. Keep this path
                        // equivalent to the dedicated MethodCall AST variant.
                        ExpressionKind::FieldAccess { object, field } => {
                            return self.lower_method_call_with_type_args(
                                object, field, args, &explicit, &expr.span,
                            )
                        }
                        _ => {
                            return Err(CodegenError::unsupported_at(
                                "explicit type arguments need a named function or method",
                                &expr.span,
                            ));
                        }
                    }
                    // Not generic after all: the arguments were redundant.
                    return self.lower_call(callee, args, &expr.span);
                }
                self.lower_call(callee, args, &expr.span)
            }

            ExpressionKind::MethodCall {
                receiver,
                method,
                args,
                type_args,
            } => {
                let explicit: Vec<Type> = type_args
                    .iter()
                    .map(|t| self.l.resolve_ann(t, &HashSet::new()))
                    .collect::<CodegenResult<_>>()?;
                self.lower_method_call_with_type_args(receiver, method, args, &explicit, &expr.span)
            }

            ExpressionKind::FieldAccess { object, field } => {
                if let Some(value) = self.lower_enum_reflection(object, field)? {
                    return Ok(Some(value));
                }
                if matches!(self.ty_of(object)?, Type::Any) {
                    let object = self.lower_expr_value(object, &Type::Any)?;
                    let key = self.string_constant(field)?;
                    let key = self.call_rt_value("lira_rt_any_box_string", &[key])?;
                    return self
                        .call_rt_value("lira_rt_any_index", &[object, key])
                        .map(Some);
                }
                let (base, offset, field_ty) = self.field_address(object, field, &expr.span)?;
                Ok(Some(self.load_at(base, offset, &field_ty)?))
            }

            ExpressionKind::Index { object, index } => {
                let object_ty = self.ty_of(object)?;
                match object_ty.clone() {
                    Type::Any => {
                        let object = self.lower_expr_value(object, &Type::Any)?;
                        let key_ty = self.ty_of(index)?;
                        let key = self.lower_expr_value(index, &Type::Any)?;
                        let _ = key_ty;
                        self.call_rt_value("lira_rt_any_index", &[object, key])
                            .map(Some)
                    }
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
                    Type::String => {
                        let string = self.lower_expr_value(object, &Type::String)?;
                        let index = self.lower_expr_value(index, &Type::Int)?;
                        self.call_rt_value("lira_rt_str_index", &[string, index])
                            .map(Some)
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
                self.lower_array_literal(elements, &element_ty).map(Some)
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
                let result_ty = self.ty_of(expr)?;
                self.lower_select(arms, &result_ty, &expr.span)
            }
            ExpressionKind::OptionalAccess { object, field } => self
                .lower_optional_access(object, field, &expr.span)
                .map(Some),
            ExpressionKind::TypeCheck {
                expr: inner,
                type_expr: _,
            } => self.lower_type_check(inner, expr.id, &expr.span).map(Some),
        }
    }

    fn lower_array_literal(
        &mut self,
        elements: &[Expression],
        element_ty: &Type,
    ) -> CodegenResult<Value> {
        let capacity = self.builder.ins().iconst(types::I64, elements.len() as i64);
        let array = self.call_rt_value("lira_rt_array_new", &[capacity])?;
        for element in elements {
            let value = self.lower_expr_value(element, element_ty)?;
            let slot = self.value_to_slot(value, element_ty)?;
            self.call_rt("lira_rt_array_push", &[array, slot])?;
        }
        Ok(array)
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

fn vm_reference_equality_is_always_false(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Array(_)
            | Type::Tuple(_)
            | Type::Map(_, _)
            | Type::Struct(_)
            | Type::Class(_)
            | Type::Enum(_)
            | Type::Result { .. }
            | Type::Function { .. }
            | Type::Channel(_)
    )
}

fn dynamic_binary_opcode(op: BinaryOp) -> i64 {
    match op {
        BinaryOp::Add => 0,
        BinaryOp::Sub => 1,
        BinaryOp::Mul => 2,
        BinaryOp::Div => 3,
        BinaryOp::Mod => 4,
        BinaryOp::Pow => 5,
        BinaryOp::BitAnd => 6,
        BinaryOp::BitOr => 7,
        BinaryOp::BitXor => 8,
        BinaryOp::Shl => 9,
        BinaryOp::Shr => 10,
        BinaryOp::UShr => 11,
        BinaryOp::Eq => 0,
        BinaryOp::Ne => 1,
        BinaryOp::Lt => 2,
        BinaryOp::Le => 3,
        BinaryOp::Gt => 4,
        BinaryOp::Ge => 5,
        BinaryOp::And | BinaryOp::Or | BinaryOp::NullCoalesce => {
            unreachable!("control-flow operators are lowered before dynamic dispatch")
        }
    }
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

        if matches!(left_ty, Type::Any) || matches!(right_ty, Type::Any) {
            let l = self.lower_expr_value(left, &Type::Any)?;
            let r = self.lower_expr_value(right, &Type::Any)?;
            let opcode = self
                .builder
                .ins()
                .iconst(types::I64, dynamic_binary_opcode(op));
            let symbol = if is_comparison(op) {
                "lira_rt_any_compare"
            } else {
                "lira_rt_any_binary"
            };
            let result = self.call_rt_value(symbol, &[opcode, l, r])?;
            // The checker gives `string + any` and `any + string` the concrete
            // `string` result type because the VM's Add path stringifies the
            // other operand. The dynamic arithmetic helper still returns an
            // Any box, so unwrap it to the ABI string at this boundary.
            if op == BinaryOp::Add
                && (matches!(left_ty, Type::String) || matches!(right_ty, Type::String))
            {
                return self
                    .call_rt_value("lira_rt_any_to_string", &[result])
                    .map(Some);
            }
            return Ok(Some(result));
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
        if matches!(current_ty, Type::Any) || matches!(rhs_ty, Type::Any) {
            let left = self.coerce(current, current_ty, &Type::Any, span)?;
            let right = self.lower_expr_value(rhs, &Type::Any)?;
            let opcode = self
                .builder
                .ins()
                .iconst(types::I64, dynamic_binary_opcode(op));
            return self.call_rt_value("lira_rt_any_binary", &[opcode, left, right]);
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
    /// Normalize an integer shift count as the VM does: convert to `u32`,
    /// then clamp to 63. Native Cranelift shifts mask the count to the target
    /// word width instead, so passing the raw RHS would make e.g. `64` act as
    /// zero on a 64-bit target.
    fn normalize_shift_count(&mut self, value: Value) -> Value {
        let mask = self.builder.ins().iconst(types::I64, 0xffff_ffff);
        let wrapped = self.builder.ins().band(value, mask);
        let limit = self.builder.ins().iconst(types::I64, 63);
        let over_limit = self
            .builder
            .ins()
            .icmp(IntCC::UnsignedGreaterThan, wrapped, limit);
        self.builder.ins().select(over_limit, limit, wrapped)
    }

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
            if matches!(repr, Repr::Ref) && vm_reference_equality_is_always_false(ty) {
                let cc = if op == BinaryOp::Ne {
                    IntCC::Equal
                } else {
                    IntCC::NotEqual
                };
                return Ok(self.builder.ins().icmp(cc, l, l));
            }
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

        let r = if matches!(op, BinaryOp::Shl | BinaryOp::Shr | BinaryOp::UShr) {
            self.normalize_shift_count(r)
        } else {
            r
        };

        match repr {
            Repr::Float => Ok(match op {
                BinaryOp::Add => self.builder.ins().fadd(l, r),
                BinaryOp::Sub => self.builder.ins().fsub(l, r),
                BinaryOp::Mul => self.builder.ins().fmul(l, r),
                BinaryOp::Div => self.builder.ins().fdiv(l, r),
                // Keep floating remainder and power on the shared runtime
                // ABI.  `fmod` deliberately preserves IEEE NaN/inf behavior
                // for a zero divisor, while `pow` matches the VM's `powf`.
                BinaryOp::Mod => self.call_rt_value("lira_rt_math_fmod", &[l, r])?,
                BinaryOp::Pow => self.call_rt_value("lira_rt_math_pow", &[l, r])?,
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
                if matches!(ty, Type::Any) {
                    return self.call_rt_value("lira_rt_any_neg", &[value]).map(Some);
                }
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
                if matches!(self.ty_of(operand)?, Type::Any) {
                    let value = self.lower_expr_value(operand, &Type::Any)?;
                    return self
                        .call_rt_value("lira_rt_any_bit_not", &[value])
                        .map(Some);
                }
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
            _ if matches!(ty, Type::Any) => {
                self.call_rt_value("lira_rt_any_to_string", &[value])?
            }
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
        if from == to {
            return Ok(value);
        }

        let source_is_dynamic = matches!(
            from,
            Type::Any | Type::Unknown | Type::TypeVar(_) | Type::TypeParam(_)
        );
        if source_is_dynamic && matches!(to, Type::Any) {
            return Ok(value);
        }
        if matches!(to, Type::Any) {
            return self.box_any(value, from, span);
        }
        if matches!(to, Type::Interface(_)) {
            return self.coerce(value, from, to, span);
        }
        if source_is_dynamic {
            return match to {
                Type::Bool => self.call_rt_value("lira_rt_any_cast_bool", &[value]),
                ty if matches!(repr_of(ty)?, Repr::Int) => {
                    self.call_rt_value("lira_rt_any_cast_int", &[value])
                }
                Type::Float => self.call_rt_value("lira_rt_any_cast_float", &[value]),
                Type::String => self.call_rt_value("lira_rt_any_to_string", &[value]),
                Type::Function { .. } => self.unbox_any(value, to, span),
                Type::Array(_)
                | Type::Tuple(_)
                | Type::Map(_, _)
                | Type::Struct(_)
                | Type::Class(_)
                | Type::Enum(_)
                | Type::Result { .. }
                | Type::Channel(_)
                | Type::Optional(_)
                | Type::Interface(_) => self.unbox_any(value, to, span),
                _ => Err(CodegenError::unsupported_at(
                    format!(
                        "cannot cast `any` to `{}` in native code",
                        to.display_name()
                    ),
                    span,
                )),
            };
        }
        if matches!(to, Type::String) {
            if matches!(from, Type::Null) {
                return self.string_constant("null");
            }
            return match repr_of(from)? {
                Repr::Int | Repr::Float | Repr::Bool if !matches!(from, Type::Any) => {
                    self.value_to_string(value, from, span)
                }
                _ if matches!(from, Type::String) => Ok(value),
                _ => {
                    let boxed = self.box_any(value, from, span)?;
                    self.call_rt_value("lira_rt_any_to_string", &[boxed])
                }
            };
        }
        if let Type::Optional(inner) = to {
            if matches!(from, Type::Null)
                || from == inner.as_ref()
                || self.is_class_upcast(from, inner)
            {
                return self.coerce(value, from, to, span);
            }
        }
        if self.is_class_upcast(from, to) {
            return Ok(value);
        }

        let from_repr = repr_of(from)?;
        let to_repr = repr_of(to)?;
        Ok(match (from_repr, to_repr) {
            // All integer widths share the i64 register ABI. The checker has
            // already established an integer/char cast; representation
            // equality is safe for this scalar family only.
            (Repr::Int, Repr::Int) => value,
            (Repr::Int, Repr::Float) => self.builder.ins().fcvt_from_sint(types::F64, value),
            // Saturating rather than trapping: an out-of-range cast clamps.
            (Repr::Float, Repr::Int) => self.builder.ins().fcvt_to_sint_sat(types::I64, value),
            (Repr::Bool, Repr::Int) => self.builder.ins().uextend(types::I64, value),
            (Repr::Bool, Repr::Float) => {
                let integer = self.builder.ins().uextend(types::I64, value);
                self.builder.ins().fcvt_from_uint(types::F64, integer)
            }
            (Repr::Ref, Repr::Int) if matches!(from, Type::String) => {
                self.call_rt_value("lira_rt_str_to_int", &[value])?
            }
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

    fn is_class_upcast(&self, from: &Type, to: &Type) -> bool {
        let (Type::Class(child), Type::Class(parent)) = (from, to) else {
            return false;
        };
        let mut current = child.as_str();
        for _ in 0..=self.l.layouts.structs.len() {
            if current == parent {
                return true;
            }
            let Some(next) = self
                .l
                .layouts
                .structs
                .get(current)
                .and_then(|layout| layout.parent.as_deref())
            else {
                return false;
            };
            current = next;
        }
        false
    }

    /// Lower `value is Type`. Fully concrete values need no run-time tag, but
    /// the value expression is still evaluated for its effects. A nullable
    /// value retains one dynamic bit of information, so test its null pointer
    /// when the requested kind matches the payload kind.
    fn lower_type_check(
        &mut self,
        expr: &Expression,
        type_check_id: NodeId,
        span: &Span,
    ) -> CodegenResult<Value> {
        let actual = self.type_check_type(self.ty_of(expr)?);
        // Do not infer this from the source spelling.  In particular, generic
        // applications and aliases carry information that a runtime kind does
        // not, and nominal class checks need the actual parent relation.
        let resolved_target = self
            .l
            .sema
            .type_check_targets
            .get(&type_check_id)
            .cloned()
            .ok_or_else(|| {
                CodegenError::internal(format!(
                    "missing checker-resolved type-check target for expression {}",
                    type_check_id
                ))
            })?;
        let target = self.type_check_type(resolved_target);

        if let Type::Interface(interface_name) = &target {
            if matches!(actual, Type::Any) {
                let value = self.lower_expr_value(expr, &Type::Any)?;
                let implementations = self
                    .l
                    .sema
                    .interface_implementations
                    .get(interface_name)
                    .cloned()
                    .unwrap_or_default();
                let supports_bool = implementations
                    .iter()
                    .any(|candidate| matches!(candidate, Type::Bool));
                let integer_source_count = implementations
                    .iter()
                    .filter(|candidate| is_erased_integer_type(candidate))
                    .count();
                let supports_float = implementations
                    .iter()
                    .any(|candidate| matches!(candidate, Type::Float));
                let supports_string = implementations
                    .iter()
                    .any(|candidate| matches!(candidate, Type::String));
                let mut typed_sources: Vec<Type> = implementations
                    .iter()
                    .filter(|candidate| {
                        matches!(candidate, Type::Array(_) | Type::Struct(_) | Type::Class(_))
                    })
                    .cloned()
                    .collect();
                typed_sources.sort_by_key(Type::display_name);
                typed_sources.dedup();
                let interface_block = self.builder.create_block();
                let concrete_block = self.builder.create_block();
                let merge = self.builder.create_block();
                self.builder.append_block_param(merge, types::I8);
                let interface_kind = self.builder.ins().iconst(types::I64, 11);
                let is_interface =
                    self.call_rt_value("lira_rt_any_is", &[value, interface_kind])?;
                self.builder
                    .ins()
                    .brif(is_interface, interface_block, &[], concrete_block, &[]);
                self.terminated = true;
                self.goto(interface_block);
                let interface_value = self.call_rt_value("lira_rt_any_unbox_ref", &[value])?;
                let result = self.known_interface_implements(interface_value, interface_name)?;
                self.jump_to(merge, &[result]);
                self.goto(concrete_block);
                let mut concrete_result = None;
                for (supported, kind) in [
                    (supports_bool, 1_i64),
                    (integer_source_count == 1, 2_i64),
                    (supports_float, 3_i64),
                    (supports_string, 4_i64),
                ] {
                    if !supported {
                        continue;
                    }
                    let kind = self.builder.ins().iconst(types::I64, kind);
                    let matched = self.call_rt_value("lira_rt_any_is", &[value, kind])?;
                    concrete_result = Some(match concrete_result {
                        Some(previous) => self.builder.ins().bor(previous, matched),
                        None => matched,
                    });
                }
                for source in typed_sources {
                    let descriptor_text = self.any_type_descriptor(&source);
                    let descriptor = self.string_constant(&descriptor_text)?;
                    let matched =
                        self.call_rt_value("lira_rt_any_is_typed", &[value, descriptor])?;
                    concrete_result = Some(match concrete_result {
                        Some(previous) => self.builder.ins().bor(previous, matched),
                        None => matched,
                    });
                }
                let concrete_result =
                    concrete_result.unwrap_or_else(|| self.builder.ins().iconst(types::I8, 0));
                self.jump_to(merge, &[concrete_result]);
                self.goto(merge);
                return Ok(self.builder.block_params(merge)[0]);
            }
            if let Type::Interface(_) = &actual {
                let value = self.lower_expr_value(expr, &actual)?;
                return self.known_interface_implements(value, interface_name);
            }
            let result = self
                .l
                .sema
                .interface_implementations
                .get(interface_name)
                .is_some_and(|types| types.iter().any(|candidate| candidate == &actual));
            self.lower_expr_discard(expr)?;
            return Ok(self.builder.ins().iconst(types::I8, i64::from(result)));
        }

        if matches!(actual, Type::Any) {
            let value = self.lower_expr_value(expr, &Type::Any)?;
            if matches!(target, Type::Any) {
                return Ok(self.builder.ins().iconst(types::I8, 1));
            }
            if matches!(
                target,
                Type::Null
                    | Type::Bool
                    | Type::Int
                    | Type::Int8
                    | Type::Int16
                    | Type::Int32
                    | Type::Int64
                    | Type::UInt8
                    | Type::UInt16
                    | Type::UInt32
                    | Type::UInt64
                    | Type::Char
                    | Type::Float
                    | Type::String
            ) {
                let kind = runtime_kind_of_type(&target).ok_or_else(|| {
                    CodegenError::unsupported_at(
                        format!(
                            "type `{}` has no native runtime kind",
                            target.display_name()
                        ),
                        span,
                    )
                })?;
                let kind = self.builder.ins().iconst(types::I64, kind as i64);
                return self.call_rt_value("lira_rt_any_is", &[value, kind]);
            }
            return self.lower_any_typed_is(value, &target, span);
        }

        if let Type::Optional(inner) = &actual {
            let value = self.lower_expr_value(expr, &actual)?;
            if matches!(target, Type::Null) {
                return Ok(self.builder.ins().icmp_imm_s(IntCC::Equal, value, 0));
            }
            if let Type::Optional(target_inner) = &target {
                if self.static_is_type(inner, target_inner.as_ref()) {
                    return Ok(self.builder.ins().iconst(types::I8, 1));
                }
                return Ok(self.builder.ins().iconst(types::I8, 0));
            }
            return Ok(if self.static_is_type(inner, &target) {
                self.builder.ins().icmp_imm_s(IntCC::NotEqual, value, 0)
            } else {
                self.builder.ins().iconst(types::I8, 0)
            });
        }

        // A statically parent-typed class value can still contain a concrete
        // child. Preserve the narrowing semantics of `is` by inspecting the
        // concrete vtable, while evaluating the source exactly once. The
        // reverse relation (child is parent) is decided statically above.
        if let (Type::Class(_), Type::Class(_)) = (&actual, &target) {
            if self.is_class_upcast(&target, &actual) && !self.is_class_upcast(&actual, &target) {
                return self.lower_class_type_check(expr, &actual, &target, span);
            }
        }

        let result = self.static_is_type(&actual, &target);
        if matches!(actual, Type::Interface(_)) {
            return Err(CodegenError::unsupported_at(
                "interface type checks require an interface target",
                span,
            ));
        }
        if !result && runtime_kind_of_type(&actual).is_none() && !matches!(target, Type::Any) {
            return Err(CodegenError::unsupported_at(
                format!(
                    "a value of type `{}` needs a dynamic representation for `is`",
                    actual.display_name()
                ),
                span,
            ));
        }
        self.lower_expr_discard(expr)?;
        Ok(self.builder.ins().iconst(types::I8, i64::from(result)))
    }

    /// Restore nominal class identity after the backend's general type
    /// normalizer has resolved a user-written name to `Struct(name)`. Most
    /// native layout operations deliberately accept both spellings, but `is`
    /// needs the distinction to apply inheritance rather than exact structural
    /// identity. Recurse so descriptors inside erased containers retain the
    /// same semantic class relation.
    fn type_check_type(&self, ty: Type) -> Type {
        match ty {
            Type::Struct(name)
                if self
                    .l
                    .layouts
                    .structs
                    .get(&name)
                    .is_some_and(|layout| layout.is_class) =>
            {
                Type::Class(name)
            }
            Type::Array(inner) => Type::Array(Box::new(self.type_check_type(*inner))),
            Type::Tuple(items) => Type::Tuple(
                items
                    .into_iter()
                    .map(|item| self.type_check_type(item))
                    .collect(),
            ),
            Type::Map(key, value) => Type::Map(
                Box::new(self.type_check_type(*key)),
                Box::new(self.type_check_type(*value)),
            ),
            Type::Function {
                params,
                return_type,
                required_params,
            } => Type::Function {
                params: params
                    .into_iter()
                    .map(|param| self.type_check_type(param))
                    .collect(),
                return_type: Box::new(self.type_check_type(*return_type)),
                required_params,
            },
            Type::Channel(inner) => Type::Channel(Box::new(self.type_check_type(*inner))),
            Type::Optional(inner) => Type::Optional(Box::new(self.type_check_type(*inner))),
            Type::Result { ok_type, err_type } => Type::Result {
                ok_type: Box::new(self.type_check_type(*ok_type)),
                err_type: Box::new(self.type_check_type(*err_type)),
            },
            other => other,
        }
    }

    /// Check a class value whose static type is an ancestor of the requested
    /// class. Every concrete descendant of the target is accepted, matching
    /// the erased descriptor path while retaining native object identity.
    fn lower_class_type_check(
        &mut self,
        expr: &Expression,
        actual: &Type,
        target: &Type,
        span: &Span,
    ) -> CodegenResult<Value> {
        let Type::Class(target_name) = target else {
            return Err(CodegenError::internal(
                "class type check received a non-class target",
            ));
        };
        let value = self.lower_expr_value(expr, actual)?;
        let pointer_ty = self.pointer_ty();
        let actual_vtable = self.builder.ins().load(
            pointer_ty,
            MemFlagsData::trusted(),
            value,
            CLASS_VTABLE_OFFSET,
        );
        let mut candidates = vec![target_name.clone()];
        let descendants: Vec<String> = self
            .l
            .layouts
            .structs
            .iter()
            .filter(|(name, layout)| {
                layout.is_class
                    && name.as_str() != target_name.as_str()
                    && self.is_class_upcast(
                        &Type::Class((*name).clone()),
                        &Type::Class(target_name.clone()),
                    )
            })
            .map(|(name, _)| name.clone())
            .collect();
        candidates.extend(descendants);

        let mut result = None;
        for candidate in candidates {
            let expected = self.class_vtable(&candidate)?;
            let matched = self
                .builder
                .ins()
                .icmp(IntCC::Equal, actual_vtable, expected);
            result = Some(match result {
                Some(previous) => self.builder.ins().bor(previous, matched),
                None => matched,
            });
        }
        result.ok_or_else(|| {
            CodegenError::unsupported_at(
                format!("class `{}` has no native vtable descriptor", target_name),
                span,
            )
        })
    }

    /// Compare an erased value against the complete descriptor of a target.
    ///
    /// A class value erased as `Any` retains its concrete class descriptor. A
    /// declared-parent check therefore accepts the parent descriptor and every
    /// known child descriptor, while still evaluating the source expression
    /// only once before any descriptor probe.
    fn lower_any_typed_is(
        &mut self,
        value: Value,
        target: &Type,
        span: &Span,
    ) -> CodegenResult<Value> {
        if matches!(target, Type::Interface(_)) {
            return Err(CodegenError::unsupported_at(
                "interface type checks are not represented by the native Any descriptor ABI",
                span,
            ));
        }
        let mut candidates = vec![target.clone()];
        if let Type::Class(parent) = target {
            let descendants: Vec<Type> = self
                .l
                .layouts
                .structs
                .iter()
                .filter(|(name, layout)| {
                    layout.is_class
                        && name.as_str() != parent.as_str()
                        && self.is_class_upcast(
                            &Type::Class((*name).clone()),
                            &Type::Class(parent.clone()),
                        )
                })
                .map(|(name, _)| Type::Class(name.clone()))
                .collect();
            candidates.extend(descendants);
        }

        let mut result = None;
        for candidate in candidates {
            let descriptor = self.any_type_descriptor(&candidate);
            let descriptor = self.string_constant(&descriptor)?;
            let matched = self.call_rt_value("lira_rt_any_is_typed", &[value, descriptor])?;
            result = Some(match result {
                Some(previous) => self.builder.ins().bor(previous, matched),
                None => matched,
            });
        }
        result.ok_or_else(|| {
            CodegenError::unsupported_at(
                format!(
                    "type `{}` has no native Any descriptor",
                    target.display_name()
                ),
                span,
            )
        })
    }

    /// Semantic `is` relation for values whose representation is statically
    /// known. Numeric primitives intentionally retain the VM's coarse family
    /// semantics; nominal aggregates do not.
    fn static_is_type(&self, actual: &Type, target: &Type) -> bool {
        if matches!(target, Type::Any) {
            return true;
        }
        if actual == target {
            return true;
        }
        if matches!(actual, Type::Null) {
            return matches!(target, Type::Optional(_));
        }
        if matches!((actual, target), (Type::Class(_), Type::Class(_))) {
            return self.is_class_upcast(actual, target);
        }
        runtime_kind_of_type(actual).is_some()
            && runtime_kind_of_type(actual) == runtime_kind_of_type(target)
            && !matches!(
                (actual, target),
                (
                    Type::Array(_)
                        | Type::Tuple(_)
                        | Type::Map(_, _)
                        | Type::Function { .. }
                        | Type::Channel(_)
                        | Type::Struct(_)
                        | Type::Class(_)
                        | Type::Enum(_)
                        | Type::Result { .. },
                    _
                )
            )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
enum RuntimeKind {
    Null,
    Bool,
    Int,
    Float,
    String,
    Array,
    Object,
    Function,
    Tuple,
    Channel,
}

/// The bytecode VM's run-time type tags, which define `is` semantics.
fn runtime_kind_of_type(ty: &Type) -> Option<RuntimeKind> {
    Some(match ty {
        Type::Null => RuntimeKind::Null,
        Type::Bool => RuntimeKind::Bool,
        Type::Int
        | Type::Int8
        | Type::Int16
        | Type::Int32
        | Type::Int64
        | Type::UInt8
        | Type::UInt16
        | Type::UInt32
        | Type::UInt64
        | Type::Char => RuntimeKind::Int,
        Type::Float => RuntimeKind::Float,
        Type::String => RuntimeKind::String,
        Type::Array(_) => RuntimeKind::Array,
        Type::Tuple(_) => RuntimeKind::Tuple,
        Type::Channel(_) => RuntimeKind::Channel,
        Type::Function { .. } => RuntimeKind::Function,
        Type::Map(_, _)
        | Type::Struct(_)
        | Type::Class(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Result { .. } => RuntimeKind::Object,
        Type::Optional(_) | Type::Any | Type::Unknown | Type::TypeVar(_) | Type::TypeParam(_) => {
            return None
        }
        Type::Void => return None,
    })
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
                if let Some(result) =
                    self.lower_generic_call(&name.clone(), None, None, args, &[], span)?
                {
                    return Ok(result);
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
        self.lower_method_call_with_type_args(receiver, method, args, &[], span)
    }

    fn lower_method_call_with_type_args(
        &mut self,
        receiver: &Expression,
        method: &str,
        args: &[Argument],
        type_args: &[Type],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        // `super.method()` is a direct call to the parent's implementation, not
        // a dispatch — that is the whole point of writing it.
        if matches!(&receiver.kind, ExpressionKind::Identifier(name) if name == "super") {
            if !type_args.is_empty() {
                return Err(CodegenError::unsupported_at(
                    "explicit type arguments on `super` are not supported",
                    span,
                ));
            }
            return self.lower_super_call(method, args, span);
        }

        // `Counter.new()` and `Counter::new()` both reach here as a call on a
        // bare type name. A local variable of the same name wins.
        if let ExpressionKind::Identifier(name) = &receiver.kind {
            let owner = self.l.layouts.canonical_impl_owner(name);
            if self.lookup(name).is_none()
                && (self.l.layouts.is_aggregate(&owner)
                    || self.l.layouts.generics.contains_key(&owner)
                    || self.l.funcs.contains_key(&fn_key(Some(&owner), method)))
            {
                if self.l.layouts.enums.contains_key(&owner) {
                    return self
                        .lower_enum_construction(&owner, method, args, span)
                        .map(Some);
                }
                let key = fn_key(Some(&owner), method);
                if self.l.generic_index.contains_key(&key) {
                    return match self.lower_generic_call(&key, None, None, args, type_args, span)? {
                        Some(result) => Ok(result),
                        None => Err(CodegenError::unsupported_at(
                            format!("unknown static method `{}`", method),
                            span,
                        )),
                    };
                }
                return self.lower_user_call(&key, None, args, span);
            }
        }

        let receiver_ty = self.ty_of(receiver)?;
        match &receiver_ty {
            Type::Interface(interface_name) => {
                if !type_args.is_empty() {
                    return Err(CodegenError::unsupported_at(
                        "generic methods on interfaces are not lowered yet",
                        span,
                    ));
                }
                let interface = self
                    .l
                    .layouts
                    .interfaces
                    .get(interface_name)
                    .cloned()
                    .ok_or_else(|| {
                        CodegenError::unsupported_at(
                            format!("unknown interface `{interface_name}`"),
                            span,
                        )
                    })?;
                let method_layout = interface.method(method).cloned().ok_or_else(|| {
                    CodegenError::unsupported_at(
                        format!("interface `{interface_name}` has no method `{method}`"),
                        span,
                    )
                })?;
                let receiver_value = self.lower_expr_value(receiver, &receiver_ty)?;
                let explicit = &method_layout.params[1..];
                let mut slots: Vec<Option<Value>> = vec![None; explicit.len()];
                let mut positional = 0usize;
                for arg in args {
                    let index = match &arg.name {
                        Some(name) => explicit
                            .iter()
                            .position(|param| param.name == *name)
                            .ok_or_else(|| {
                                CodegenError::unsupported_at(
                                    format!("`{method}` has no parameter named `{name}`"),
                                    &arg.span,
                                )
                            })?,
                        None => {
                            let index = positional;
                            positional += 1;
                            if index >= explicit.len() {
                                return Err(CodegenError::unsupported_at(
                                    format!("too many arguments for `{method}`"),
                                    &arg.span,
                                ));
                            }
                            index
                        }
                    };
                    if slots[index].is_some() {
                        return Err(CodegenError::unsupported_at(
                            format!(
                                "argument `{}` was provided more than once",
                                explicit[index].name
                            ),
                            &arg.span,
                        ));
                    }
                    slots[index] =
                        Some(self.lower_call_argument_value(&arg.value, &explicit[index].ty)?);
                }
                let mut call_args = Vec::with_capacity(method_layout.params.len());
                call_args.push(receiver_value);
                for (index, param) in explicit.iter().enumerate() {
                    call_args.push(match slots[index] {
                        Some(value) => value,
                        None => match &param.default {
                            Some(default) => self.lower_expr_value(default, &param.ty)?,
                            None => {
                                return Err(CodegenError::unsupported_at(
                                    format!("missing argument `{}` for `{method}`", param.name),
                                    span,
                                ));
                            }
                        },
                    });
                }
                let mut sig = Signature::new(self.l.call_conv);
                for param in &method_layout.params {
                    sig.params.push(AbiParam::new(
                        repr_of(&param.ty)?.clif(self.pointer_ty()).ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("interface method `{method}` has a void parameter"),
                                span,
                            )
                        })?,
                    ));
                }
                let ret = method_return(&method_layout.signature);
                if let Some(clif) = repr_of(&ret)?.clif(self.pointer_ty()) {
                    sig.returns.push(AbiParam::new(clif));
                }
                let sig_ref = self.builder.import_signature(sig);
                let method_index = self
                    .builder
                    .ins()
                    .iconst(types::I32, method_layout.slot as i64);
                let slot = self.call_rt_value(
                    "lira_rt_interface_method_slot",
                    &[receiver_value, method_index],
                )?;
                let call = self.builder.ins().call_indirect(sig_ref, slot, &call_args);
                let result = self.builder.inst_results(call).first().copied();
                return Ok(if matches!(ret, Type::Void) {
                    None
                } else {
                    result
                });
            }
            Type::Array(element_ty) => {
                let element_ty = (**element_ty).clone();
                // A user `impl [int]` / `impl array` method wins over nothing;
                // the three built-in operations stay built in.
                if let Some(key) = self.builtin_impl_key(&receiver_ty, method) {
                    let self_value = self.method_receiver_for_key(receiver, &receiver_ty, &key)?;
                    return self.lower_user_call(&key, self_value, args, span);
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
                    let self_value = self.method_receiver_for_key(receiver, &receiver_ty, &key)?;
                    return self.lower_user_call(&key, self_value, args, span);
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
            Type::Struct(name) | Type::Class(name) | Type::Enum(name) => {
                let name = name.clone();
                // A class dispatches through its table; a struct calls directly.
                if self
                    .l
                    .layouts
                    .structs
                    .get(&name)
                    .is_some_and(|layout| layout.is_class)
                {
                    let self_value = self.lower_expr_value(receiver, &receiver_ty)?;
                    return self.lower_virtual_call(&name, method, self_value, args, span);
                }
                if let Some(key) = self.resolve_method(&name, method) {
                    let self_value = self.method_receiver_for_key(receiver, &receiver_ty, &key)?;
                    return self.lower_user_call(&key, self_value, args, span);
                }
                if let Some(template_name) = self.l.generic_template_name(&name) {
                    let key = fn_key(Some(template_name), method);
                    let self_value = self.method_receiver_value(receiver, &receiver_ty, &key)?;
                    if let Some(result) = self.lower_generic_call(
                        &key,
                        Some(self_value),
                        Some(&receiver_ty),
                        args,
                        type_args,
                        span,
                    )? {
                        return Ok(result);
                    }
                }
                return Err(CodegenError::unsupported_at(
                    format!("`{}` has no method `{}`", name, method),
                    span,
                ));
            }
            Type::Any => {
                let receiver = self.lower_expr_value(receiver, &Type::Any)?;
                return match (method, args) {
                    ("len", []) => Ok(Some(self.call_rt_value("lira_rt_any_len", &[receiver])?)),
                    ("push", [arg]) => {
                        let value = self.lower_expr_value(&arg.value, &Type::Any)?;
                        self.call_rt("lira_rt_any_push", &[receiver, value])?;
                        Ok(None)
                    }
                    ("pop", []) => Ok(Some(self.call_rt_value("lira_rt_any_pop", &[receiver])?)),
                    _ => Err(CodegenError::unsupported_at(
                        format!("`any.{}` is not lowered by the native backend yet", method),
                        span,
                    )),
                };
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
            ("pop", 0) => Ok(Some(self.lower_array_pop(array, element_ty, span)?)),
            _ => Err(CodegenError::unsupported_at(
                format!(
                    "`array.{}` is not lowered by the native backend yet",
                    method
                ),
                span,
            )),
        }
    }

    /// Pop from a statically typed array while preserving the `T?` contract.
    ///
    /// The raw array helper intentionally rejects an empty array, so the
    /// length check must happen in generated code before calling it.  Present
    /// values pass through `wrap_optional`, which boxes scalars and leaves
    /// references nullable; its coercion boundary also performs the semantic
    /// copy required for value structs without copying class identity.
    fn lower_array_pop(
        &mut self,
        array: Value,
        element_ty: &Type,
        span: &Span,
    ) -> CodegenResult<Value> {
        let empty = self.builder.create_block();
        let present = self.builder.create_block();
        let merge = self.builder.create_block();
        let ptr = self.pointer_ty();
        self.builder.append_block_param(merge, ptr);

        let len = self.call_rt_value("lira_rt_array_len", &[array])?;
        let is_empty = self.builder.ins().icmp_imm_s(IntCC::Equal, len, 0);
        self.builder.ins().brif(is_empty, empty, &[], present, &[]);
        self.terminated = true;

        self.goto(empty);
        let null = self.zero_of(Repr::Ref);
        self.jump_to(merge, &[null]);

        self.goto(present);
        let slot = self.call_rt_value("lira_rt_array_pop", &[array])?;
        let value = self.slot_to_value(slot, element_ty)?;
        let wrapped = self.wrap_optional(value, element_ty, element_ty, span)?;
        self.jump_to(merge, &[wrapped]);

        self.goto(merge);
        Ok(self.builder.block_params(merge)[0])
    }

    /// Call a user function or method, filling in defaults and reordering
    /// named arguments to match the declaration.
    fn build_user_call_args(
        &mut self,
        key: &str,
        self_value: Option<Value>,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<(Vec<Value>, Type)> {
        let info = self.l.funcs.get(key).ok_or_else(|| {
            CodegenError::unsupported_at(format!("unknown function `{}`", key), span)
        })?;
        let ret = info.ret.clone();
        let takes_self =
            info.owner.is_some() && info.params.first().is_some_and(|p| is_receiver(&p.name));
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
            let expected_ty = &explicit[index].1;
            slots[index] = Some(self.lower_call_argument_value(&arg.value, expected_ty)?);
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
        Ok((call_args, ret))
    }

    /// Lower one explicit source argument across a call boundary.
    ///
    /// Concrete values headed for `Any` must retain their concrete type until
    /// boxing so the runtime descriptor remains precise. An already-erased
    /// value receives an independent `Any` wrapper at every language-level
    /// call boundary; reference payloads inside that wrapper intentionally
    /// remain shared.
    fn lower_call_argument_value(
        &mut self,
        expression: &Expression,
        expected_ty: &Type,
    ) -> CodegenResult<Value> {
        let source_ty = self.ty_of(expression)?;
        let mut value = if matches!(expected_ty, Type::Any) && !matches!(source_ty, Type::Any) {
            let value = self.lower_expr_value(expression, &source_ty)?;
            self.coerce(value, &source_ty, expected_ty, &expression.span)?
        } else {
            self.lower_expr_value(expression, expected_ty)?
        };
        if matches!(expected_ty, Type::Any) && matches!(source_ty, Type::Any) {
            value = self.copy_any_boundary(value)?;
        }
        Ok(value)
    }

    fn lower_user_call(
        &mut self,
        key: &str,
        self_value: Option<Value>,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        let (call_args, ret) = self.build_user_call_args(key, self_value, args, span)?;
        let func_id = self
            .l
            .funcs
            .get(key)
            .ok_or_else(|| CodegenError::internal(format!("`{}` disappeared", key)))?
            .func_id;

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

    /// Lower a value crossing a channel boundary.
    ///
    /// Typed structs are copied by `coerce`, while a freshly boxed concrete
    /// value is copied by `box_any`. The remaining case is an already-erased
    /// `Any`: its box may contain a value struct, so it needs the same semantic
    /// copy used by other `Any -> Any` boundaries before the channel retains
    /// the slot. Reference payloads remain aliases inside `copy_any_boundary`.
    fn lower_channel_payload(
        &mut self,
        expr: &Expression,
        element_ty: &Type,
    ) -> CodegenResult<Value> {
        let source_ty = self.ty_of(expr)?;
        let mut value = self.lower_expr_value(expr, element_ty)?;
        if matches!(source_ty, Type::Any) && matches!(element_ty, Type::Any) {
            value = self.copy_any_boundary(value)?;
        }
        Ok(value)
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
            "assert" => {
                if args.len() != 1 {
                    return Err(arity_error(1));
                }
                let condition = self.lower_expr_value(&args[0].value, &Type::Bool)?;
                let passed = self.builder.create_block();
                let failed = self.builder.create_block();
                self.builder.ins().brif(condition, passed, &[], failed, &[]);
                self.terminated = true;

                self.goto(failed);
                let message = self.string_constant("assertion failed")?;
                self.call_rt("lira_rt_abort", &[message])?;
                self.builder.ins().trap(unreachable_trap());
                self.terminated = true;

                self.goto(passed);
                BuiltinResult::Void
            }
            "print" | "println" => {
                if args.len() != 1 {
                    return Err(arity_error(1));
                }
                let arg = &args[0].value;
                let arg_ty = self.ty_of(arg)?;
                let value = self.lower_expr_value(arg, &arg_ty)?;
                // Reference aggregates need their complete descriptor to
                // render fields/elements safely. Route every supported static
                // aggregate (including optionals, functions and channels)
                // through the same bounded Any renderer used by dynamic code.
                let (arg_ty, value) = if matches!(
                    &arg_ty,
                    Type::Array(_)
                        | Type::Tuple(_)
                        | Type::Map(_, _)
                        | Type::Struct(_)
                        | Type::Class(_)
                        | Type::Enum(_)
                        | Type::Result { .. }
                        | Type::Function { .. }
                        | Type::Channel(_)
                        | Type::Optional(_)
                ) {
                    (Type::Any, self.box_any(value, &arg_ty, span)?)
                } else {
                    (arg_ty, value)
                };
                // A nullable reference prints like the reference it wraps.
                let ty = strip_optional(&arg_ty).clone();
                // The argument's static type picks the runtime entry point, so
                // there is no dispatch at run time. Core `print` is
                // newline-free; `println` selects the corresponding newline
                // entry point.
                let prefix = if name == "print" {
                    "lira_rt_print"
                } else {
                    "lira_rt_println"
                };
                let (symbol, value) = match repr_of(&ty)? {
                    _ if matches!(ty, Type::Any) => (
                        format!("{prefix}_str"),
                        self.call_rt_value("lira_rt_any_to_string", &[value])?,
                    ),
                    _ if matches!(ty, Type::String | Type::Null) => {
                        (format!("{prefix}_str"), value)
                    }
                    Repr::Int => (format!("{prefix}_int"), value),
                    Repr::Float => (format!("{prefix}_float"), value),
                    Repr::Bool => (format!("{prefix}_bool"), value),
                    // An optional renders through the string path, which knows
                    // how to say "null".
                    _ if matches!(arg_ty, Type::Optional(_)) => (
                        format!("{prefix}_str"),
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
                self.call_rt(&symbol, &[value])?;
                BuiltinResult::Void
            }

            "len" => {
                if args.len() != 1 {
                    return Err(arity_error(1));
                }
                let arg = &args[0].value;
                let declared = self.ty_of(arg)?;
                let value = self.lower_expr_value(arg, &declared)?;
                // `string?` measures like the `string` it wraps.
                let ty = strip_optional(&declared).clone();
                let symbol = match ty {
                    Type::Any => "lira_rt_any_len",
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
                if matches!(receiver_ty, Type::Any) {
                    let array = self.lower_expr_value(&receiver.value, &Type::Any)?;
                    return match (name, &args[1..]) {
                        ("push", [arg]) => {
                            let value = self.lower_expr_value(&arg.value, &Type::Any)?;
                            self.call_rt("lira_rt_any_push", &[array, value])?;
                            Ok(Some(BuiltinResult::Void))
                        }
                        ("pop", []) => Ok(Some(BuiltinResult::Value(
                            self.call_rt_value("lira_rt_any_pop", &[array])?,
                        ))),
                        ("push", _) => Err(arity_error(2)),
                        ("pop", _) => Err(arity_error(1)),
                        _ => unreachable!("matched only push/pop"),
                    };
                }
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
                let channel_ty = self.ty_of(&args[0].value)?;
                let (channel, element_ty) = match &channel_ty {
                    Type::Channel(element)
                        if matches!(element.as_ref(), Type::Unknown | Type::TypeVar(_)) =>
                    {
                        (
                            self.lower_expr_value(&args[0].value, &channel_ty)?,
                            Type::Any,
                        )
                    }
                    Type::Channel(element) => (
                        self.lower_expr_value(&args[0].value, &channel_ty)?,
                        element.as_ref().clone(),
                    ),
                    Type::Any => {
                        let boxed = self.lower_expr_value(&args[0].value, &Type::Any)?;
                        (
                            self.call_rt_value("lira_rt_any_unbox_channel", &[boxed])?,
                            Type::Any,
                        )
                    }
                    _ => {
                        return Err(CodegenError::unsupported_at(
                            "`send` expects a channel",
                            span,
                        ))
                    }
                };
                let value = self.lower_channel_payload(&args[1].value, &element_ty)?;
                let slot = self.value_to_slot(value, &element_ty)?;
                self.call_rt("lira_rt_chan_send", &[channel, slot])?;
                BuiltinResult::Void
            }

            "recv" => {
                if args.len() != 1 {
                    return Err(arity_error(1));
                }
                let channel_ty = self.ty_of(&args[0].value)?;
                let (channel, element_ty) = match &channel_ty {
                    Type::Channel(element)
                        if matches!(element.as_ref(), Type::Unknown | Type::TypeVar(_)) =>
                    {
                        (
                            self.lower_expr_value(&args[0].value, &channel_ty)?,
                            Type::Any,
                        )
                    }
                    Type::Channel(element) => (
                        self.lower_expr_value(&args[0].value, &channel_ty)?,
                        element.as_ref().clone(),
                    ),
                    Type::Any => {
                        let boxed = self.lower_expr_value(&args[0].value, &Type::Any)?;
                        (
                            self.call_rt_value("lira_rt_any_unbox_channel", &[boxed])?,
                            Type::Any,
                        )
                    }
                    _ => {
                        return Err(CodegenError::unsupported_at(
                            "`recv` expects a channel",
                            span,
                        ))
                    }
                };
                let slot = self.call_rt_value("lira_rt_chan_recv", &[channel])?;
                let value = if matches!(element_ty, Type::Any) {
                    self.call_rt_value("lira_rt_any_from_slot", &[slot])?
                } else {
                    self.slot_to_value(slot, &element_ty)?
                };
                BuiltinResult::Value(value)
            }

            "close" => {
                if args.len() != 1 {
                    return Err(arity_error(1));
                }
                let channel_ty = self.ty_of(&args[0].value)?;
                let channel = match &channel_ty {
                    Type::Channel(_) => self.lower_expr_value(&args[0].value, &channel_ty)?,
                    Type::Any => {
                        let boxed = self.lower_expr_value(&args[0].value, &Type::Any)?;
                        self.call_rt_value("lira_rt_any_unbox_channel", &[boxed])?
                    }
                    _ => {
                        return Err(CodegenError::unsupported_at(
                            "`close` expects a channel",
                            span,
                        ))
                    }
                };
                self.call_rt("lira_rt_chan_close", &[channel])?;
                BuiltinResult::Void
            }

            "fiber_yield" => {
                self.call_rt("lira_rt_yield", &[])?;
                BuiltinResult::Void
            }
            "fiber_id" => BuiltinResult::Value(self.call_rt_value("lira_rt_fiber_id", &[])?),

            "collect" => {
                if !args.is_empty() {
                    return Err(arity_error(0));
                }
                self.call_rt("lira_rt_collect", &[])?;
                BuiltinResult::Void
            }

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
        // `Box { value: 42 }` names no type arguments; they come from the
        // values the fields are given.
        let name = &self.instantiate_from_literal(name, fields, span)?;
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
        if layout.is_class {
            // Every instance points at its class's method table, which is what
            // makes an inherited method dispatch to the concrete override.
            let vtable = self.class_vtable(name)?;
            self.builder
                .ins()
                .store(MemFlagsData::trusted(), vtable, object, CLASS_VTABLE_OFFSET);
        }
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
            // A nested struct literal allocates a new object as part of this
            // expression, so an exact-type value-struct literal cannot alias
            // anything that existed before this field is stored. Avoid the
            // normal value-boundary copy for that one provably fresh case.
            // Identifiers, field reads, calls, and every other expression
            // retain the ordinary copy boundary below.
            let fresh_exact_value_struct = match &expr.kind {
                ExpressionKind::StructLiteral {
                    name: Some(literal_name),
                    ..
                } => {
                    let literal_ty = Type::Struct(literal_name.clone());
                    literal_ty == field_ty
                        && self
                            .l
                            .layouts
                            .structs
                            .get(literal_name)
                            .is_some_and(|literal_layout| !literal_layout.is_class)
                        && self.is_value_struct_type(&literal_ty)
                }
                _ => false,
            };
            let value = if fresh_exact_value_struct {
                self.lower_expr(expr)?.ok_or_else(|| {
                    CodegenError::unsupported_at(
                        "a fresh struct literal field needs a value",
                        &expr.span,
                    )
                })?
            } else {
                self.lower_expr_value(expr, &field_ty)?
            };
            self.store_at(object, offset, &field_ty, value)?;
            initialised.insert(field_name.clone());
        }
        // `lira_rt_alloc` zeroes, so an omitted field reads as 0/false/null
        // rather than as garbage; the checker is what rejects real omissions.
        Ok(object)
    }

    /// If `name` is a generic type, work out its arguments from the field values
    /// and instantiate it, returning the concrete name.
    fn instantiate_from_literal(
        &mut self,
        name: &str,
        fields: &[(String, Expression)],
        span: &Span,
    ) -> CodegenResult<String> {
        let Some(template) = self.l.layouts.generics.get(name).cloned() else {
            return Ok(name.to_string());
        };
        let in_scope: HashSet<String> = template.type_params.iter().cloned().collect();
        let mut bindings = HashMap::new();
        for (field_name, declared) in &template.fields {
            let Some((_, value)) = fields.iter().find(|(given, _)| given == field_name) else {
                continue;
            };
            let resolve = |name: &str| self.binding_type(name);
            unify_declared_expression(self.l, &resolve, declared, value, &in_scope, &mut bindings)
                .ok_or_else(|| {
                    CodegenError::unsupported_at(
                        format!("cannot infer the type of field `{}`", field_name),
                        span,
                    )
                })?;
        }
        let args: Option<Vec<Type>> = template
            .type_params
            .iter()
            .map(|param| bindings.get(param).cloned())
            .collect();
        let args = args.ok_or_else(|| {
            CodegenError::unsupported_at(
                format!(
                    "cannot work out the type arguments for `{}` from these fields",
                    name
                ),
                span,
            )
        })?;
        let ty = self.l.instantiate_type(name, &args, span)?;
        match ty {
            Type::Struct(mangled) | Type::Enum(mangled) | Type::Class(mangled) => Ok(mangled),
            other => Err(CodegenError::internal(format!(
                "instantiating `{}` gave a `{}`",
                name,
                other.display_name()
            ))),
        }
    }

    /// Read an aggregate expression for lvalue traversal without crossing a
    /// semantic value boundary. This is deliberately separate from
    /// `lower_expr_value`: a write such as `tuple[0].field = value` must load
    /// the original tuple slot, while an ordinary rvalue extraction still
    /// copies the tuple and nested value structs.
    fn lower_lvalue_value(&mut self, expr: &Expression) -> CodegenResult<Value> {
        match &expr.kind {
            ExpressionKind::Identifier(name) => {
                let binding = self.lookup(name).ok_or_else(|| {
                    CodegenError::unsupported_at(format!("unknown name `{name}`"), &expr.span)
                })?;
                Ok(self.load_binding(&binding))
            }
            ExpressionKind::FieldAccess { object, field } => {
                let object_ty = self.ty_of(object)?;
                if matches!(object_ty, Type::Any) {
                    let object = self.lower_lvalue_value(object)?;
                    let key = self.string_constant(field)?;
                    let key = self.call_rt_value("lira_rt_any_box_string", &[key])?;
                    return self.call_rt_value("lira_rt_any_index", &[object, key]);
                }
                let (Type::Struct(name) | Type::Class(name)) = object_ty else {
                    return Err(CodegenError::unsupported_at(
                        format!(
                            "cannot read field `{field}` from a `{}`",
                            object_ty.display_name()
                        ),
                        &expr.span,
                    ));
                };
                let base = self.lower_lvalue_value(object)?;
                let layout = self.l.layouts.structs.get(&name).ok_or_else(|| {
                    CodegenError::unsupported_at(format!("unknown type `{name}`"), &expr.span)
                })?;
                let field_layout = layout.field(field).ok_or_else(|| {
                    CodegenError::unsupported_at(
                        format!("`{name}` has no field `{field}`"),
                        &expr.span,
                    )
                })?;
                let field_ty = self.l.normalize(field_layout.ty.clone());
                self.load_at(base, field_layout.offset, &field_ty)
            }
            ExpressionKind::Index { object, index } => {
                let object_ty = self.ty_of(object)?;
                match object_ty {
                    Type::Any => {
                        let object = self.lower_lvalue_value(object)?;
                        let key = self.lower_expr_value(index, &Type::Any)?;
                        self.call_rt_value("lira_rt_any_index", &[object, key])
                    }
                    Type::Array(element_ty) => {
                        let array = self.lower_lvalue_value(object)?;
                        let index = self.lower_expr_value(index, &Type::Int)?;
                        let slot = self.call_rt_value("lira_rt_array_get", &[array, index])?;
                        self.slot_to_value(slot, &element_ty)
                    }
                    Type::Map(_, value_ty) => {
                        let map = self.lower_lvalue_value(object)?;
                        let key = self.lower_expr_value(index, &Type::String)?;
                        let slot = self.call_rt_value("lira_rt_map_get", &[map, key])?;
                        self.slot_to_value(slot, &value_ty)
                    }
                    Type::Tuple(element_types) => {
                        let ExpressionKind::IntLiteral(position) = index.kind else {
                            return Err(CodegenError::unsupported_at(
                                "a tuple can only be indexed by a literal position",
                                &expr.span,
                            ));
                        };
                        let element_ty = element_types.get(position as usize).ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("this tuple has no position {position}"),
                                &expr.span,
                            )
                        })?;
                        let tuple = self.lower_lvalue_value(object)?;
                        let position = self.builder.ins().iconst(types::I64, position);
                        let slot = self.call_rt_value("lira_rt_array_get", &[tuple, position])?;
                        self.slot_to_value(slot, element_ty)
                    }
                    other => Err(CodegenError::unsupported_at(
                        format!("cannot index a value of type `{}`", other.display_name()),
                        &expr.span,
                    )),
                }
            }
            _ => self.lower_expr(expr)?.ok_or_else(|| {
                CodegenError::unsupported_at("an lvalue receiver needs a value", &expr.span)
            }),
        }
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
        // Lower the receiver before consulting its layout. A generic function
        // call can return `Box$T` whose concrete layout is created by
        // `instantiate_fn`; looking up the field first would report an
        // unknown type even though the call is fully inferable.
        // This is address computation, not a value boundary. In particular,
        // `outer.inner.x = ...` must follow the original `inner` pointer;
        // cloning `outer.inner` here would detach the write from `outer`.
        let base = self.lower_lvalue_value(object)?;
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
        Ok((base, offset, field_ty))
    }

    fn lower_enum_construction(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Value> {
        self.lower_enum_construction_expected(enum_name, variant_name, args, None, span)
    }

    fn lower_expected_enum_construction(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        args: &[Argument],
        expected: &Type,
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        let Some(expected_name) = (match expected {
            Type::Enum(name) | Type::Struct(name) if self.l.layouts.enums.contains_key(name) => {
                Some(name.as_str())
            }
            _ => None,
        }) else {
            return Ok(None);
        };
        let Some(template_name) = self.l.generic_template_name(expected_name) else {
            return Ok(None);
        };
        if template_name != enum_name {
            return Ok(None);
        }
        self.lower_enum_construction_expected(
            enum_name,
            variant_name,
            args,
            Some(expected_name),
            span,
        )
        .map(Some)
    }

    fn lower_enum_construction_expected(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        args: &[Argument],
        expected: Option<&str>,
        span: &Span,
    ) -> CodegenResult<Value> {
        // `Opt::Some(42)` names no type arguments; the payload supplies them.
        let enum_name = match expected {
            Some(name) => name.to_string(),
            None => self.instantiate_enum_from_args(enum_name, variant_name, args, span)?,
        };
        let layout = self
            .l
            .layouts
            .enums
            .get(&enum_name)
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

    /// Instantiate a generic enum from the payload a variant was given.
    ///
    /// A payload-free variant such as `Opt::None` cannot say what `T` is on its
    /// own; the type it is assigned to has to, which is why the expected type is
    /// pushed down to enum construction.
    fn instantiate_enum_from_args(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<String> {
        let Some(template) = self.l.layouts.generics.get(enum_name).cloned() else {
            return Ok(enum_name.to_string());
        };
        let in_scope: HashSet<String> = template.type_params.iter().cloned().collect();
        let mut bindings = HashMap::new();
        if let Some((_, payloads)) = template
            .variants
            .iter()
            .find(|(name, _)| name == variant_name)
        {
            for (declared, arg) in payloads.iter().zip(args) {
                let resolve = |name: &str| self.binding_type(name);
                unify_declared_expression(
                    self.l,
                    &resolve,
                    declared,
                    &arg.value,
                    &in_scope,
                    &mut bindings,
                )
                .ok_or_else(|| {
                    CodegenError::unsupported_at("cannot infer a generic enum payload type", span)
                })?;
            }
        }
        let arguments: Vec<Type> = template
            .type_params
            .iter()
            .map(|param| {
                bindings
                    .get(param)
                    .cloned()
                    // A variant that carries nothing of this parameter leaves it
                    // free; an integer slot is as good as any and never read.
                    .unwrap_or(Type::Int)
            })
            .collect();
        let ty = self.l.instantiate_type(enum_name, &arguments, span)?;
        match ty {
            Type::Struct(mangled) | Type::Enum(mangled) | Type::Class(mangled) => Ok(mangled),
            other => Err(CodegenError::internal(format!(
                "instantiating `{}` gave a `{}`",
                enum_name,
                other.display_name()
            ))),
        }
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
                if matches!(self.ty_of(object)?, Type::Any) {
                    // This is address/lvalue traversal. Do not apply the
                    // Any semantic-copy boundary to the receiver or the
                    // assignment would mutate a detached struct snapshot.
                    let object = self.lower_lvalue_value(object)?;
                    let key = self.string_constant(field)?;
                    let key = self.call_rt_value("lira_rt_any_box_string", &[key])?;
                    self.call_rt("lira_rt_any_set", &[object, key, value])?;
                    return Ok(());
                }
                let (base, offset, field_ty) = self.field_address(object, field, span)?;
                // Assignment lowering normally coerces the RHS before it
                // reaches this method. Re-apply the semantic boundary here as
                // well: a recursive field can be recorded as dynamic by the
                // checker, but its resolved layout still requires a snapshot
                // when stored in a value-struct field.
                let value = match &field_ty {
                    Type::Struct(name) | Type::Class(name)
                        if self
                            .l
                            .layouts
                            .structs
                            .get(name)
                            .is_some_and(|layout| !layout.is_class) =>
                    {
                        let helper_id = self.l.ensure_copy_helper(name)?;
                        let copy_ctx = self.call_rt_value("lira_rt_copy_ctx_new", &[])?;
                        let helper = self.func_ref_by_id(helper_id);
                        let call = self.builder.ins().call(helper, &[value, copy_ctx]);
                        let copied = self.builder.inst_results(call)[0];
                        self.call_rt("lira_rt_copy_ctx_free", &[copy_ctx])?;
                        copied
                    }
                    Type::Optional(inner) => match inner.as_ref() {
                        Type::Struct(name) | Type::Class(name)
                            if self
                                .l
                                .layouts
                                .structs
                                .get(name)
                                .is_some_and(|layout| !layout.is_class) =>
                        {
                            let helper_id = self.l.ensure_copy_helper(name)?;
                            let copy_ctx = self.call_rt_value("lira_rt_copy_ctx_new", &[])?;
                            let helper = self.func_ref_by_id(helper_id);
                            let call = self.builder.ins().call(helper, &[value, copy_ctx]);
                            let copied = self.builder.inst_results(call)[0];
                            self.call_rt("lira_rt_copy_ctx_free", &[copy_ctx])?;
                            copied
                        }
                        _ => value,
                    },
                    _ => value,
                };
                self.store_at(base, offset, &field_ty, value)
            }
            ExpressionKind::Index { object, index } => {
                let object_ty = self.ty_of(object)?;
                match object_ty.clone() {
                    Type::Any => {
                        // Preserve the original aggregate for a write; the
                        // value boundary belongs to the RHS, not traversal.
                        let object = self.lower_lvalue_value(object)?;
                        let key = self.lower_expr_value(index, &Type::Any)?;
                        self.call_rt("lira_rt_any_set", &[object, key, value])?;
                        Ok(())
                    }
                    Type::Array(element_ty) => {
                        let array = self.lower_lvalue_value(object)?;
                        let index = self.lower_expr_value(index, &Type::Int)?;
                        let slot = self.value_to_slot(value, &element_ty)?;
                        self.call_rt("lira_rt_array_set", &[array, index, slot])?;
                        Ok(())
                    }
                    Type::Map(_, value_ty) => {
                        let value_ty = self.l.normalize(*value_ty);
                        let map = self.lower_lvalue_value(object)?;
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

    /// Join one conditional-expression arm at its value merge.
    ///
    /// An unannotated function has the dynamic return type `any`; falling off
    /// an effect-only arm therefore contributes the canonical dynamic `null`,
    /// just as falling off the whole function does. Emitting an argument-less
    /// jump to a value-bearing merge is invalid Cranelift IR, so keep the
    /// representation invariant explicit here.
    fn jump_expression_result(
        &mut self,
        merge: Block,
        value: Option<Value>,
        result_ty: &Type,
        span: &Span,
    ) -> CodegenResult<()> {
        if let Some(value) = value {
            self.jump_to(merge, &[value]);
            return Ok(());
        }
        if matches!(result_ty, Type::Any) {
            let null = self.call_rt_value("lira_rt_any_null", &[])?;
            self.jump_to(merge, &[null]);
            return Ok(());
        }
        if repr_of(result_ty)?.clif(self.pointer_ty()).is_none() {
            self.jump_to(merge, &[]);
            return Ok(());
        }
        Err(CodegenError::internal(format!(
            "{}:{}: a `{}` conditional arm lowered as `void`",
            span.line,
            span.column,
            result_ty.display_name()
        )))
    }

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
                self.jump_expression_result(merge, value, result_ty, &expr.span)?;
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
            self.test_pattern_alternatives(&arm.pattern, subject_value, &subject_ty, fail)?;

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
                self.jump_expression_result(merge, value, result_ty, &arm.body.span)?;
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
    fn test_pattern_alternatives(
        &mut self,
        pattern: &Pattern,
        subject: Value,
        subject_ty: &Type,
        fail: Block,
    ) -> CodegenResult<()> {
        let alternatives = expand_or_pattern(pattern)
            .map_err(|message| CodegenError::unsupported_at(message, &pattern.span))?;
        let Some(canonical) = alternatives.first() else {
            return Err(CodegenError::unsupported_at(
                "or-pattern has no alternatives",
                &pattern.span,
            ));
        };
        if alternatives.len() == 1 {
            return self.test_pattern(canonical, subject, subject_ty, fail);
        }

        let mut bindings = Vec::new();
        collect_pattern_binding_specs(canonical, self.l.sema, &mut bindings)?;
        let matched = self.builder.create_block();
        for (_, ty) in &bindings {
            let clif = repr_of(ty)?.clif(self.pointer_ty()).ok_or_else(|| {
                CodegenError::unsupported_at(
                    "an or-pattern cannot bind a void value",
                    &pattern.span,
                )
            })?;
            self.builder.append_block_param(matched, clif);
        }

        for alternative in alternatives {
            let next = self.builder.create_block();
            self.push_scope();
            self.test_pattern(&alternative, subject, subject_ty, next)?;
            let mut values = Vec::with_capacity(bindings.len());
            for (name, _) in &bindings {
                let binding = self.lookup(name).ok_or_else(|| {
                    CodegenError::internal(format!(
                        "validated or-pattern alternative did not bind `{name}`"
                    ))
                })?;
                values.push(self.load_binding(&binding));
            }
            self.jump_to(matched, &values);
            self.pop_scope();
            self.goto(next);
        }

        self.jump_to(fail, &[]);
        self.goto(matched);
        let values = self.builder.block_params(matched).to_vec();
        for ((name, ty), value) in bindings.into_iter().zip(values) {
            self.declare_local(&name, ty, Some(value))?;
        }
        Ok(())
    }

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
                let subject = self.copy_value_boundary(subject, subject_ty)?;
                self.declare_local(name, subject_ty.clone(), Some(subject))?;
                Ok(())
            }

            PatternKind::Binding { name, pattern } => {
                let subject = self.copy_value_boundary(subject, subject_ty)?;
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
                // Match lowering expands nested ORs before testing. Retain a
                // binding-free fallback for defensive callers.
                if alternatives.iter().any(pattern_binds) {
                    return Err(CodegenError::internal(
                        "a binding or-pattern reached the unexpanded matcher",
                    ));
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
                // An enum is represented by a pointer, so `Enum?` uses null
                // for absence rather than an additional optional box.  A
                // constructor pattern is a presence test followed by the
                // normal tag/payload test; crucially, the null path goes to
                // the next arm before we dereference the enum header.
                if let Type::Optional(inner) = subject_ty {
                    let present = self.builder.create_block();
                    let is_null = self.builder.ins().icmp_imm_s(IntCC::Equal, subject, 0);
                    self.builder.ins().brif(is_null, fail, &[], present, &[]);
                    self.terminated = true;
                    self.goto(present);
                    self.test_constructor(name, fields, subject, inner, fail, &pattern.span)
                } else {
                    self.test_constructor(name, fields, subject, subject_ty, fail, &pattern.span)
                }
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
        // An instantiated generic enum is named before its layout exists, so it
        // can arrive labelled as a struct; the layout settles which it is.
        let enum_name = match subject_ty {
            Type::Enum(name) => name.clone(),
            Type::Struct(name) if self.l.layouts.enums.contains_key(name) => name.clone(),
            other => {
                return Err(CodegenError::unsupported_at(
                    format!(
                        "`{}` is a variant pattern, but the subject is a `{}`",
                        name,
                        other.display_name()
                    ),
                    span,
                ))
            }
        };
        let enum_name = &enum_name;
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
        if matches!(subject_ty, Type::Any) {
            let expected = self.lower_expr_value(literal, &Type::Any)?;
            let opcode = self
                .builder
                .ins()
                .iconst(types::I64, dynamic_binary_opcode(BinaryOp::Eq));
            return self.call_rt_value("lira_rt_any_compare", &[opcode, subject, expected]);
        }
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
    type_args: &[lirac::ast::TypeExpr],
    args: &[Argument],
) -> Option<Type> {
    let explicit: Vec<Type> = type_args
        .iter()
        .map(|ty| l.normalize(layout::type_of_ann(ty)))
        .collect();
    // `super.method()` resolves against the parent of whatever class the
    // receiver belongs to.
    if matches!(&receiver.kind, ExpressionKind::Identifier(name) if name == "super") {
        let receiver_ty = resolve("self").or_else(|| resolve("this"))?;
        let (Type::Struct(class) | Type::Class(class)) = receiver_ty else {
            return None;
        };
        let mut current = l.layouts.structs.get(&class)?.parent.clone();
        while let Some(name) = current {
            if let Some(info) = l.funcs.get(&fn_key(Some(&name), method)) {
                return Some(info.ret.clone());
            }
            current = l.layouts.structs.get(&name)?.parent.clone();
        }
        return None;
    }

    // `Counter.new()` — a call on a bare type name.
    if let ExpressionKind::Identifier(name) = &receiver.kind {
        if resolve(name).is_none()
            && (l.layouts.is_aggregate(name) || l.layouts.generics.contains_key(name))
        {
            if l.layouts.enums.contains_key(name) {
                return Some(l.user_type(name));
            }
            let key = fn_key(Some(name), method);
            if let Some(info) = l.funcs.get(&key) {
                return Some(info.ret.clone());
            }
            let arg_types: Vec<Type> = args
                .iter()
                .map(|arg| infer_or_checked_with(l, resolve, &arg.value))
                .collect::<Option<_>>()?;
            return l.generic_call_type(&key, &arg_types, &explicit, None, None);
        }
    }
    let receiver_ty = infer_or_checked_with(l, resolve, receiver)?;
    if let Type::Interface(name) = &receiver_ty {
        return l
            .layouts
            .interface(name)
            .and_then(|interface| interface.method(method))
            .map(|method| method_return(&method.signature));
    }
    // A user `impl` block wins wherever one exists — including `impl int` and
    // `impl string`, which is how the standard library defines most of its
    // methods on primitive types.
    if let Some(ret) = impl_method_return(l, &receiver_ty, method) {
        return Some(ret);
    }
    if let Type::Struct(name) | Type::Enum(name) | Type::Class(name) = &receiver_ty {
        if let Some(template_name) = l.generic_template_name(name) {
            let key = fn_key(Some(template_name), method);
            if l.generic_index.contains_key(&key) {
                let arg_types: Vec<Type> = args
                    .iter()
                    .map(|arg| infer_or_checked_with(l, resolve, &arg.value))
                    .collect::<Option<_>>()?;
                let owner_args =
                    l.generic_owner_args_for_expr(template_name, &receiver_ty, receiver, resolve);
                if let Some(ret) = l.generic_call_type(
                    &key,
                    &arg_types,
                    &explicit,
                    Some(&receiver_ty),
                    owner_args.as_deref(),
                ) {
                    return Some(ret);
                }
            }
        }
    }
    // An inherited method lives on an ancestor rather than on the class itself.
    if let Type::Struct(class) | Type::Class(class) = &receiver_ty {
        let mut current = Some(class.clone());
        while let Some(name) = current {
            if let Some(info) = l.funcs.get(&fn_key(Some(&name), method)) {
                return Some(info.ret.clone());
            }
            current = l.layouts.structs.get(&name)?.parent.clone();
        }
    }
    Some(match receiver_ty {
        Type::Array(inner) => match method {
            "len" => Type::Int,
            "pop" => Type::Optional(inner),
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

fn method_return(signature: &Type) -> Type {
    match signature {
        Type::Function { return_type, .. } => (**return_type).clone(),
        _ => Type::Any,
    }
}

fn interface_payload_kind(ty: &Type) -> CodegenResult<u32> {
    Ok(match repr_of(ty)? {
        Repr::Ref => 0,
        Repr::Int => 1,
        Repr::Float => 2,
        Repr::Bool => 3,
        Repr::Void => {
            return Err(CodegenError::unsupported(
                "void cannot be stored in an interface payload",
            ))
        }
    })
}

/// Integer-family values share one untyped `Any` tag in the native ABI.
fn is_erased_integer_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Int64
            | Type::UInt8
            | Type::UInt16
            | Type::UInt32
            | Type::UInt64
            | Type::Char
    )
}

/// A structural key used only for generated symbol/cache names.  It is built
/// from the type tree directly, so it cannot collide through display-string
/// parsing or source aliases.
fn interface_type_key(ty: &Type) -> String {
    interface_signature(ty)
}

/// Canonical run-time signature for one interface method.
///
/// The synthetic receiver is deliberately excluded: `Wide.first` and
/// `Narrow.first` have the same callable contract even though their receiver
/// slots name different interfaces.  Preserve the per-position default mask,
/// because the checker permits defaults outside a simple trailing suffix.
fn interface_method_signature(method: &layout::InterfaceMethodLayout) -> String {
    let explicit_params = method.params.iter().skip(1);
    let param_types = explicit_params
        .clone()
        .map(|param| interface_signature(&param.ty))
        .collect::<Vec<_>>()
        .join(",");
    let defaults = explicit_params
        .map(|param| if param.default.is_some() { '1' } else { '0' })
        .collect::<String>();
    format!(
        "method/{defaults}<{param_types}>->{}",
        interface_signature(&method_return(&method.signature))
    )
}

/// Canonical interface metadata signature.  Required parameter count is part
/// of every function node, including nested function types.
fn interface_signature(ty: &Type) -> String {
    match ty {
        Type::Int => "int".to_owned(),
        Type::Float => "float".to_owned(),
        Type::Bool => "bool".to_owned(),
        Type::String => "string".to_owned(),
        Type::Char => "char".to_owned(),
        Type::Void => "void".to_owned(),
        Type::Null => "null".to_owned(),
        Type::Int8 => "int8".to_owned(),
        Type::Int16 => "int16".to_owned(),
        Type::Int32 => "int32".to_owned(),
        Type::Int64 => "int64".to_owned(),
        Type::UInt8 => "uint8".to_owned(),
        Type::UInt16 => "uint16".to_owned(),
        Type::UInt32 => "uint32".to_owned(),
        Type::UInt64 => "uint64".to_owned(),
        Type::Array(inner) => format!("array<{}>", interface_signature(inner)),
        Type::Channel(inner) => format!("channel<{}>", interface_signature(inner)),
        Type::Tuple(items) => format!(
            "tuple<{}>",
            items
                .iter()
                .map(interface_signature)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Map(key, value) => format!(
            "map<{},{}>",
            interface_signature(key),
            interface_signature(value)
        ),
        Type::Optional(inner) => format!("optional<{}>", interface_signature(inner)),
        Type::Result { ok_type, err_type } => format!(
            "result<{},{}>",
            interface_signature(ok_type),
            interface_signature(err_type)
        ),
        Type::Function {
            params,
            return_type,
            required_params,
        } => format!(
            "fn/{required_params}<{}>->{}",
            params
                .iter()
                .map(interface_signature)
                .collect::<Vec<_>>()
                .join(","),
            interface_signature(return_type)
        ),
        Type::Class(name) => format!("class:{name}"),
        Type::Struct(name) => format!("struct:{name}"),
        Type::Enum(name) => format!("enum:{name}"),
        Type::Interface(name) => format!("interface:{name}"),
        Type::TypeVar(id) => format!("typevar:{id}"),
        Type::Unknown => "unknown".to_owned(),
        Type::Any => "any".to_owned(),
        Type::TypeParam(name) => format!("typeparam:{name}"),
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

fn collect_pattern_binding_specs(
    pattern: &Pattern,
    sema: &SemanticTables,
    bindings: &mut Vec<(String, Type)>,
) -> CodegenResult<()> {
    let binding_type = || {
        sema.pattern_types.get(&pattern.id).cloned().ok_or_else(|| {
            CodegenError::unsupported_at(
                "the checker did not record this pattern binding's type",
                &pattern.span,
            )
        })
    };
    match &pattern.kind {
        PatternKind::Variable(name) => bindings.push((name.clone(), binding_type()?)),
        PatternKind::Binding {
            name,
            pattern: inner,
        } => {
            bindings.push((name.clone(), binding_type()?));
            collect_pattern_binding_specs(inner, sema, bindings)?;
        }
        PatternKind::Tuple(items) | PatternKind::Or(items) => {
            for item in items {
                collect_pattern_binding_specs(item, sema, bindings)?;
            }
        }
        PatternKind::Constructor { fields, .. } => {
            for field in fields {
                collect_pattern_binding_specs(field, sema, bindings)?;
            }
        }
        PatternKind::Struct { fields, .. } => {
            for (_, field) in fields {
                collect_pattern_binding_specs(field, sema, bindings)?;
            }
        }
        PatternKind::Wildcard | PatternKind::Literal(_) | PatternKind::Range { .. } => {}
    }
    Ok(())
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
        let (target, values, env_types) = match &call.kind {
            ExpressionKind::Call {
                callee,
                args,
                type_args,
            } if !self.is_spawn_builtin_call(callee) => {
                self.lower_spawn_call(callee, args, type_args, span)?
            }
            ExpressionKind::Call { .. } => self.lower_spawn_body(call, span)?,
            ExpressionKind::MethodCall {
                receiver,
                method,
                args,
                type_args,
            } => self.lower_spawn_method(receiver, method, args, type_args, span)?,
            _ => self.lower_spawn_body(call, span)?,
        };

        let env_size = HEADER_SIZE + SLOT_SIZE * values.len() as i32;
        let env = self.alloc_object(env_size, runtime::KIND_STRUCT)?;
        for (index, (value, ty)) in values.iter().zip(env_types.iter()).enumerate() {
            let slot = self.value_to_slot(*value, ty)?;
            let offset = HEADER_SIZE + SLOT_SIZE * index as i32;
            self.builder
                .ins()
                .store(MemFlagsData::trusted(), slot, env, offset);
        }

        let symbol = format!("lira__spawn__{}", self.l.next_spawn);
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
            target,
            env_types,
        });

        let thunk = self.func_ref_by_id(func_id);
        let ptr = self.pointer_ty();
        let thunk_addr = self.builder.ins().func_addr(ptr, thunk);
        let fiber_id = self.call_rt_value("lira_rt_spawn", &[thunk_addr, env])?;
        self.call_rt_value("lira_rt_any_box_fiber", &[fiber_id])
    }

    /// Builtin calls and non-call expressions run in a zero-argument child
    /// closure. Call operands are lowered into parent locals first, matching
    /// direct spawn's eager, exactly-once argument evaluation while allowing
    /// the builtin itself to execute on the child fiber.
    fn lower_spawn_body(
        &mut self,
        expression: &Expression,
        span: &Span,
    ) -> CodegenResult<(SpawnTarget, Vec<Value>, Vec<Type>)> {
        let mut body = expression.clone();
        if let ExpressionKind::Call { callee, args, .. } = &expression.kind {
            if matches!(&callee.kind, ExpressionKind::Identifier(_)) {
                let mut staged = Vec::with_capacity(args.len());
                for argument in args {
                    let ty = self.ty_of(&argument.value)?;
                    let value = self.lower_expr_value(&argument.value, &ty)?;
                    let temp = self.fresh_spawn_temp_name();
                    self.declare_local(&temp, ty, Some(value))?;
                    staged.push(Argument {
                        name: None,
                        value: Expression {
                            id: NodeId::new(0),
                            kind: ExpressionKind::Identifier(temp),
                            span: argument.value.span.clone(),
                        },
                        span: argument.span.clone(),
                    });
                }
                body = Expression {
                    id: NodeId::new(0),
                    kind: ExpressionKind::Call {
                        callee: Box::new((**callee).clone()),
                        type_args: Vec::new(),
                        args: staged,
                    },
                    span: expression.span.clone(),
                };
            }
        }

        let closure_ty = Type::Function {
            params: Vec::new(),
            return_type: Box::new(Type::Void),
            required_params: 0,
        };
        let closure = self.lower_lambda(&[], &body, Some(&closure_ty), span)?;
        Ok((
            SpawnTarget::Indirect {
                params: Vec::new(),
                ret: Type::Void,
            },
            vec![closure],
            vec![closure_ty],
        ))
    }

    fn fresh_spawn_temp_name(&mut self) -> String {
        loop {
            let candidate = self.l.next_spawn_temp;
            self.l.next_spawn_temp += 1;
            let name = format!("__lira_native_spawn_{candidate}");
            if self.lookup(&name).is_none() {
                return name;
            }
        }
    }

    fn is_spawn_builtin_call(&self, callee: &Expression) -> bool {
        let ExpressionKind::Identifier(name) = &callee.kind else {
            return false;
        };
        if self.l.funcs.contains_key(name) {
            return false;
        }
        matches!(
            name.as_str(),
            "print"
                | "println"
                | "assert"
                | "chan"
                | "send"
                | "recv"
                | "close"
                | "fiber_yield"
                | "fiber_id"
                | "len"
                | "push"
                | "pop"
                | "collect"
                | "sqrt"
                | "abs"
                | "floor"
                | "ceil"
                | "trunc"
                | "is_nan"
                | "is_infinite"
                | "is_finite"
        ) || runtime::builtin(name).is_some()
    }

    /// Prepare a spawn whose callee is represented by a normal call AST node.
    fn lower_spawn_call(
        &mut self,
        callee: &Expression,
        args: &[Argument],
        type_args: &[lirac::ast::TypeExpr],
        span: &Span,
    ) -> CodegenResult<(SpawnTarget, Vec<Value>, Vec<Type>)> {
        match &callee.kind {
            ExpressionKind::Identifier(name) => {
                if let Some(binding) = self.lookup(name) {
                    let fn_ty = match &binding {
                        Binding::Local { ty, .. } => ty.clone(),
                        Binding::Global(global) => global.ty.clone(),
                    };
                    if let Type::Function {
                        ref params,
                        ref return_type,
                        ..
                    } = fn_ty
                    {
                        let params: Vec<Type> = params
                            .iter()
                            .map(|ty| self.l.normalize(ty.clone()))
                            .collect();
                        if args.len() != params.len() {
                            return Err(CodegenError::unsupported_at(
                                format!("this function takes {} argument(s)", params.len()),
                                span,
                            ));
                        }
                        let closure = self.lower_expr_value(callee, &fn_ty)?;
                        let mut values = vec![closure];
                        for (arg, ty) in args.iter().zip(params.iter()) {
                            values.push(self.lower_expr_value(&arg.value, ty)?);
                        }
                        let mut env_types = vec![fn_ty.clone()];
                        env_types.extend(params.iter().cloned());
                        return Ok((
                            SpawnTarget::Indirect {
                                params,
                                ret: self.l.normalize((**return_type).clone()),
                            },
                            values,
                            env_types,
                        ));
                    }
                }
                if self.l.generic_index.contains_key(name) {
                    let explicit: Vec<Type> = type_args
                        .iter()
                        .map(|ty| self.l.resolve_ann(ty, &HashSet::new()))
                        .collect::<CodegenResult<_>>()?;
                    let call_arg_types: Vec<Type> = args
                        .iter()
                        .map(|arg| self.ty_of(&arg.value))
                        .collect::<CodegenResult<_>>()?;
                    let inferred = self
                        .l
                        .infer_type_args(name, &call_arg_types, &explicit, None, None)
                        .ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("cannot work out the type arguments for `{}`", name),
                                span,
                            )
                        })?;
                    let key = self.l.instantiate_fn(name, &inferred, span)?;
                    let info =
                        self.l.funcs.get(&key).ok_or_else(|| {
                            CodegenError::internal(format!("`{}` disappeared", key))
                        })?;
                    let arg_types: Vec<Type> = info.params.iter().map(|p| p.ty.clone()).collect();
                    let (values, _) = self.build_user_call_args(&key, None, args, span)?;
                    return Ok((SpawnTarget::Direct { key }, values, arg_types));
                }
                if self.l.funcs.contains_key(name) {
                    let info =
                        self.l.funcs.get(name).ok_or_else(|| {
                            CodegenError::internal(format!("`{}` disappeared", name))
                        })?;
                    let arg_types: Vec<Type> = info.params.iter().map(|p| p.ty.clone()).collect();
                    let (values, _) = self.build_user_call_args(name, None, args, span)?;
                    return Ok((SpawnTarget::Direct { key: name.clone() }, values, arg_types));
                }
                if let Some(first) = args.first() {
                    let receiver_ty = self.ty_of(&first.value)?;
                    if let Some(key) = self.impl_key_for(&receiver_ty, name) {
                        let receiver =
                            self.method_receiver_value(&first.value, &receiver_ty, &key)?;
                        let info = self.l.funcs.get(&key).ok_or_else(|| {
                            CodegenError::internal(format!("`{}` disappeared", key))
                        })?;
                        let arg_types: Vec<Type> =
                            info.params.iter().map(|p| p.ty.clone()).collect();
                        let (values, _) =
                            self.build_user_call_args(&key, Some(receiver), &args[1..], span)?;
                        return Ok((SpawnTarget::Direct { key }, values, arg_types));
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
                        "only `Type::member` paths are callable in native spawn",
                        span,
                    ));
                };
                let key = fn_key(Some(type_name), member);
                if self.l.generic_index.contains_key(&key) {
                    let explicit: Vec<Type> = type_args
                        .iter()
                        .map(|ty| self.l.resolve_ann(ty, &HashSet::new()))
                        .collect::<CodegenResult<_>>()?;
                    let call_arg_types: Vec<Type> = args
                        .iter()
                        .map(|arg| self.ty_of(&arg.value))
                        .collect::<CodegenResult<_>>()?;
                    let inferred = self
                        .l
                        .infer_type_args(&key, &call_arg_types, &explicit, None, None)
                        .ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("cannot work out the type arguments for `{}`", key),
                                span,
                            )
                        })?;
                    let instance = self.l.instantiate_fn(&key, &inferred, span)?;
                    let info = self.l.funcs.get(&instance).ok_or_else(|| {
                        CodegenError::internal(format!("`{}` disappeared", instance))
                    })?;
                    let arg_types: Vec<Type> = info.params.iter().map(|p| p.ty.clone()).collect();
                    let (values, _) = self.build_user_call_args(&instance, None, args, span)?;
                    return Ok((SpawnTarget::Direct { key: instance }, values, arg_types));
                }
                let info = self.l.funcs.get(&key).ok_or_else(|| {
                    CodegenError::unsupported_at(format!("unknown method `{}`", key), span)
                })?;
                let arg_types: Vec<Type> = info.params.iter().map(|p| p.ty.clone()).collect();
                let (values, _) = self.build_user_call_args(&key, None, args, span)?;
                Ok((SpawnTarget::Direct { key }, values, arg_types))
            }
            ExpressionKind::FieldAccess { object, field } => {
                if let ExpressionKind::Identifier(type_name) = &object.kind {
                    if self.lookup(type_name).is_none()
                        && (self.l.layouts.is_aggregate(type_name)
                            || self.l.layouts.generics.contains_key(type_name))
                    {
                        let key = fn_key(Some(type_name), field);
                        if self.l.generic_index.contains_key(&key) {
                            let explicit: Vec<Type> = type_args
                                .iter()
                                .map(|ty| self.l.resolve_ann(ty, &HashSet::new()))
                                .collect::<CodegenResult<_>>()?;
                            let call_arg_types: Vec<Type> = args
                                .iter()
                                .map(|arg| self.ty_of(&arg.value))
                                .collect::<CodegenResult<_>>()?;
                            let inferred = self
                                .l
                                .infer_type_args(&key, &call_arg_types, &explicit, None, None)
                                .ok_or_else(|| {
                                    CodegenError::unsupported_at(
                                        format!("cannot work out the type arguments for `{}`", key),
                                        span,
                                    )
                                })?;
                            let instance = self.l.instantiate_fn(&key, &inferred, span)?;
                            let info = self.l.funcs.get(&instance).ok_or_else(|| {
                                CodegenError::internal(format!("`{}` disappeared", instance))
                            })?;
                            let arg_types: Vec<Type> =
                                info.params.iter().map(|p| p.ty.clone()).collect();
                            let (values, _) =
                                self.build_user_call_args(&instance, None, args, span)?;
                            return Ok((SpawnTarget::Direct { key: instance }, values, arg_types));
                        }
                        let info = self.l.funcs.get(&key).ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("unknown static method `{}`", field),
                                span,
                            )
                        })?;
                        let arg_types: Vec<Type> =
                            info.params.iter().map(|p| p.ty.clone()).collect();
                        let (values, _) = self.build_user_call_args(&key, None, args, span)?;
                        return Ok((SpawnTarget::Direct { key }, values, arg_types));
                    }
                }
                self.lower_spawn_method(object, field, args, type_args, span)
            }
            _ => {
                let fn_ty = self.ty_of(callee)?;
                let Type::Function {
                    params,
                    return_type,
                    ..
                } = fn_ty.clone()
                else {
                    return Err(CodegenError::unsupported_at(
                        format!("`{}` is not callable", fn_ty.display_name()),
                        span,
                    ));
                };
                let params: Vec<Type> = params
                    .iter()
                    .map(|ty| self.l.normalize(ty.clone()))
                    .collect();
                if args.len() != params.len() {
                    return Err(CodegenError::unsupported_at(
                        format!("this function takes {} argument(s)", params.len()),
                        span,
                    ));
                }
                let closure = self.lower_expr_value(callee, &fn_ty)?;
                let mut values = vec![closure];
                for (arg, ty) in args.iter().zip(params.iter()) {
                    values.push(self.lower_expr_value(&arg.value, ty)?);
                }
                let mut env_types = vec![fn_ty];
                env_types.extend(params.iter().cloned());
                Ok((
                    SpawnTarget::Indirect {
                        params: params.clone(),
                        ret: self.l.normalize(*return_type),
                    },
                    values,
                    env_types,
                ))
            }
        }
    }

    /// Prepare a spawn of an instance method, including class vtable dispatch.
    fn lower_spawn_method(
        &mut self,
        receiver: &Expression,
        method: &str,
        args: &[Argument],
        type_args: &[lirac::ast::TypeExpr],
        span: &Span,
    ) -> CodegenResult<(SpawnTarget, Vec<Value>, Vec<Type>)> {
        let receiver_ty = self.ty_of(receiver)?;
        if let Type::Interface(interface_name) = &receiver_ty {
            if !type_args.is_empty() {
                return Err(CodegenError::unsupported_at(
                    "generic methods on interfaces are not lowered yet",
                    span,
                ));
            }
            let method_layout = self
                .l
                .layouts
                .interface_method(interface_name, method)
                .cloned()
                .ok_or_else(|| {
                    CodegenError::unsupported_at(
                        format!("interface `{interface_name}` has no method `{method}`"),
                        span,
                    )
                })?;
            let receiver_value = self.lower_expr_value(receiver, &receiver_ty)?;
            let explicit = &method_layout.params[1..];
            let mut slots: Vec<Option<Value>> = vec![None; explicit.len()];
            let mut positional = 0usize;
            for arg in args {
                let index = match &arg.name {
                    Some(name) => explicit
                        .iter()
                        .position(|param| param.name == *name)
                        .ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!("`{method}` has no parameter named `{name}`"),
                                &arg.span,
                            )
                        })?,
                    None => {
                        let index = positional;
                        positional += 1;
                        if index >= explicit.len() {
                            return Err(CodegenError::unsupported_at(
                                format!("too many arguments for `{method}`"),
                                &arg.span,
                            ));
                        }
                        index
                    }
                };
                if slots[index].is_some() {
                    return Err(CodegenError::unsupported_at(
                        format!(
                            "argument `{}` was provided more than once",
                            explicit[index].name
                        ),
                        &arg.span,
                    ));
                }
                slots[index] =
                    Some(self.lower_call_argument_value(&arg.value, &explicit[index].ty)?);
            }
            let mut values = Vec::with_capacity(method_layout.params.len());
            values.push(receiver_value);
            for (index, param) in explicit.iter().enumerate() {
                values.push(match slots[index] {
                    Some(value) => value,
                    None => match &param.default {
                        Some(default) => self.lower_expr_value(default, &param.ty)?,
                        None => {
                            return Err(CodegenError::unsupported_at(
                                format!("missing argument `{}` for `{method}`", param.name),
                                span,
                            ));
                        }
                    },
                });
            }
            let params: Vec<Type> = method_layout
                .params
                .iter()
                .map(|param| param.ty.clone())
                .collect();
            let ret = method_return(&method_layout.signature);
            let target = SpawnTarget::Interface {
                interface: interface_name.clone(),
                method: method.to_owned(),
                slot: method_layout.slot,
                params: params.clone(),
                ret,
            };
            return Ok((target, values, params));
        }
        let (class, key, virtual_call) = match &receiver_ty {
            Type::Struct(name) | Type::Class(name) | Type::Enum(name) => {
                let key = if let Some(key) = self.resolve_method(name, method) {
                    key
                } else if let Some(template_name) = self.l.generic_template_name(name) {
                    let template_key = fn_key(Some(template_name), method);
                    let explicit: Vec<Type> = type_args
                        .iter()
                        .map(|ty| self.l.resolve_ann(ty, &HashSet::new()))
                        .collect::<CodegenResult<_>>()?;
                    let call_arg_types: Vec<Type> = args
                        .iter()
                        .map(|arg| self.ty_of(&arg.value))
                        .collect::<CodegenResult<_>>()?;
                    let inferred = self
                        .l
                        .infer_type_args(
                            &template_key,
                            &call_arg_types,
                            &explicit,
                            Some(&receiver_ty),
                            None,
                        )
                        .ok_or_else(|| {
                            CodegenError::unsupported_at(
                                format!(
                                    "cannot work out the type arguments for `{}`",
                                    template_key
                                ),
                                span,
                            )
                        })?;
                    self.l.instantiate_fn(&template_key, &inferred, span)?
                } else {
                    return Err(CodegenError::unsupported_at(
                        format!("`{}` has no method `{}`", name, method),
                        span,
                    ));
                };
                let virtual_call = self
                    .l
                    .layouts
                    .structs
                    .get(name)
                    .is_some_and(|layout| layout.is_class);
                (Some(name.clone()), key, virtual_call)
            }
            other => {
                let key = self.impl_key_for(other, method).ok_or_else(|| {
                    CodegenError::unsupported_at(
                        format!("`{}` has no method `{}`", other.display_name(), method),
                        span,
                    )
                })?;
                (None, key, false)
            }
        };
        let receiver_value = self.method_receiver_value(receiver, &receiver_ty, &key)?;
        let info = self
            .l
            .funcs
            .get(&key)
            .ok_or_else(|| CodegenError::internal(format!("`{}` disappeared", key)))?;
        let arg_types: Vec<Type> = info.params.iter().map(|p| p.ty.clone()).collect();
        let (values, _) = self.build_user_call_args(&key, Some(receiver_value), args, span)?;
        let target = if virtual_call {
            let class = class.ok_or_else(|| {
                CodegenError::internal("virtual method was resolved without a class")
            })?;
            SpawnTarget::Virtual {
                class,
                method: method.to_string(),
                key,
            }
        } else {
            SpawnTarget::Direct { key }
        };
        Ok((target, values, arg_types))
    }
}

// ====================================================================== //
// Fallback type inference                                                 //
// ====================================================================== //

fn inferred_field_type(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    object: &Expression,
    owner: Type,
    field: &str,
) -> Option<Type> {
    match owner {
        Type::Struct(name) | Type::Class(name) => {
            if let Some(layout) = l.layouts.structs.get(&name) {
                Some(l.normalize(layout.field(field)?.ty.clone()))
            } else {
                generic_field_type(l, resolve, object, &name, field)
            }
        }
        Type::Enum(_) if field == "__enum" || field == "__variant" => Some(Type::String),
        _ => None,
    }
}

/// Work out an expression's type without the checker's tables.
///
/// Type checking covers method bodies, but generic/template expressions can
/// still be intentionally erased or lack a concrete instantiation in the
/// semantic tables. Bytecode can tolerate that dynamic boundary; native code
/// needs a concrete representation. Everything needed for this fallback is
/// already in hand: the names the caller can resolve, the aggregate layouts,
/// and every declared signature.
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

        ExpressionKind::FieldAccess { object, field } => inferred_field_type(
            l,
            resolve,
            object,
            infer_or_checked_with(l, resolve, object)?,
            field,
        )?,

        ExpressionKind::Index { object, index } => {
            match infer_or_checked_with(l, resolve, object)? {
                Type::Array(inner) => *inner,
                Type::Map(_, value) => *value,
                Type::Tuple(items) => {
                    let ExpressionKind::IntLiteral(position) = index.kind else {
                        return None;
                    };
                    items.get(position as usize)?.clone()
                }
                Type::String => Type::String,
                _ => return None,
            }
        }

        ExpressionKind::Array(elements) => {
            let first = elements.first()?;
            Type::Array(Box::new(infer_or_checked_with(l, resolve, first)?))
        }

        ExpressionKind::StructLiteral {
            name: Some(name),
            fields,
        } => generic_literal_type(l, resolve, name, fields).unwrap_or_else(|| l.user_type(name)),

        ExpressionKind::EnumVariant {
            enum_name,
            variant_name,
        } => generic_variant_type(l, resolve, enum_name, variant_name, &[])
            .unwrap_or_else(|| l.user_type(enum_name)),

        ExpressionKind::Path { segments } => {
            let [type_name, _] = segments.as_slice() else {
                return None;
            };
            if !l.layouts.enums.contains_key(type_name) {
                return None;
            }
            l.user_type(type_name)
        }

        ExpressionKind::Call {
            callee,
            type_args,
            args,
        } => match &callee.kind {
            // A user function of the same name wins over a built-in, so look for
            // one before matching the built-in names.
            ExpressionKind::Identifier(name) if l.funcs.contains_key(name.as_str()) => {
                l.funcs[name.as_str()].ret.clone()
            }
            // A call through a local or parameter holding a function value
            // yields that function's return type.
            ExpressionKind::Identifier(name)
                if matches!(resolve(name), Some(Type::Function { .. })) =>
            {
                let Some(Type::Function { return_type, .. }) = resolve(name) else {
                    return None;
                };
                l.normalize(*return_type)
            }
            // A generic call's result depends on the arguments it was given.
            ExpressionKind::Identifier(name) if l.generic_index.contains_key(name.as_str()) => {
                let arg_types: Vec<Type> = args
                    .iter()
                    .map(|arg| infer_or_checked_with(l, resolve, &arg.value))
                    .collect::<Option<_>>()?;
                let explicit: Vec<Type> = type_args
                    .iter()
                    .map(|ty| l.normalize(layout::type_of_ann(ty)))
                    .collect();
                l.generic_call_type(name, &arg_types, &explicit, None, None)?
            }
            ExpressionKind::Identifier(name) => match name.as_str() {
                "print" | "println" | "assert" | "send" | "close" | "fiber_yield" | "collect" => {
                    Type::Void
                }
                "len" | "fiber_id" => Type::Int,
                "recv" => match infer_or_checked_with(l, resolve, &args.first()?.value)? {
                    Type::Channel(inner)
                        if matches!(inner.as_ref(), Type::Unknown | Type::TypeVar(_)) =>
                    {
                        Type::Any
                    }
                    Type::Channel(inner) => *inner,
                    Type::Any => Type::Any,
                    _ => return None,
                },
                "push" => Type::Void,
                // Typed arrays preserve the checker/runtime `T?` contract;
                // genuinely dynamic arrays remain `Any`.
                "pop" => match infer_or_checked_with(l, resolve, &args.first()?.value)? {
                    Type::Array(inner) => Type::Optional(inner),
                    _ => return None,
                },
                "chan" => Type::Channel(Box::new(Type::Unknown)),
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
            ExpressionKind::EnumVariant {
                enum_name,
                variant_name,
            } => generic_variant_type(l, resolve, enum_name, variant_name, args)
                .unwrap_or_else(|| l.user_type(enum_name)),
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
                method_call_type(l, resolve, object, field, type_args, args)?
            }
            _ => return None,
        },

        ExpressionKind::MethodCall {
            receiver,
            method,
            type_args,
            args,
        } => method_call_type(l, resolve, receiver, method, type_args, args)?,

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

        // A spawn yields an opaque fiber handle. The native backend boxes the
        // scheduler id so printing and dynamic operations match the VM's
        // `Value::Fiber` representation without exposing the id.
        ExpressionKind::Spawn(_) => Type::Any,
        // A block's value is its trailing expression, or what it returns — the
        // second shape is how a lambda with a body block gives back a value.
        ExpressionKind::Block(block) => match block.statements.last().map(|s| &s.kind) {
            Some(StatementKind::Expression(expr)) | Some(StatementKind::Return(Some(expr))) => {
                infer_or_checked_with(l, resolve, expr)?
            }
            _ => Type::Void,
        },

        ExpressionKind::Select(arms) => arms
            .iter()
            .find_map(|arm| infer_or_checked_with(l, resolve, &arm.body))
            .filter(|ty| !matches!(ty, Type::Void))
            .unwrap_or(Type::Void),

        _ => return None,
    })
}

/// Whatever the checker recorded, falling back to [`infer_with`].
///
/// A name the caller resolved always wins: the checker erases enum payloads and
/// pattern bindings to `any`, and native code needs the real type. A recorded
/// `Any` is otherwise authoritative because it describes the boxed ABI; only
/// the few placeholder nodes listed by [`needs_structural_any`] are refined.
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
            // The checker records call expressions as `Any` while walking a
            // function body, even when the resolved declaration has an
            // explicit concrete return type.  The native ABI must use that
            // declaration rather than passing the raw return register to an
            // Any consumer (for example `println(area(point))`).  An
            // unannotated function still has an `Any` return in `l.funcs`, so
            // this refinement does not make dynamic calls look typed.
            let inferred_structural = match &expr.kind {
                ExpressionKind::Call { .. }
                | ExpressionKind::MethodCall { .. }
                | ExpressionKind::Binary { .. }
                | ExpressionKind::Unary { .. } => infer_with(l, resolve, expr),
                // The checker records built-in aggregate fields such as
                // `Range.inclusive` as `any`, while the native layout carries
                // their concrete representation. Refine only when the receiver
                // itself is statically known; a field read through a genuinely
                // dynamic receiver must stay boxed Any.
                ExpressionKind::FieldAccess { object, field } => {
                    let owner = infer_or_checked_with(l, resolve, object)?;
                    if matches!(owner, Type::Any) {
                        None
                    } else {
                        inferred_field_type(l, resolve, object, owner, field)
                    }
                }
                _ => None,
            };
            // The checker records a generic return such as `Pair<T>` using the
            // template spelling (`Pair$T`) even after the call arguments make
            // `T` concrete.  Prefer the native structural inference when both
            // spellings refer to the same aggregate template; otherwise a
            // field chain like `make(7).inner.value` would retain `T` all the
            // way to the load and be lowered as an unresolved type.
            let same_generic_template = match (&recorded, inferred_structural.as_ref()) {
                (Type::Struct(recorded), Some(Type::Struct(inferred)))
                | (Type::Enum(recorded), Some(Type::Enum(inferred)))
                | (Type::Class(recorded), Some(Type::Class(inferred))) => l
                    .generic_template_name(recorded)
                    .zip(l.generic_template_name(inferred))
                    .is_some_and(|(recorded, inferred)| recorded == inferred),
                _ => false,
            };
            if matches!(
                &expr.kind,
                ExpressionKind::Call { .. } | ExpressionKind::MethodCall { .. }
            ) && same_generic_template
            {
                return inferred_structural;
            }
            if matches!(recorded, Type::Any)
                && inferred_structural.as_ref().is_some_and(|inferred| {
                    !matches!(inferred, Type::Any | Type::Unknown | Type::TypeParam(_))
                })
            {
                return inferred_structural;
            }
            if matches!(recorded, Type::Any) && !needs_structural_any(expr) {
                // `Any` is an ABI-bearing semantic result, not a hint to
                // re-run inference. In particular, calls, dynamic indexes,
                // and conditional/match values must stay boxed even when a
                // first branch or argument happens to be an integer.
                Some(recorded)
            } else if is_uninformative(l, &recorded) {
                infer_with(l, resolve, expr).or(Some(recorded))
            } else {
                Some(recorded)
            }
        }
        _ => infer_with(l, resolve, expr),
    }
}

/// Whether an expression crosses the boxed `Any` ABI even when structural
/// inference can guess a concrete scalar from one operand or branch.
///
/// This shared form is used both while declaring globals and while lowering a
/// function body. Keeping the resolver explicit prevents global declarations
/// from depending on function-local binding state.
fn dynamic_any_expression_with(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    expr: &Expression,
) -> bool {
    let inferred_is_any =
        |value: &Expression| matches!(infer_or_checked_with(l, resolve, value), Some(Type::Any));
    match &expr.kind {
        ExpressionKind::Identifier(_) => inferred_is_any(expr),
        ExpressionKind::Call { .. } | ExpressionKind::MethodCall { .. } => inferred_is_any(expr),
        ExpressionKind::Index { object, .. } | ExpressionKind::FieldAccess { object, .. } => {
            inferred_is_any(object) || dynamic_any_expression_with(l, resolve, object)
        }
        ExpressionKind::Binary { left, op, right } => {
            if is_comparison(*op)
                || matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::NullCoalesce)
            {
                return false;
            }
            if *op == BinaryOp::Add
                && (matches!(infer_or_checked_with(l, resolve, left), Some(Type::String))
                    || matches!(infer_or_checked_with(l, resolve, right), Some(Type::String)))
            {
                return false;
            }
            inferred_is_any(left)
                || inferred_is_any(right)
                || dynamic_any_expression_with(l, resolve, left)
                || dynamic_any_expression_with(l, resolve, right)
        }
        ExpressionKind::Unary { op, operand } => {
            !matches!(op, UnaryOp::Not) && dynamic_any_expression_with(l, resolve, operand)
        }
        ExpressionKind::IfExpr {
            then_expr,
            else_expr,
            ..
        } => {
            dynamic_any_expression_with(l, resolve, then_expr)
                || dynamic_any_expression_with(l, resolve, else_expr)
        }
        ExpressionKind::Block(block) => block
            .statements
            .last()
            .and_then(|statement| match &statement.kind {
                StatementKind::Expression(value) | StatementKind::Return(Some(value)) => {
                    Some(value)
                }
                _ => None,
            })
            .is_some_and(|value| dynamic_any_expression_with(l, resolve, value)),
        _ => false,
    }
}

/// A few checker nodes intentionally use `Any` as a placeholder for a
/// concrete native representation. Their structural forms are unambiguous:
/// ranges are native `Range` objects. Every other recorded `Any` is authoritative
/// and must remain a tagged dynamic value.
fn needs_structural_any(expr: &Expression) -> bool {
    matches!(&expr.kind, ExpressionKind::Range { .. })
}

/// Whether a recorded type says nothing native code can act on.
///
/// `any` is the obvious one. `any?` is the same story one level in: the checker
/// records `Optional(Any)` for every `?.`, so taking it at face value would lose
/// the field's real type.
fn is_uninformative(l: &Lowerer<'_>, ty: &Type) -> bool {
    match ty {
        Type::Any => true,
        // A bare `Box` names a template, not a type: it has no size until its
        // arguments are known, so the instantiated name has to come from
        // structural inference instead.
        Type::Struct(name) | Type::Enum(name) | Type::Class(name) => {
            l.layouts.generics.contains_key(name)
        }
        // The checker erases generics, so a call to `identity(42)` is recorded
        // as returning `T`. Native code needs the `int`, which only structural
        // inference over the call site can supply.
        Type::TypeParam(_) => true,
        Type::Optional(inner) => is_uninformative(l, inner),
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
    /// Byte offsets of the built-in `Range`'s `start`, `end` and `inclusive`
    /// fields.
    fn range_layout(&self, span: &Span) -> CodegenResult<(i32, i32, i32)> {
        if !self.l.layouts.range_layout_is_usable() {
            return Err(CodegenError::unsupported_at(
                "the native built-in `Range` layout is unavailable",
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
                    self.scopes.push(Vec::new());
                    match &arm.kind {
                        lirac::ast::SelectArmKind::Recv { channel, variable } => {
                            self.visit_expr(channel);
                            if let Some(variable) = variable {
                                self.bind(variable);
                            }
                        }
                        lirac::ast::SelectArmKind::Send { value, channel } => {
                            self.visit_expr(value);
                            self.visit_expr(channel);
                        }
                        lirac::ast::SelectArmKind::Default => {}
                    }
                    self.visit_expr(&arm.body);
                    self.scopes.pop();
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

    /// Emit one memoizing copy helper. The source layout is known to the
    /// compiler, while the runtime context only stores the source/destination
    /// mapping needed to close cycles. Reference-typed fields are copied as
    /// pointers; only non-class struct fields call another helper.
    fn lower_copy_helper(&mut self, name: &str) -> CodegenResult<()> {
        let layout = self
            .layouts
            .structs
            .get(name)
            .cloned()
            .ok_or_else(|| CodegenError::internal(format!("missing copy layout `{name}`")))?;
        if layout.is_class {
            return Err(CodegenError::internal(format!(
                "class layout `{name}` cannot have a value-copy helper"
            )));
        }
        let func_id = self.copy_helpers.get(name).copied().ok_or_else(|| {
            CodegenError::internal(format!("copy helper `{name}` was not declared"))
        })?;
        let mut sig = Signature::new(self.call_conv);
        sig.params.push(AbiParam::new(self.pointer_ty));
        sig.params.push(AbiParam::new(self.pointer_ty));
        sig.returns.push(AbiParam::new(self.pointer_ty));

        let frontend_config = self.module.target_config();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let source = builder.block_params(entry)[0];
            let copy_ctx = builder.block_params(entry)[1];
            let mut gen = FuncGen::new(self, builder, Type::Struct(name.to_string()));

            let null_block = gen.builder.create_block();
            let lookup_block = gen.builder.create_block();
            let found_block = gen.builder.create_block();
            let allocate_block = gen.builder.create_block();
            let merge = gen.builder.create_block();
            gen.builder.append_block_param(merge, gen.pointer_ty());

            let is_null = gen.builder.ins().icmp_imm_s(IntCC::Equal, source, 0);
            gen.builder
                .ins()
                .brif(is_null, null_block, &[], lookup_block, &[]);
            gen.terminated = true;

            gen.goto(null_block);
            let pointer_ty = gen.pointer_ty();
            let null_value = gen.builder.ins().iconst(pointer_ty, 0);
            gen.jump_to(merge, &[null_value]);

            gen.goto(lookup_block);
            let existing = gen.call_rt_value("lira_rt_copy_ctx_lookup", &[copy_ctx, source])?;
            let has_existing = gen.builder.ins().icmp_imm_s(IntCC::NotEqual, existing, 0);
            gen.builder
                .ins()
                .brif(has_existing, found_block, &[], allocate_block, &[]);
            gen.terminated = true;

            gen.goto(found_block);
            gen.jump_to(merge, &[existing]);

            gen.goto(allocate_block);
            let copy = gen.alloc_object(layout.size, runtime::KIND_STRUCT)?;
            // Publish before descending, so a self-edge or mutually recursive
            // edge resolves to this object rather than allocating forever.
            gen.call_rt("lira_rt_copy_ctx_insert", &[copy_ctx, source, copy])?;
            for field in &layout.fields {
                let field_ty = gen.l.normalize(field.ty.clone());
                let field_value = gen.load_at(source, field.offset, &field_ty)?;
                let field_value =
                    gen.copy_value_for_type_with_context(field_value, &field_ty, copy_ctx)?;
                gen.store_at(copy, field.offset, &field_ty, field_value)?;
            }
            gen.jump_to(merge, &[copy]);

            gen.goto(merge);
            let result = gen.builder.block_params(merge)[0];
            gen.builder.ins().return_(&[result]);
            gen.builder.seal_all_blocks();
            gen.builder.finalize(frontend_config);
        }

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| CodegenError::internal(format!("{name} copy helper: {e}")))?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    /// Emit one interface witness adapter.  The ABI is intentionally typed at
    /// the Cranelift boundary even though the C witness stores erased function
    /// pointers: this keeps incompatible signatures a lowering error instead
    /// of an ABI reinterpretation.
    fn lower_interface_thunk(&mut self, pending: &PendingInterfaceThunk) -> CodegenResult<()> {
        if let Type::Interface(source_name) = &pending.source_ty {
            return self.lower_interface_forward_thunk(pending, source_name);
        }
        if pending.impl_key.starts_with("@intrinsic:") {
            return self.lower_interface_intrinsic_thunk(pending);
        }
        let info = self
            .funcs
            .get(&pending.impl_key)
            .ok_or_else(|| CodegenError::internal("interface implementation disappeared"))?;
        let impl_params = info.params.clone();
        let impl_ret = info.ret.clone();
        let target_ret = method_return(&pending.method.signature);
        if impl_params.len() != pending.method.params.len() {
            return Err(CodegenError::unsupported(format!(
                "interface method `{}` and implementation `{}` have incompatible arity",
                pending.method.name, pending.impl_key
            )));
        }
        for (implementation, interface) in impl_params.iter().zip(&pending.method.params) {
            if is_receiver(&interface.name) {
                continue;
            }
            if repr_of(&implementation.ty)?.clif(self.pointer_ty).is_none()
                || repr_of(&interface.ty)?.clif(self.pointer_ty).is_none()
            {
                return Err(CodegenError::unsupported(format!(
                    "interface method `{}` contains a void parameter",
                    pending.method.name
                )));
            }
        }
        let sig = {
            let mut sig = Signature::new(self.call_conv);
            sig.params.push(AbiParam::new(self.pointer_ty));
            for param in pending.method.params.iter().skip(1) {
                sig.params.push(AbiParam::new(
                    repr_of(&param.ty)?.clif(self.pointer_ty).ok_or_else(|| {
                        CodegenError::unsupported("interface method parameter cannot be `void`")
                    })?,
                ));
            }
            if let Some(ret) = repr_of(&target_ret)?.clif(self.pointer_ty) {
                sig.returns.push(AbiParam::new(ret));
            }
            sig
        };
        let frontend_config = self.module.target_config();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let mut gen = FuncGen::new(self, builder, target_ret.clone());
            let interface_value = gen.builder.block_params(entry)[0];
            let payload_kind = interface_payload_kind(&pending.source_ty)?;
            let kind = gen
                .builder
                .ins()
                .iconst(types::I32, i64::from(payload_kind));
            let raw = gen.call_rt_value("lira_rt_interface_payload", &[interface_value, kind])?;
            let source_value = gen.slot_to_value(raw, &pending.source_ty)?;
            let mut call_args = Vec::with_capacity(impl_params.len());
            call_args.push(source_value);
            for (index, implementation) in impl_params.iter().enumerate().skip(1) {
                let target_param = &pending.method.params[index].ty;
                let value = gen.builder.block_params(entry)[index];
                call_args.push(gen.coerce(
                    value,
                    target_param,
                    &implementation.ty,
                    &Span { line: 0, column: 0 },
                )?);
            }

            let source_class = match &pending.source_ty {
                Type::Class(name) | Type::Struct(name)
                    if gen
                        .l
                        .layouts
                        .structs
                        .get(name)
                        .is_some_and(|layout| layout.is_class) =>
                {
                    Some(name.as_str())
                }
                _ => None,
            };
            let call = if let Some(class) = source_class {
                let layout = gen
                    .l
                    .layouts
                    .structs
                    .get(class)
                    .ok_or_else(|| CodegenError::internal("missing interface class layout"))?;
                let slot = layout.vtable_slot(&pending.method.name).ok_or_else(|| {
                    CodegenError::unsupported(format!(
                        "class `{class}` has no method `{}`",
                        pending.method.name
                    ))
                })?;
                let pointer_ty = gen.pointer_ty();
                let vtable = gen.builder.ins().load(
                    pointer_ty,
                    MemFlagsData::trusted(),
                    source_value,
                    CLASS_VTABLE_OFFSET,
                );
                let code = gen.builder.ins().load(
                    pointer_ty,
                    MemFlagsData::trusted(),
                    vtable,
                    slot as i32 * SLOT_SIZE,
                );
                let mut call_sig = Signature::new(gen.l.call_conv);
                for param in &impl_params {
                    call_sig.params.push(AbiParam::new(
                        repr_of(&param.ty)?.clif(gen.pointer_ty()).ok_or_else(|| {
                            CodegenError::unsupported("implementation parameter cannot be `void`")
                        })?,
                    ));
                }
                if let Some(ret) = repr_of(&impl_ret)?.clif(gen.pointer_ty()) {
                    call_sig.returns.push(AbiParam::new(ret));
                }
                let sig_ref = gen.builder.import_signature(call_sig);
                gen.builder.ins().call_indirect(sig_ref, code, &call_args)
            } else {
                let func_id = gen
                    .l
                    .funcs
                    .get(&pending.impl_key)
                    .ok_or_else(|| CodegenError::internal("interface implementation missing"))?
                    .func_id;
                let func_ref = gen.func_ref_by_id(func_id);
                gen.builder.ins().call(func_ref, &call_args)
            };
            let results = gen.builder.inst_results(call).to_vec();
            if matches!(target_ret, Type::Void) {
                gen.builder.ins().return_(&[]);
            } else {
                let result = results.first().copied().ok_or_else(|| {
                    CodegenError::internal("interface implementation returned no value")
                })?;
                let result =
                    gen.coerce(result, &impl_ret, &target_ret, &Span { line: 0, column: 0 })?;
                gen.builder.ins().return_(&[result]);
            }
            gen.builder.seal_all_blocks();
            gen.builder.finalize(frontend_config);
        }
        self.module
            .define_function(pending.func_id, &mut ctx)
            .map_err(|e| CodegenError::internal(format!("{}: {e}", pending.symbol)))?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    fn lower_interface_intrinsic_thunk(
        &mut self,
        pending: &PendingInterfaceThunk,
    ) -> CodegenResult<()> {
        let target_ret = method_return(&pending.method.signature);
        let mut sig = Signature::new(self.call_conv);
        sig.params.push(AbiParam::new(self.pointer_ty));
        for param in pending.method.params.iter().skip(1) {
            sig.params.push(AbiParam::new(
                repr_of(&param.ty)?.clif(self.pointer_ty).ok_or_else(|| {
                    CodegenError::unsupported("interface method parameter cannot be `void`")
                })?,
            ));
        }
        if let Some(ret) = repr_of(&target_ret)?.clif(self.pointer_ty) {
            sig.returns.push(AbiParam::new(ret));
        }
        let frontend_config = self.module.target_config();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let mut gen = FuncGen::new(self, builder, target_ret.clone());
            let receiver = gen.builder.block_params(entry)[0];
            let kind = gen.builder.ins().iconst(types::I32, 0);
            let raw = gen.call_rt_value("lira_rt_interface_payload", &[receiver, kind])?;
            let source_value = gen.slot_to_value(raw, &pending.source_ty)?;
            let intrinsic = pending.impl_key.as_str();
            let result = match intrinsic {
                "@intrinsic:string.len" => {
                    if pending.method.params.len() != 1 {
                        return Err(CodegenError::unsupported(
                            "String.len interface signature is incompatible",
                        ));
                    }
                    let value = gen.call_rt_value("lira_rt_str_len", &[source_value])?;
                    if matches!(target_ret, Type::Void) {
                        None
                    } else {
                        Some(gen.coerce(
                            value,
                            &Type::Int,
                            &target_ret,
                            &Span { line: 0, column: 0 },
                        )?)
                    }
                }
                "@intrinsic:array.len" => {
                    if pending.method.params.len() != 1 {
                        return Err(CodegenError::unsupported(
                            "Array.len interface signature is incompatible",
                        ));
                    }
                    let value = gen.call_rt_value("lira_rt_array_len", &[source_value])?;
                    if matches!(target_ret, Type::Void) {
                        None
                    } else {
                        Some(gen.coerce(
                            value,
                            &Type::Int,
                            &target_ret,
                            &Span { line: 0, column: 0 },
                        )?)
                    }
                }
                "@intrinsic:array.push" => {
                    let Type::Array(element_ty) = &pending.source_ty else {
                        return Err(CodegenError::internal(
                            "array push intrinsic has non-array source",
                        ));
                    };
                    if pending.method.params.len() != 2 || !matches!(target_ret, Type::Void) {
                        return Err(CodegenError::unsupported(
                            "Array.push interface signature is incompatible",
                        ));
                    }
                    let value = gen.builder.block_params(entry)[1];
                    let value = gen.coerce(
                        value,
                        &pending.method.params[1].ty,
                        element_ty,
                        &Span { line: 0, column: 0 },
                    )?;
                    let slot = gen.value_to_slot(value, element_ty)?;
                    gen.call_rt("lira_rt_array_push", &[source_value, slot])?;
                    None
                }
                "@intrinsic:array.pop" => {
                    let Type::Array(element_ty) = &pending.source_ty else {
                        return Err(CodegenError::internal(
                            "array pop intrinsic has non-array source",
                        ));
                    };
                    if pending.method.params.len() != 1 {
                        return Err(CodegenError::unsupported(
                            "Array.pop interface signature is incompatible",
                        ));
                    }
                    let value = gen.lower_array_pop(
                        source_value,
                        element_ty,
                        &Span { line: 0, column: 0 },
                    )?;
                    if matches!(target_ret, Type::Void) {
                        None
                    } else {
                        Some(gen.coerce(
                            value,
                            &Type::Optional(element_ty.clone()),
                            &target_ret,
                            &Span { line: 0, column: 0 },
                        )?)
                    }
                }
                _ => return Err(CodegenError::internal("unknown interface intrinsic")),
            };
            if let Some(result) = result {
                gen.builder.ins().return_(&[result]);
            } else {
                gen.builder.ins().return_(&[]);
            }
            gen.builder.seal_all_blocks();
            gen.builder.finalize(frontend_config);
        }
        self.module
            .define_function(pending.func_id, &mut ctx)
            .map_err(|e| CodegenError::internal(format!("{}: {e}", pending.symbol)))?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    fn lower_interface_forward_thunk(
        &mut self,
        pending: &PendingInterfaceThunk,
        source_name: &str,
    ) -> CodegenResult<()> {
        let source_method = self
            .layouts
            .interfaces
            .get(source_name)
            .and_then(|interface| interface.method(&pending.method.name))
            .cloned()
            .ok_or_else(|| {
                CodegenError::unsupported(format!(
                    "interface `{source_name}` has no method `{}`",
                    pending.method.name
                ))
            })?;
        let target_ret = method_return(&pending.method.signature);
        let source_ret = method_return(&source_method.signature);
        let mut sig = Signature::new(self.call_conv);
        sig.params.push(AbiParam::new(self.pointer_ty));
        for param in pending.method.params.iter().skip(1) {
            sig.params.push(AbiParam::new(
                repr_of(&param.ty)?.clif(self.pointer_ty).ok_or_else(|| {
                    CodegenError::unsupported("interface method parameter cannot be `void`")
                })?,
            ));
        }
        if let Some(ret) = repr_of(&target_ret)?.clif(self.pointer_ty) {
            sig.returns.push(AbiParam::new(ret));
        }
        let frontend_config = self.module.target_config();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let mut gen = FuncGen::new(self, builder, target_ret.clone());
            let receiver = gen.builder.block_params(entry)[0];
            let kind = gen.builder.ins().iconst(types::I32, 0);
            let source_receiver =
                gen.call_rt_value("lira_rt_interface_payload", &[receiver, kind])?;
            let source_index = gen
                .builder
                .ins()
                .iconst(types::I32, source_method.slot as i64);
            let code = gen.call_rt_value(
                "lira_rt_interface_method_slot",
                &[source_receiver, source_index],
            )?;
            let mut call_args = Vec::with_capacity(source_method.params.len());
            call_args.push(source_receiver);
            for (index, source_param) in source_method.params.iter().enumerate().skip(1) {
                let target_param = &pending.method.params[index].ty;
                let value = gen.builder.block_params(entry)[index];
                call_args.push(gen.coerce(
                    value,
                    target_param,
                    &source_param.ty,
                    &Span { line: 0, column: 0 },
                )?);
            }
            let mut call_sig = Signature::new(gen.l.call_conv);
            for param in &source_method.params {
                call_sig.params.push(AbiParam::new(
                    repr_of(&param.ty)?.clif(gen.pointer_ty()).ok_or_else(|| {
                        CodegenError::unsupported("interface method parameter cannot be `void`")
                    })?,
                ));
            }
            if let Some(ret) = repr_of(&source_ret)?.clif(gen.pointer_ty()) {
                call_sig.returns.push(AbiParam::new(ret));
            }
            let sig_ref = gen.builder.import_signature(call_sig);
            let call = gen.builder.ins().call_indirect(sig_ref, code, &call_args);
            let results = gen.builder.inst_results(call).to_vec();
            if matches!(target_ret, Type::Void) {
                gen.builder.ins().return_(&[]);
            } else {
                let result = results.first().copied().ok_or_else(|| {
                    CodegenError::internal("forwarded interface method returned no value")
                })?;
                let result = gen.coerce(
                    result,
                    &source_ret,
                    &target_ret,
                    &Span { line: 0, column: 0 },
                )?;
                gen.builder.ins().return_(&[result]);
            }
            gen.builder.seal_all_blocks();
            gen.builder.finalize(frontend_config);
        }
        self.module
            .define_function(pending.func_id, &mut ctx)
            .map_err(|e| CodegenError::internal(format!("{}: {e}", pending.symbol)))?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }
}

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Structs are value types in Lira, while classes and all other heap
    /// aggregates are references. The native ABI represents both with a
    /// pointer, so value boundaries call one reusable, layout-specific helper.
    fn copy_value_for_type(&mut self, value: Value, ty: &Type) -> CodegenResult<Value> {
        let copy_ctx = self.call_rt_value("lira_rt_copy_ctx_new", &[])?;
        let result = self.copy_value_for_type_with_context(value, ty, copy_ctx)?;
        self.call_rt("lira_rt_copy_ctx_free", &[copy_ctx])?;
        Ok(result)
    }

    /// Copy a value while sharing the memoizing context with the enclosing
    /// aggregate copy. Tuples are arrays at run time, but they are immutable
    /// value aggregates: allocate a fresh array and recursively copy only
    /// value-semantic elements. Arrays, maps, channels, classes, and other
    /// reference-semantic values remain pointers in their tuple slots.
    fn copy_value_for_type_with_context(
        &mut self,
        value: Value,
        ty: &Type,
        copy_ctx: Value,
    ) -> CodegenResult<Value> {
        let ty = self.l.normalize(ty.clone());
        match ty {
            Type::Tuple(element_types) => self.copy_tuple_for_type(value, &element_types, copy_ctx),
            Type::Any => self.copy_any_boundary(value),
            Type::Optional(inner) => {
                if self.is_copyable_value_type(&inner) {
                    self.copy_value_for_type_with_context(value, &inner, copy_ctx)
                } else {
                    Ok(value)
                }
            }
            Type::Struct(name) | Type::Class(name) => {
                let Some(layout) = self.l.layouts.structs.get(&name) else {
                    return Ok(value);
                };
                if layout.is_class {
                    return Ok(value);
                }
                let helper_id = self.l.ensure_copy_helper(&name)?;
                let helper = self.func_ref_by_id(helper_id);
                let call = self.builder.ins().call(helper, &[value, copy_ctx]);
                Ok(self.builder.inst_results(call)[0])
            }
            _ => Ok(value),
        }
    }

    /// Copy one tuple through the same context used by nested value structs.
    /// The null path matters for optional tuples and mirrors the null guard in
    /// generated struct copy helpers. Publishing the destination before
    /// descending makes recursive struct -> tuple -> struct edges terminate
    /// without imposing a fixed recursion depth.
    fn copy_tuple_for_type(
        &mut self,
        source: Value,
        element_types: &[Type],
        copy_ctx: Value,
    ) -> CodegenResult<Value> {
        let null_block = self.builder.create_block();
        let lookup_block = self.builder.create_block();
        let found_block = self.builder.create_block();
        let allocate_block = self.builder.create_block();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, self.pointer_ty());

        let is_null = self.builder.ins().icmp_imm_s(IntCC::Equal, source, 0);
        self.builder
            .ins()
            .brif(is_null, null_block, &[], lookup_block, &[]);
        self.terminated = true;

        self.goto(null_block);
        let pointer_ty = self.pointer_ty();
        let null_value = self.builder.ins().iconst(pointer_ty, 0);
        self.jump_to(merge, &[null_value]);

        self.goto(lookup_block);
        let existing = self.call_rt_value("lira_rt_copy_ctx_lookup", &[copy_ctx, source])?;
        let has_existing = self.builder.ins().icmp_imm_s(IntCC::NotEqual, existing, 0);
        self.builder
            .ins()
            .brif(has_existing, found_block, &[], allocate_block, &[]);
        self.terminated = true;

        self.goto(found_block);
        self.jump_to(merge, &[existing]);

        self.goto(allocate_block);
        let element_count = self
            .builder
            .ins()
            .iconst(types::I64, element_types.len() as i64);
        let copy = self.call_rt_value("lira_rt_array_new", &[element_count])?;
        self.call_rt("lira_rt_copy_ctx_insert", &[copy_ctx, source, copy])?;
        for (index, element_ty) in element_types.iter().enumerate() {
            let index_value = self.builder.ins().iconst(types::I64, index as i64);
            let slot = self.call_rt_value("lira_rt_array_get", &[source, index_value])?;
            let element_ty = self.l.normalize(element_ty.clone());
            let element = self.slot_to_value(slot, &element_ty)?;
            let element = self.copy_value_for_type_with_context(element, &element_ty, copy_ctx)?;
            let slot = self.value_to_slot(element, &element_ty)?;
            self.call_rt("lira_rt_array_push", &[copy, slot])?;
        }
        self.jump_to(merge, &[copy]);

        self.goto(merge);
        Ok(self.builder.block_params(merge)[0])
    }

    /// Copy a value at a semantic value boundary. This helper is intentionally
    /// separate from lvalue lowering: callers that are computing an address
    /// must use the original receiver, not a detached copy.
    fn copy_value_boundary(&mut self, value: Value, ty: &Type) -> CodegenResult<Value> {
        if matches!(self.l.normalize(ty.clone()), Type::Any) {
            self.copy_any_boundary(value)
        } else {
            self.copy_value_for_type(value, ty)
        }
    }

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
                is_mutable: param.is_mutable,
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
            let value = self.copy_value_boundary(value, ty)?;
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
    /// Adapt a present optional payload while preserving `null` as the empty
    /// case. Pointer equality of `T?` and `U?` is not enough when their payload
    /// ABIs differ or `U` is an interface/`Any` that needs a real wrapper.
    fn coerce_optional(
        &mut self,
        value: Value,
        from_inner: &Type,
        to_inner: &Type,
        span: &Span,
    ) -> CodegenResult<Value> {
        let missing = self.builder.create_block();
        let present = self.builder.create_block();
        let merge = self.builder.create_block();
        let pointer_ty = self.pointer_ty();
        self.builder.append_block_param(merge, pointer_ty);

        let is_null = self.builder.ins().icmp_imm_s(IntCC::Equal, value, 0);
        self.builder.ins().brif(is_null, missing, &[], present, &[]);
        self.terminated = true;

        self.goto(missing);
        let null = self.builder.ins().iconst(pointer_ty, 0);
        self.jump_to(merge, &[null]);

        self.goto(present);
        let payload = self.unwrap_optional(value, from_inner)?;
        let payload = self.coerce(payload, from_inner, to_inner, span)?;
        let wrapped = self.wrap_optional(payload, to_inner, to_inner, span)?;
        self.jump_to(merge, &[wrapped]);

        self.goto(merge);
        Ok(self.builder.block_params(merge)[0])
    }

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
    /// Materialize every channel and send value once, then ask the runtime to
    /// arbitrate all communication arms in one cooperative operation. The
    /// descriptor's ordinal is the original source-arm ordinal, so a default
    /// arm between two communication arms does not renumber their scores.
    fn lower_select(
        &mut self,
        arms: &[SelectArm],
        result_ty: &Type,
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        let default_arm = arms
            .iter()
            .find(|arm| matches!(arm.kind, SelectArmKind::Default));
        let channel_arms: Vec<&SelectArm> = arms
            .iter()
            .filter(|arm| !matches!(arm.kind, SelectArmKind::Default))
            .collect();

        if channel_arms.is_empty() {
            if let Some(arm) = default_arm {
                self.push_scope();
                let value = self.lower_expr_typed(&arm.body, result_ty)?;
                self.pop_scope();
                return Ok(value);
            }
            return Err(CodegenError::unsupported_at(
                "`select` needs at least one arm",
                span,
            ));
        }

        enum NativeArm<'x> {
            Recv {
                arm: &'x SelectArm,
                variable: Option<&'x str>,
                element_ty: Type,
            },
            Send {
                arm: &'x SelectArm,
            },
        }

        // The C ABI is { pointer, i64, u64, u8 + 7 bytes padding }, 32 bytes
        // per descriptor and 8-byte aligned. The stack slot itself is a GC
        // root while this select can yield; no descriptor probing allocates.
        let descriptor_size = channel_arms
            .len()
            .checked_mul(32)
            .ok_or_else(|| CodegenError::internal("select descriptor storage is too large"))?;
        let descriptors = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            descriptor_size as u32,
            3,
        ));

        // `lira_rt_select` writes a received slot only after it has selected a
        // receive arm. Keep that slot live across the retry loop.
        let received = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            SLOT_SIZE as u32,
            3,
        ));

        let pointer_ty = self.pointer_ty();
        let mut native_arms = Vec::with_capacity(channel_arms.len());
        for (descriptor_index, arm) in channel_arms.iter().enumerate() {
            let offset = (descriptor_index * 32) as i32;
            let channel = match &arm.kind {
                SelectArmKind::Recv { variable, channel } => {
                    let channel_ty = self.ty_of(channel)?;
                    let Type::Channel(element_ty) = &channel_ty else {
                        return Err(CodegenError::unsupported_at(
                            "a receive arm expects a channel",
                            &arm.span,
                        ));
                    };
                    let channel_value = self.lower_expr_value(channel, &channel_ty)?;
                    self.builder
                        .ins()
                        .stack_store(pointer_ty, channel_value, descriptors, offset);
                    // The value field is ignored by receives, but initializing
                    // it makes the descriptor fully defined for C sanitizers.
                    let zero = self.builder.ins().iconst(types::I64, 0);
                    self.builder
                        .ins()
                        .stack_store(pointer_ty, zero, descriptors, offset + 8);
                    let ty = if matches!(element_ty.as_ref(), Type::Unknown | Type::TypeVar(_)) {
                        // An erased `chan()` stores a real LiraAny pointer in
                        // its slot. Keep the receive dynamic as well; reading
                        // that pointer as a scalar would expose the address.
                        Type::Any
                    } else {
                        element_ty.as_ref().clone()
                    };
                    native_arms.push(NativeArm::Recv {
                        arm,
                        variable: variable.as_deref(),
                        element_ty: ty,
                    });
                    0u8
                }
                SelectArmKind::Send { value, channel } => {
                    let channel_ty = self.ty_of(channel)?;
                    let Type::Channel(channel_element_ty) = &channel_ty else {
                        return Err(CodegenError::unsupported_at(
                            "a send arm expects a channel",
                            &arm.span,
                        ));
                    };
                    // `chan()` starts as `Channel<unknown>`. The checker
                    // refines direct bindings, but method bodies are checked
                    // without expression tables, so native lowering must use
                    // the concrete value type at this boundary as well.
                    let element_ty = if matches!(
                        channel_element_ty.as_ref(),
                        Type::Unknown | Type::TypeVar(_)
                    ) {
                        infer_or_checked_with(self.l, &|name| self.binding_type(name), value)
                            .or_else(|| self.infer_or_checked(value))
                            .unwrap_or(Type::Any)
                    } else {
                        channel_element_ty.as_ref().clone()
                    };
                    let channel_value = self.lower_expr_value(channel, &channel_ty)?;
                    self.builder
                        .ins()
                        .stack_store(pointer_ty, channel_value, descriptors, offset);
                    let sent = self.lower_channel_payload(value, &element_ty)?;
                    let slot = self.value_to_slot(sent, &element_ty)?;
                    self.builder
                        .ins()
                        .stack_store(pointer_ty, slot, descriptors, offset + 8);
                    native_arms.push(NativeArm::Send { arm });
                    1u8
                }
                SelectArmKind::Default => unreachable!("filtered out above"),
            };
            let ordinal = arms
                .iter()
                .position(|candidate| std::ptr::eq(candidate, *arm))
                .ok_or_else(|| CodegenError::internal("select arm disappeared during lowering"))?;
            let ordinal_value = self.builder.ins().iconst(types::I64, ordinal as i64);
            self.builder
                .ins()
                .stack_store(pointer_ty, ordinal_value, descriptors, offset + 16);
            let operation = self.builder.ins().iconst(types::I8, i64::from(channel));
            self.builder
                .ins()
                .stack_store(pointer_ty, operation, descriptors, offset + 24);
        }

        let retry = self.builder.create_block();
        let no_ready = self.builder.create_block();
        let dispatch = self.builder.create_block();
        let merge = self.builder.create_block();
        let result_clif = repr_of(result_ty)?.clif(self.pointer_ty());
        if let Some(clif) = result_clif {
            self.builder.append_block_param(merge, clif);
        }
        self.jump_to(retry, &[]);
        self.goto(retry);

        let descriptor_ptr = self.builder.ins().stack_addr(pointer_ty, descriptors, 0);
        let received_ptr = self.builder.ins().stack_addr(pointer_ty, received, 0);
        let count = self
            .builder
            .ins()
            .iconst(types::I64, channel_arms.len() as i64);
        let selected =
            self.call_rt_value("lira_rt_select", &[descriptor_ptr, count, received_ptr])?;
        let no_arm = self.builder.ins().icmp_imm_s(IntCC::Equal, selected, -1);
        self.builder
            .ins()
            .brif(no_arm, no_ready, &[], dispatch, &[]);
        self.terminated = true;

        let body_blocks: Vec<Block> = native_arms
            .iter()
            .map(|_| self.builder.create_block())
            .collect();
        let invalid_dispatch = self.builder.create_block();
        self.goto(dispatch);
        for (index, body_block) in body_blocks.iter().enumerate() {
            let next = if index + 1 == body_blocks.len() {
                invalid_dispatch
            } else {
                self.builder.create_block()
            };
            let arm_index = self
                .builder
                .ins()
                .icmp_imm_s(IntCC::Equal, selected, index as i64);
            self.builder
                .ins()
                .brif(arm_index, *body_block, &[], next, &[]);
            self.terminated = true;
            self.goto(next);
        }
        self.builder.ins().trap(unreachable_trap());
        self.terminated = true;

        let mut merge_reached = false;
        for (native_arm, body_block) in native_arms.iter().zip(body_blocks.iter()) {
            self.goto(*body_block);
            match native_arm {
                NativeArm::Recv {
                    arm,
                    variable,
                    element_ty,
                } => {
                    self.push_scope();
                    if let Some(variable) = variable {
                        let ptr = self.pointer_ty();
                        let slot = self.builder.ins().stack_load(ptr, types::I64, received, 0);
                        let value = if matches!(element_ty, Type::Any) {
                            self.call_rt_value("lira_rt_any_from_slot", &[slot])?
                        } else {
                            self.slot_to_value(slot, element_ty)?
                        };
                        let value = self.copy_value_boundary(value, element_ty)?;
                        self.declare_local(variable, element_ty.clone(), Some(value))?;
                    }
                    let value = self.lower_expr_typed(&arm.body, result_ty)?;
                    if !self.terminated {
                        merge_reached = true;
                        self.jump_expression_result(merge, value, result_ty, &arm.body.span)?;
                    }
                    self.pop_scope();
                }
                NativeArm::Send { arm } => {
                    self.push_scope();
                    let value = self.lower_expr_typed(&arm.body, result_ty)?;
                    if !self.terminated {
                        merge_reached = true;
                        self.jump_expression_result(merge, value, result_ty, &arm.body.span)?;
                    }
                    self.pop_scope();
                }
            }
        }

        // Nothing was ready: use the default, or park and retry with the same
        // descriptor values (so channel/value side effects happen once).
        self.goto(no_ready);
        match default_arm {
            Some(arm) => {
                self.push_scope();
                let value = self.lower_expr_typed(&arm.body, result_ty)?;
                if !self.terminated {
                    merge_reached = true;
                    self.jump_expression_result(merge, value, result_ty, &arm.body.span)?;
                }
                self.pop_scope();
            }
            None => {
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
        Ok(result_clif.map(|_| self.builder.block_params(merge)[0]))
    }
}

// ====================================================================== //
// Classes and virtual dispatch                                            //
// ====================================================================== //

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Find the method `name` on `type_name`, walking up the inheritance chain.
    ///
    /// A subclass that does not override a method has no entry of its own; the
    /// implementation lives on an ancestor.
    fn resolve_method(&self, type_name: &str, method: &str) -> Option<String> {
        let mut current = Some(type_name.to_string());
        while let Some(name) = current {
            let key = fn_key(Some(&name), method);
            if self.l.funcs.contains_key(&key) {
                return Some(key);
            }
            current = self
                .l
                .layouts
                .structs
                .get(&name)
                .and_then(|layout| layout.parent.clone());
        }
        None
    }

    /// The address of a class's virtual method table.
    ///
    /// Emitted once per class into read-only data, with a descriptor relocation
    /// followed by one function relocation per slot, so building an instance is
    /// a single store rather than a loop.
    fn class_vtable(&mut self, class: &str) -> CodegenResult<Value> {
        let data_id = match self.l.vtables.get(class) {
            Some(id) => *id,
            None => {
                let layout = self
                    .l
                    .layouts
                    .structs
                    .get(class)
                    .ok_or_else(|| CodegenError::internal(format!("unknown class `{}`", class)))?
                    .clone();

                // Keep the concrete class descriptor beside the function
                // slots. Instances point at the first function slot below;
                // the runtime walks back one slot when a class value crosses
                // an erased boundary. Interning the descriptor as a normal
                // static LiraStr gives both AOT and JIT data relocations the
                // same immutable target and keeps the descriptor grammar in
                // one place.
                let descriptor = self.any_type_descriptor(&Type::Class(class.to_owned()));
                if !self.l.strings.contains_key(&descriptor) {
                    let _ = self.string_constant(&descriptor)?;
                }
                let descriptor_id =
                    *self.l.strings.get(&descriptor).ok_or_else(|| {
                        CodegenError::internal("class descriptor was not interned")
                    })?;

                let mut description = DataDescription::new();
                description.define(
                    vec![0u8; (layout.vtable.len().max(1) + 1) * SLOT_SIZE as usize]
                        .into_boxed_slice(),
                );
                description.set_align(8);
                let descriptor_ref = self
                    .l
                    .module
                    .declare_data_in_data(descriptor_id, &mut description);
                description.write_data_addr(0, descriptor_ref, 0);
                for (slot, entry) in layout.vtable.iter().enumerate() {
                    let key = fn_key(Some(&entry.owner), &entry.method);
                    let func_id = self
                        .l
                        .funcs
                        .get(&key)
                        .ok_or_else(|| CodegenError::internal(format!("`{}` is missing", key)))?
                        .func_id;
                    let func_ref = self
                        .l
                        .module
                        .declare_func_in_data(func_id, &mut description);
                    description
                        .write_function_addr(((slot as i32 + 1) * SLOT_SIZE) as u32, func_ref);
                }

                let symbol = format!("lira__vtable__{}", class);
                let id = self
                    .l
                    .module
                    .declare_data(&symbol, Linkage::Local, false, false)
                    .map_err(|e| CodegenError::internal(e.to_string()))?;
                self.l
                    .module
                    .define_data(id, &description)
                    .map_err(|e| CodegenError::internal(e.to_string()))?;
                self.l.vtables.insert(class.to_string(), id);
                id
            }
        };
        let gv = self.global_value(data_id);
        let ptr = self.pointer_ty();
        let base = self.builder.ins().symbol_value(ptr, gv);
        Ok(self.builder.ins().iadd_imm_s(base, i64::from(SLOT_SIZE)))
    }

    /// Call a class method through the receiver's virtual method table.
    ///
    /// The static type only fixes the slot; which implementation runs comes from
    /// the instance, which is what makes an inherited `describe()` reach a
    /// subclass's `speak()` override.
    fn lower_virtual_call(
        &mut self,
        class: &str,
        method: &str,
        self_value: Value,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        let layout = self
            .l
            .layouts
            .structs
            .get(class)
            .ok_or_else(|| {
                CodegenError::unsupported_at(format!("unknown class `{}`", class), span)
            })?
            .clone();
        let Some(slot) = layout.vtable_slot(method) else {
            return Err(CodegenError::unsupported_at(
                format!("`{}` has no method `{}`", class, method),
                span,
            ));
        };

        // The signature comes from the declaration the slot currently holds;
        // an override has to match it, which the checker enforces.
        let key = fn_key(Some(&layout.vtable[slot].owner), method);
        let info = self
            .l
            .funcs
            .get(&key)
            .ok_or_else(|| CodegenError::internal(format!("`{}` is missing", key)))?;
        let ret = info.ret.clone();
        let params: Vec<(String, Type, Option<Expression>)> = info
            .params
            .iter()
            .map(|p| (p.name.clone(), p.ty.clone(), p.default.clone()))
            .collect();

        let mut sig = Signature::new(self.l.call_conv);
        for (_, ty, _) in &params {
            let clif = repr_of(ty)?
                .clif(self.pointer_ty())
                .ok_or_else(|| CodegenError::internal("a parameter cannot be `void`"))?;
            sig.params.push(AbiParam::new(clif));
        }
        if let Some(clif) = repr_of(&ret)?.clif(self.pointer_ty()) {
            sig.returns.push(AbiParam::new(clif));
        }
        let sig_ref = self.builder.import_signature(sig);

        let ptr = self.pointer_ty();
        let vtable = self.builder.ins().load(
            ptr,
            MemFlagsData::trusted(),
            self_value,
            CLASS_VTABLE_OFFSET,
        );
        let code = self.builder.ins().load(
            ptr,
            MemFlagsData::trusted(),
            vtable,
            slot as i32 * SLOT_SIZE,
        );

        let call_args = self.build_method_args(&key, &params, self_value, args, span)?;
        let call = self.builder.ins().call_indirect(sig_ref, code, &call_args);
        let results = self.builder.inst_results(call);
        let result = results.first().copied();
        Ok(if matches!(ret, Type::Void) {
            None
        } else {
            result
        })
    }

    /// `super.method(...)` — the parent's implementation, without dispatch.
    fn lower_super_call(
        &mut self,
        method: &str,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Option<Value>> {
        let receiver = self
            .lookup("self")
            .or_else(|| self.lookup("this"))
            .ok_or_else(|| CodegenError::unsupported_at("`super` outside of a method", span))?;
        let receiver_ty = match &receiver {
            Binding::Local { ty, .. } => ty.clone(),
            Binding::Global(global) => global.ty.clone(),
        };
        let (Type::Struct(class) | Type::Class(class)) = receiver_ty.clone() else {
            return Err(CodegenError::unsupported_at(
                "`super` outside of a class",
                span,
            ));
        };
        let parent = self
            .l
            .layouts
            .structs
            .get(&class)
            .and_then(|layout| layout.parent.clone())
            .ok_or_else(|| {
                CodegenError::unsupported_at(format!("`{}` has no parent class", class), span)
            })?;
        let key = self.resolve_method(&parent, method).ok_or_else(|| {
            CodegenError::unsupported_at(format!("`{}` has no method `{}`", parent, method), span)
        })?;
        let self_value = self.load_binding(&receiver);
        self.lower_user_call(&key, Some(self_value), args, span)
    }

    /// Evaluate a method call's arguments into the order the declaration wants.
    fn build_method_args(
        &mut self,
        key: &str,
        params: &[(String, Type, Option<Expression>)],
        self_value: Value,
        args: &[Argument],
        span: &Span,
    ) -> CodegenResult<Vec<Value>> {
        let explicit = &params[1..];
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
            slots[index] = Some(self.lower_expr_value(&arg.value, &explicit[index].1)?);
        }

        let mut call_args = Vec::with_capacity(params.len());
        call_args.push(self_value);
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
        Ok(call_args)
    }
}

// ====================================================================== //
// Generics                                                                //
// ====================================================================== //

impl<'a> Lowerer<'a> {
    /// Declare an instantiation of a generic function or method, queueing its
    /// body, and return the key it is registered under.
    ///
    /// Instantiating the same bindings twice is a no-op, which is what makes a
    /// generic that calls itself terminate.
    fn instantiate_fn(
        &mut self,
        template_key: &str,
        args: &[Type],
        span: &Span,
    ) -> CodegenResult<String> {
        let index = *self.generic_index.get(template_key).ok_or_else(|| {
            CodegenError::unsupported_at(format!("`{}` is not generic", template_key), span)
        })?;
        let template = &self.generic_fns[index];
        if template.type_params.len() != args.len() {
            return Err(CodegenError::unsupported_at(
                format!(
                    "`{}` takes {} type argument(s), not {}",
                    template_key,
                    template.type_params.len(),
                    args.len()
                ),
                span,
            ));
        }
        let key = mangle(template_key, args);
        if self.instances.contains_key(&key) {
            return Ok(key);
        }

        let bindings: HashMap<String, Type> = template
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        let in_scope: HashSet<String> = template.type_params.iter().cloned().collect();
        let owner = template.owner.clone();
        let owner_param_count = template.owner_param_count;
        let params = template.params.clone();
        let return_type = template.return_type.clone();
        // The owner's arguments are the trailing ones; a method with type
        // parameters of its own puts those first.
        let owner_args: Vec<Type> = args[args.len() - owner_param_count..].to_vec();

        // Resolve the signature under the bindings, exactly as the body will be.
        let previous = std::mem::replace(&mut self.bindings, bindings.clone());
        let mut param_infos = Vec::with_capacity(params.len());
        let mut failure = None;
        for param in &params {
            let ty = if is_receiver(&param.name) {
                match &owner {
                    Some(type_name) if owner_param_count > 0 => {
                        Type::Struct(mangle(type_name, &owner_args))
                    }
                    Some(type_name) => self.user_type(type_name),
                    None => {
                        failure = Some(CodegenError::unsupported_at(
                            "`self` outside of a method",
                            &param.span,
                        ));
                        break;
                    }
                }
            } else {
                match self.resolve_ann(&param.type_ann, &in_scope) {
                    Ok(ty) => ty,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            };
            param_infos.push(ParamInfo {
                name: param.name.clone(),
                ty,
                default: param.default.clone(),
                is_mutable: param.is_mutable,
            });
        }
        let ret = match return_type.as_ref() {
            Some(t) => match self.resolve_ann(t, &in_scope) {
                Ok(ty) => ty,
                Err(error) => {
                    self.bindings = previous;
                    return Err(error);
                }
            },
            None => Type::Any,
        };
        self.bindings = previous;
        if let Some(error) = failure {
            return Err(error);
        }

        let sig = self.signature_for(&param_infos, &ret)?;
        let symbol = format!("lira__{}", sanitise_symbol(&key));
        let func_id = self
            .module
            .declare_function(&symbol, Linkage::Local, &sig)
            .map_err(|e| CodegenError::internal(e.to_string()))?;

        self.funcs.insert(
            key.clone(),
            FnInfo {
                symbol,
                func_id,
                params: param_infos,
                ret,
                owner: owner.clone(),
            },
        );
        self.instances.insert(key.clone(), index);
        self.pending_instances.push(PendingInstance {
            key: key.clone(),
            template: index,
            bindings,
        });
        Ok(key)
    }

    /// Lower one queued instantiation, with its bindings in force.
    fn lower_instance(&mut self, pending: &PendingInstance) -> CodegenResult<()> {
        // Copy the template out first: lowering borrows `self` mutably, and the
        // body is shared by every instantiation.
        let template = &self.generic_fns[pending.template];
        let name = template.name.clone();
        let params = template.params.clone();
        let return_type = template.return_type.clone();
        let body = template.body.clone();
        let span = template.span.clone();

        let decl = FnDeclRef {
            name: &name,
            type_params: &[],
            owner_type_params: None,
            params: &params,
            return_type: return_type.as_ref(),
            body: &body,
            span: &span,
        };
        // `lower_function` looks the signature up by key, so hand it the
        // instantiated one rather than the template's name.
        let previous = std::mem::replace(&mut self.bindings, pending.bindings.clone());
        let result = self.lower_function_as(&pending.key, decl);
        self.bindings = previous;
        result
    }

    /// Resolve a type annotation, instantiating any generic type it names.
    ///
    /// This is what makes `fn describe(o: Opt<int>)` build the `Opt$int` layout
    /// at declaration time, rather than waiting for something to construct one.
    fn resolve_ann(
        &mut self,
        ann: &lirac::ast::TypeExpr,
        in_scope: &HashSet<String>,
    ) -> CodegenResult<Type> {
        use lirac::ast::TypeExprKind;
        match &ann.kind {
            TypeExprKind::Generic { name, args } if name == "Channel" => {
                let [element] = args.as_slice() else {
                    return Err(CodegenError::unsupported_at(
                        "`Channel` takes exactly one type argument",
                        &ann.span,
                    ));
                };
                Ok(Type::Channel(Box::new(
                    self.resolve_ann(element, in_scope)?,
                )))
            }
            TypeExprKind::Generic { name, args } if self.layouts.generics.contains_key(name) => {
                let mut resolved = Vec::with_capacity(args.len());
                for arg in args {
                    resolved.push(self.resolve_ann(arg, in_scope)?);
                }
                let resolved: Vec<Type> = resolved
                    .iter()
                    .map(|ty| substitute(ty, &self.bindings))
                    .collect();
                // Still generic — this is a template's own signature, not a use.
                if resolved.iter().any(|ty| matches!(ty, Type::TypeParam(_))) {
                    return Ok(Type::Struct(mangle(name, &resolved)));
                }
                let name = name.clone();
                let span = ann.span.clone();
                self.instantiate_type(&name, &resolved, &span)
            }
            TypeExprKind::Optional(inner) => {
                Ok(Type::Optional(Box::new(self.resolve_ann(inner, in_scope)?)))
            }
            TypeExprKind::Array(inner) => {
                Ok(Type::Array(Box::new(self.resolve_ann(inner, in_scope)?)))
            }
            _ => Ok(self.normalize(layout::type_of_ann_in(ann, in_scope))),
        }
    }

    /// Instantiate a generic aggregate and its methods, returning the concrete
    /// type. `Box<int>` becomes `Struct("Box$int")`.
    fn instantiate_type(&mut self, name: &str, args: &[Type], span: &Span) -> CodegenResult<Type> {
        let mangled = self.layouts.instantiate(name, args)?;
        self.type_instances
            .entry(mangled.clone())
            .or_insert_with(|| (name.to_string(), args.to_vec()));
        // Guard against re-entry: resolving a method's signature can name the
        // very type being instantiated.
        if !self.instantiated_types.insert(mangled.clone()) {
            return Ok(self.user_type(&mangled));
        }
        // The methods are registered under the mangled owner so an ordinary
        // method call finds them.
        let templates: Vec<(String, usize)> = self
            .generic_index
            .iter()
            .filter(|(key, _)| key.starts_with(&format!("{}::", name)))
            .map(|(key, index)| (key.clone(), *index))
            .collect();
        for (key, index) in templates {
            let template = &self.generic_fns[index];
            if template.type_params.len() != args.len() {
                continue;
            }
            let method = template.name.clone();
            let instance = self.instantiate_fn(&key, args, span)?;
            // Re-register under `Box$int::get` so method dispatch resolves it.
            if let Some(info) = self.funcs.get(&instance) {
                let cloned = FnInfo {
                    symbol: info.symbol.clone(),
                    func_id: info.func_id,
                    params: info.params.clone(),
                    ret: info.ret.clone(),
                    owner: Some(mangled.clone()),
                };
                self.funcs.insert(fn_key(Some(&mangled), &method), cloned);
            }
        }
        Ok(self.user_type(&mangled))
    }

    /// Recover the generic aggregate template behind a concrete mangled name.
    /// The explicit map is authoritative; the prefix fallback also lets type
    /// inference inspect a literal before its layout has been instantiated.
    fn generic_template_name<'b>(&'b self, concrete: &str) -> Option<&'b str> {
        if let Some((name, _)) = self.type_instances.get(concrete) {
            return Some(name);
        }
        self.layouts
            .generics
            .keys()
            .find(|name| concrete.starts_with(&format!("{}$", name)))
            .map(String::as_str)
    }
}

impl ParamInfo {
    fn clone_info(&self) -> ParamInfo {
        ParamInfo {
            name: self.name.clone(),
            ty: self.ty.clone(),
            default: self.default.clone(),
            is_mutable: self.is_mutable,
        }
    }
}

impl Clone for ParamInfo {
    fn clone(&self) -> Self {
        self.clone_info()
    }
}

/// The instantiated name of a generic struct literal, from the values its
/// fields were given. `None` when the type is not generic.
fn generic_literal_type(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    name: &str,
    fields: &[(String, Expression)],
) -> Option<Type> {
    let args = generic_literal_args(l, resolve, name, fields)?;
    // The template says which kind it is; the layout may not exist yet, and
    // `user_type` would guess "struct" for a name it has not seen.
    Some(Type::Struct(mangle(name, &args)))
}

fn generic_literal_args(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    name: &str,
    fields: &[(String, Expression)],
) -> Option<Vec<Type>> {
    let template = l.layouts.generics.get(name)?;
    let in_scope: HashSet<String> = template.type_params.iter().cloned().collect();
    let mut bindings = HashMap::new();
    for (field_name, declared) in &template.fields {
        let Some((_, value)) = fields.iter().find(|(given, _)| given == field_name) else {
            continue;
        };
        unify_declared_expression(l, resolve, declared, value, &in_scope, &mut bindings)?;
    }
    let args: Vec<Type> = template
        .type_params
        .iter()
        .map(|param| bindings.get(param).cloned())
        .collect::<Option<_>>()?;
    Some(args)
}

/// Unify a field annotation with its expression, preserving the structure of
/// nested generic aggregates. The erased checker type for `Box<T>` is the
/// opaque name `Box$T`; a nested `Box { value: 1 }` still carries the concrete
/// argument in its literal fields, so use that expression before falling back
/// to the flattened type name.
fn unify_declared_expression(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    declared: &lirac::ast::TypeExpr,
    value: &Expression,
    in_scope: &HashSet<String>,
    bindings: &mut HashMap<String, Type>,
) -> Option<()> {
    let actual = infer_or_checked_with(l, resolve, value)?;
    // A present value flows directly into an optional field. Peel only the
    // declared wrapper for inference so a `Maybe<T>` constructor can determine
    // the `T` in a declared `Maybe<T>?`; null itself intentionally carries no
    // type information.
    let declared = match (&declared.kind, &actual) {
        (lirac::ast::TypeExprKind::Optional(inner), actual)
            if !matches!(actual, Type::Optional(_) | Type::Null) =>
        {
            inner.as_ref()
        }
        _ => declared,
    };
    if let lirac::ast::TypeExprKind::Generic { name, args } = &declared.kind {
        if l.layouts.generics.contains_key(name) {
            let nested = match &value.kind {
                ExpressionKind::StructLiteral {
                    name: Some(literal_name),
                    fields,
                } if literal_name == name => generic_literal_args(l, resolve, name, fields),
                _ => match &actual {
                    Type::Struct(_) | Type::Enum(_) | Type::Class(_) => {
                        l.generic_owner_args(name, &actual)
                    }
                    _ => None,
                },
            };
            if let Some(nested) = nested {
                for (argument, concrete) in args.iter().zip(nested) {
                    unify(
                        &layout::type_of_ann_in(argument, in_scope),
                        &concrete,
                        bindings,
                    );
                }
                return Some(());
            }
        }
    }
    unify(
        &layout::type_of_ann_in(declared, in_scope),
        &actual,
        bindings,
    );
    Some(())
}

/// Infer a field's concrete type while its generic aggregate layout is still
/// waiting for the receiver call to be lowered. This matters for expressions
/// such as `make(42).value`: `make` supplies all arguments, but the layout is
/// only materialized by its native instantiation queue.
fn generic_field_type(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    receiver: &Expression,
    concrete_name: &str,
    field: &str,
) -> Option<Type> {
    let template_name = l.generic_template_name(concrete_name)?;
    let template = l.layouts.generics.get(template_name)?;
    let arguments = match &receiver.kind {
        ExpressionKind::StructLiteral {
            name: Some(name),
            fields,
        } if name == template_name => generic_literal_args(l, resolve, name, fields)?,
        ExpressionKind::Call {
            callee,
            type_args,
            args,
        } => {
            let arg_types = args
                .iter()
                .map(|arg| infer_or_checked_with(l, resolve, &arg.value))
                .collect::<Option<Vec<_>>>()?;
            let explicit = type_args
                .iter()
                .map(|arg| l.normalize(layout::type_of_ann(arg)))
                .collect::<Vec<_>>();
            let (function, receiver_ty, receiver_args) = match &callee.kind {
                ExpressionKind::Identifier(function) => (function.clone(), None, None),
                ExpressionKind::FieldAccess { object, field } => {
                    if let ExpressionKind::Identifier(type_name) = &object.kind {
                        if resolve(type_name).is_none()
                            && (l.layouts.is_aggregate(type_name)
                                || l.layouts.generics.contains_key(type_name))
                        {
                            (fn_key(Some(type_name), field), None, None)
                        } else {
                            let receiver_ty = infer_or_checked_with(l, resolve, object)?;
                            let name = match &receiver_ty {
                                Type::Struct(name) | Type::Enum(name) | Type::Class(name) => name,
                                _ => return None,
                            };
                            let template_name = l.generic_template_name(name)?;
                            let owner_args = l.generic_owner_args_for_expr(
                                template_name,
                                &receiver_ty,
                                object,
                                resolve,
                            );
                            (
                                fn_key(Some(template_name), field),
                                Some(receiver_ty),
                                owner_args,
                            )
                        }
                    } else {
                        let receiver_ty = infer_or_checked_with(l, resolve, object)?;
                        let name = match &receiver_ty {
                            Type::Struct(name) | Type::Enum(name) | Type::Class(name) => name,
                            _ => return None,
                        };
                        let template_name = l.generic_template_name(name)?;
                        let owner_args = l.generic_owner_args_for_expr(
                            template_name,
                            &receiver_ty,
                            object,
                            resolve,
                        );
                        (
                            fn_key(Some(template_name), field),
                            Some(receiver_ty),
                            owner_args,
                        )
                    }
                }
                _ => return None,
            };
            let index = *l.generic_index.get(&function)?;
            let bindings = l
                .infer_type_args(
                    &function,
                    &arg_types,
                    &explicit,
                    receiver_ty.as_ref(),
                    receiver_args.as_deref(),
                )?
                .into_iter()
                .zip(l.generic_fns[index].type_params.iter())
                .map(|(ty, name)| (name.clone(), ty))
                .collect::<HashMap<_, _>>();
            let return_type = l.generic_fns[index].return_type.as_ref()?;
            let lirac::ast::TypeExprKind::Generic { name, args } = &return_type.kind else {
                return None;
            };
            if name != template_name || args.len() != template.type_params.len() {
                return None;
            }
            args.iter()
                .map(|arg| bound_type_of_ann(l, arg, &bindings))
                .collect::<Vec<_>>()
        }
        _ => return None,
    };
    let (_, annotation) = template.fields.iter().find(|(name, _)| name == field)?;
    let bindings = template
        .type_params
        .iter()
        .cloned()
        .zip(arguments)
        .collect::<HashMap<_, _>>();
    Some(bound_type_of_ann(l, annotation, &bindings))
}

/// The instantiated name of a generic enum variant, from the payload it was
/// given. `None` when the enum is not generic.
fn generic_variant_type(
    l: &Lowerer<'_>,
    resolve: &dyn Fn(&str) -> Option<Type>,
    enum_name: &str,
    variant_name: &str,
    args: &[Argument],
) -> Option<Type> {
    let template = l.layouts.generics.get(enum_name)?;
    let in_scope: HashSet<String> = template.type_params.iter().cloned().collect();
    let mut bindings = HashMap::new();
    if let Some((_, payloads)) = template
        .variants
        .iter()
        .find(|(name, _)| name == variant_name)
    {
        for (declared, arg) in payloads.iter().zip(args) {
            unify_declared_expression(l, resolve, declared, &arg.value, &in_scope, &mut bindings)?;
        }
    }
    let arguments: Vec<Type> = template
        .type_params
        .iter()
        .map(|param| bindings.get(param).cloned().unwrap_or(Type::Int))
        .collect();
    Some(Type::Enum(mangle(enum_name, &arguments)))
}

/// Match a declared type against an actual one, recording what each type
/// parameter must be.
///
/// This is the whole of type-argument inference: the checker erases generics
/// and records no instantiations, so `identity(42)` only says `T = int` by
/// lining the declaration up against the argument.
fn unify(declared: &Type, actual: &Type, bindings: &mut HashMap<String, Type>) {
    match (declared, actual) {
        (Type::TypeParam(name), concrete) => {
            // First binding wins; a later conflict is the checker's business.
            bindings
                .entry(name.clone())
                .or_insert_with(|| concrete.clone());
        }
        (Type::Array(want), Type::Array(got)) => unify(want, got, bindings),
        (Type::Optional(want), Type::Optional(got)) => unify(want, got, bindings),
        (Type::Map(wk, wv), Type::Map(gk, gv)) => {
            unify(wk, gk, bindings);
            unify(wv, gv, bindings);
        }
        (Type::Tuple(want), Type::Tuple(got)) if want.len() == got.len() => {
            for (w, g) in want.iter().zip(got) {
                unify(w, g, bindings);
            }
        }
        (
            Type::Function {
                params: wp,
                return_type: wr,
                ..
            },
            Type::Function {
                params: gp,
                return_type: gr,
                ..
            },
        ) => {
            for (w, g) in wp.iter().zip(gp) {
                unify(w, g, bindings);
            }
            unify(wr, gr, bindings);
        }
        (
            Type::Result {
                ok_type: wo,
                err_type: we,
            },
            Type::Result {
                ok_type: go,
                err_type: ge,
            },
        ) => {
            unify(wo, go, bindings);
            unify(we, ge, bindings);
        }
        _ => {}
    }
}

impl<'a> Lowerer<'a> {
    /// The declared parameter and return types of a generic template, still
    /// carrying their type parameters.
    fn template_signature(&self, key: &str) -> Option<(Vec<Type>, Type, Vec<String>)> {
        let index = *self.generic_index.get(key)?;
        let template = &self.generic_fns[index];
        let in_scope: HashSet<String> = template.type_params.iter().cloned().collect();
        let params = template
            .params
            .iter()
            .filter(|p| !is_receiver(&p.name))
            .map(|p| layout::type_of_ann_in(&p.type_ann, &in_scope))
            .collect();
        let ret = template
            .return_type
            .as_ref()
            .map(|t| layout::type_of_ann_in(t, &in_scope))
            .unwrap_or(Type::Any);
        Some((params, ret, template.type_params.clone()))
    }

    /// Work out a generic call's type arguments from the types it is given.
    fn infer_type_args(
        &self,
        key: &str,
        arg_types: &[Type],
        explicit: &[Type],
        receiver_ty: Option<&Type>,
        receiver_args: Option<&[Type]>,
    ) -> Option<Vec<Type>> {
        let (params, _, type_params) = self.template_signature(key)?;
        let template = &self.generic_fns[*self.generic_index.get(key)?];
        let owner_count = template.owner_param_count;
        let method_count = type_params.len().saturating_sub(owner_count);
        let owner_args = receiver_args.map(|args| args.to_vec()).or_else(|| {
            template
                .owner
                .as_deref()
                .and_then(|owner| receiver_ty.and_then(|ty| self.generic_owner_args(owner, ty)))
        });

        if !explicit.is_empty() {
            if owner_count == 0 {
                return (explicit.len() == type_params.len()).then(|| explicit.to_vec());
            }
            if explicit.len() == type_params.len() {
                // Accept the fully-qualified form as well as the method-only
                // form. This is useful for static calls where no receiver can
                // provide the owner arguments.
                return Some(explicit.to_vec());
            }
            if explicit.len() != method_count {
                return None;
            }
            let owner_args = owner_args?;
            let mut args = explicit.to_vec();
            args.extend(owner_args);
            return Some(args);
        }

        let mut bindings = HashMap::new();
        if owner_count > 0 {
            let owner_args = owner_args?;
            for (name, ty) in type_params.iter().skip(method_count).zip(owner_args.iter()) {
                bindings.insert(name.clone(), ty.clone());
            }
        }
        for (declared, actual) in params.iter().zip(arg_types) {
            unify(declared, actual, &mut bindings);
        }
        type_params
            .iter()
            .map(|name| bindings.get(name).cloned())
            .collect()
    }

    /// The result type of a generic call, once its arguments are known.
    fn generic_call_type(
        &self,
        key: &str,
        arg_types: &[Type],
        explicit: &[Type],
        receiver_ty: Option<&Type>,
        receiver_args: Option<&[Type]>,
    ) -> Option<Type> {
        let args = self.infer_type_args(key, arg_types, explicit, receiver_ty, receiver_args)?;
        let template = &self.generic_fns[*self.generic_index.get(key)?];
        let bindings: HashMap<String, Type> =
            template.type_params.iter().cloned().zip(args).collect();
        Some(match &template.return_type {
            Some(ret) => bound_type_of_ann(self, ret, &bindings),
            None => Type::Any,
        })
    }

    /// Recover the concrete arguments carried by a generic aggregate receiver.
    /// The explicit instance map handles normal lowering; matching the concrete
    /// field layout is the immutable fallback used while inferring a nested call
    /// before its receiver expression has been emitted.
    fn generic_owner_args(&self, owner: &str, receiver_ty: &Type) -> Option<Vec<Type>> {
        let name = match receiver_ty {
            Type::Struct(name) | Type::Enum(name) | Type::Class(name) => name,
            _ => return None,
        };
        if let Some((template, args)) = self.type_instances.get(name) {
            if template == owner {
                return Some(args.clone());
            }
        }
        let template = self.layouts.generics.get(owner)?;
        let layout = self.layouts.structs.get(name)?;
        if template.fields.is_empty() {
            return None;
        }
        let in_scope: HashSet<String> = template.type_params.iter().cloned().collect();
        let mut bindings = HashMap::new();
        for (field_name, declared) in &template.fields {
            let actual = layout.field(field_name)?.ty.clone();
            unify(
                &layout::type_of_ann_in(declared, &in_scope),
                &actual,
                &mut bindings,
            );
        }
        template
            .type_params
            .iter()
            .map(|param| bindings.get(param).cloned())
            .collect()
    }

    fn generic_owner_args_for_expr(
        &self,
        owner: &str,
        receiver_ty: &Type,
        receiver: &Expression,
        resolve: &dyn Fn(&str) -> Option<Type>,
    ) -> Option<Vec<Type>> {
        self.generic_owner_args(owner, receiver_ty).or_else(|| {
            let ExpressionKind::StructLiteral {
                name: Some(name),
                fields,
            } = &receiver.kind
            else {
                if let ExpressionKind::Call {
                    callee,
                    type_args,
                    args,
                } = &receiver.kind
                {
                    let ExpressionKind::Identifier(function) = &callee.kind else {
                        return None;
                    };
                    let index = *self.generic_index.get(function)?;
                    let arg_types = args
                        .iter()
                        .map(|arg| infer_or_checked_with(self, resolve, &arg.value))
                        .collect::<Option<Vec<_>>>()?;
                    let explicit = type_args
                        .iter()
                        .map(|arg| self.normalize(layout::type_of_ann(arg)))
                        .collect::<Vec<_>>();
                    let inferred =
                        self.infer_type_args(function, &arg_types, &explicit, None, None)?;
                    let bindings = self.generic_fns[index]
                        .type_params
                        .iter()
                        .cloned()
                        .zip(inferred)
                        .collect::<HashMap<_, _>>();
                    let return_type = self.generic_fns[index].return_type.as_ref()?;
                    let lirac::ast::TypeExprKind::Generic { name, args } = &return_type.kind else {
                        return None;
                    };
                    if name != owner
                        || args.len() != self.layouts.generics.get(owner)?.type_params.len()
                    {
                        return None;
                    }
                    return Some(
                        args.iter()
                            .map(|arg| bound_type_of_ann(self, arg, &bindings))
                            .collect(),
                    );
                }
                return None;
            };
            (name == owner).then(|| generic_literal_args(self, resolve, name, fields))?
        })
    }
}

/// Resolve a type annotation under concrete generic bindings without mutating
/// the layout map. This is used while deciding the type of a call, before the
/// actual lowering has instantiated its returned aggregate.
fn bound_type_of_ann(
    l: &Lowerer<'_>,
    ann: &lirac::ast::TypeExpr,
    bindings: &HashMap<String, Type>,
) -> Type {
    use lirac::ast::TypeExprKind;

    let recur = |inner: &lirac::ast::TypeExpr| bound_type_of_ann(l, inner, bindings);
    match &ann.kind {
        TypeExprKind::Named(name) => bindings
            .get(name)
            .cloned()
            .unwrap_or_else(|| l.normalize(layout::type_of_ann(ann))),
        TypeExprKind::Generic { name, args } if name == "Channel" && args.len() == 1 => {
            Type::Channel(Box::new(recur(&args[0])))
        }
        TypeExprKind::Generic { name, args } if l.layouts.generics.contains_key(name) => {
            let args: Vec<Type> = args.iter().map(recur).collect();
            let mangled = mangle(name, &args);
            if l.layouts.generics[name].variants.is_empty() {
                Type::Struct(mangled)
            } else {
                Type::Enum(mangled)
            }
        }
        TypeExprKind::Generic { .. } => {
            let in_scope: HashSet<String> = bindings.keys().cloned().collect();
            l.normalize(substitute(
                &layout::type_of_ann_in(ann, &in_scope),
                bindings,
            ))
        }
        TypeExprKind::Optional(inner) => Type::Optional(Box::new(recur(inner))),
        TypeExprKind::Function {
            params,
            return_type,
        } => Type::Function {
            params: params.iter().map(recur).collect(),
            return_type: Box::new(recur(return_type)),
            required_params: params.len(),
        },
        TypeExprKind::Tuple(items) => Type::Tuple(items.iter().map(recur).collect()),
        TypeExprKind::Array(inner) => Type::Array(Box::new(recur(inner))),
        TypeExprKind::Result { ok_type, err_type } => Type::Result {
            ok_type: Box::new(recur(ok_type)),
            err_type: Box::new(recur(err_type)),
        },
        TypeExprKind::Path(_) | TypeExprKind::Infer => l.normalize(layout::type_of_ann(ann)),
    }
}

impl<'a, 'b, 'c> FuncGen<'a, 'b, 'c> {
    /// Lower a call to a generic function or method, instantiating it first.
    ///
    /// Returns `Ok(None)` when `key` names no template, so the caller can carry
    /// on with ordinary dispatch.
    fn lower_generic_call(
        &mut self,
        key: &str,
        self_value: Option<Value>,
        receiver_ty: Option<&Type>,
        args: &[Argument],
        type_args: &[Type],
        span: &Span,
    ) -> CodegenResult<Option<Option<Value>>> {
        if !self.l.generic_index.contains_key(key) {
            return Ok(None);
        }
        let arg_types: Vec<Type> = args
            .iter()
            .map(|arg| self.ty_of(&arg.value))
            .collect::<CodegenResult<_>>()?;
        let inferred = self
            .l
            .infer_type_args(key, &arg_types, type_args, receiver_ty, None)
            .ok_or_else(|| {
                CodegenError::unsupported_at(
                    format!(
                        "cannot work out the type arguments for `{}` from this call; \
                         write them explicitly",
                        key
                    ),
                    span,
                )
            })?;
        let instance = self.l.instantiate_fn(key, &inferred, span)?;
        self.lower_user_call(&instance, self_value, args, span)
            .map(Some)
    }
}
