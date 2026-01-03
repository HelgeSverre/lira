/**
 * Lira AST Type Definitions
 * Mirrors crates/lirac/src/ast.rs
 */

/** Source location information */
export interface Span {
  line: number;
  column: number;
}

/** Generic type parameter */
export interface TypeParam {
  name: string;
  bounds: string[];
}

/** A complete Lira program */
export interface Program {
  statements: Statement[];
}

/** Top-level and block statements */
export interface Statement {
  kind: StatementKind;
  span: Span;
}

/**
 * StatementKind types.
 *
 * Note: Some Rust variants are tuple variants (e.g., `Expression(Expression)`).
 * With serde's `tag = "type", content = "value"` and our normalizeAst() function,
 * tuple variants containing objects have their fields spread directly:
 * - Expression(expr) → {type: "Expression", kind: ExpressionKind, span: Span}
 * - Return(Some(expr)) → {type: "Return", kind: ExpressionKind, span: Span}
 * - Return(None) → {type: "Return", value: null}
 */
export type StatementKind =
  | { type: 'VarDecl'; pattern: Pattern; mutable: boolean; typeAnn: TypeExpr | null; initializer: Expression | null }
  | { type: 'ConstDecl'; name: string; typeAnn: TypeExpr | null; initializer: Expression }
  | { type: 'FnDecl'; name: string; typeParams: TypeParam[]; params: Parameter[]; returnType: TypeExpr | null; body: Block; isPublic: boolean; isOverride: boolean }
  | { type: 'ClassDecl'; name: string; parent: string | null; interfaces: string[]; fields: Field[]; methods: Statement[] }
  | { type: 'StructDecl'; name: string; typeParams: TypeParam[]; fields: Field[]; methods: Statement[] }
  | { type: 'EnumDecl'; name: string; variants: EnumVariant[] }
  | { type: 'InterfaceDecl'; name: string; methods: InterfaceMethod[] }
  | { type: 'TypeAlias'; name: string; typeExpr: TypeExpr }
  // Tuple variant: inner Expression fields are spread (kind, span)
  | { type: 'Expression'; kind: ExpressionKind; span: Span }
  // Tuple variant with Option: Some case has spread fields, None case has value: null
  | { type: 'Return'; kind?: ExpressionKind; span?: Span; value?: null }
  | { type: 'If'; condition: Expression; thenBranch: Block; elseBranch: Block | null }
  | { type: 'While'; condition: Expression; body: Block }
  | { type: 'For'; variable: string; iterable: Expression; body: Block }
  | { type: 'Loop'; body: Block }
  // Tuple variant with Option: Some case has spread fields, None case has value: null
  | { type: 'Break'; kind?: ExpressionKind; span?: Span; value?: null }
  | { type: 'Continue' }
  | { type: 'Import'; path: string[]; items: string[] | null }
  | { type: 'Use'; path: string[]; alias: string | null; items: string[] | null }
  | { type: 'TraitDecl'; name: string; typeParams: TypeParam[]; methods: TraitMethod[]; isPublic: boolean }
  | { type: 'ImplDecl'; traitName: string | null; typeName: string; typeParams: TypeParam[]; methods: Statement[] }
  | { type: 'Block'; block: Block };

/** A block of statements */
export interface Block {
  statements: Statement[];
  span: Span;
}

/** Function parameter */
export interface Parameter {
  name: string;
  typeAnn: TypeExpr;
  default: Expression | null;
  span: Span;
}

/** Struct/class field */
export interface Field {
  name: string;
  typeAnn: TypeExpr;
  isPublic: boolean;
  isMutable: boolean;
  span: Span;
}

/** Enum variant */
export interface EnumVariant {
  name: string;
  fields: TypeExpr[];
  span: Span;
}

/** Interface method signature */
export interface InterfaceMethod {
  name: string;
  params: Parameter[];
  returnType: TypeExpr | null;
  span: Span;
}

/** Trait method signature */
export interface TraitMethod {
  name: string;
  params: Parameter[];
  returnType: TypeExpr | null;
  hasSelf: boolean;
  defaultImpl: Block | null;
  span: Span;
}

/** Type expression */
export interface TypeExpr {
  kind: TypeExprKind;
  span: Span;
}

export type TypeExprKind =
  | { type: 'Named'; name: string }
  | { type: 'Generic'; name: string; args: TypeExpr[] }
  | { type: 'Optional'; inner: TypeExpr }
  | { type: 'Function'; params: TypeExpr[]; returnType: TypeExpr }
  | { type: 'Tuple'; elements: TypeExpr[] }
  | { type: 'Array'; element: TypeExpr }
  | { type: 'Result'; okType: TypeExpr; errType: TypeExpr }
  | { type: 'Path'; segments: string[] };

/** Expressions */
export interface Expression {
  kind: ExpressionKind;
  span: Span;
}

export type ExpressionKind =
  | { type: 'IntLiteral'; value: number }
  | { type: 'FloatLiteral'; value: number }
  | { type: 'StringLiteral'; value: string }
  | { type: 'CharLiteral'; value: string }
  | { type: 'BoolLiteral'; value: boolean }
  | { type: 'Null' }
  | { type: 'Identifier'; name: string }
  | { type: 'Binary'; left: Expression; op: BinaryOp; right: Expression }
  | { type: 'Unary'; op: UnaryOp; operand: Expression }
  | { type: 'Call'; callee: Expression; typeArgs: TypeExpr[]; args: Argument[] }
  | { type: 'FieldAccess'; object: Expression; field: string }
  | { type: 'OptionalAccess'; object: Expression; field: string }
  | { type: 'Index'; object: Expression; index: Expression }
  | { type: 'Array'; elements: Expression[] }
  | { type: 'Map'; entries: [Expression, Expression][] }
  | { type: 'Tuple'; elements: Expression[] }
  | { type: 'StructLiteral'; name: string | null; fields: [string, Expression][] }
  | { type: 'Lambda'; params: Parameter[]; body: Expression }
  | { type: 'IfExpr'; condition: Expression; thenExpr: Expression; elseExpr: Expression }
  | { type: 'Match'; subject: Expression; arms: MatchArm[] }
  | { type: 'Range'; start: Expression | null; end: Expression | null; inclusive: boolean }
  | { type: 'Cast'; expr: Expression; typeExpr: TypeExpr }
  | { type: 'TypeCheck'; expr: Expression; typeExpr: TypeExpr }
  | { type: 'Assign'; target: Expression; value: Expression }
  | { type: 'CompoundAssign'; target: Expression; op: BinaryOp; value: Expression }
  | { type: 'Block'; block: Block }
  | { type: 'Spawn'; expr: Expression }
  | { type: 'Select'; arms: SelectArm[] }
  | { type: 'EnumVariant'; enumName: string; variantName: string }
  | { type: 'Path'; segments: string[] }
  | { type: 'Try'; expr: Expression }
  | { type: 'MethodCall'; receiver: Expression; method: string; typeArgs: TypeExpr[]; args: Argument[] };

/** Binary operators */
export type BinaryOp =
  | 'Add' | 'Sub' | 'Mul' | 'Div' | 'Mod' | 'Pow'
  | 'Eq' | 'Ne' | 'Lt' | 'Le' | 'Gt' | 'Ge'
  | 'And' | 'Or'
  | 'BitAnd' | 'BitOr' | 'BitXor' | 'Shl' | 'Shr' | 'UShr'
  | 'NullCoalesce';

/** Unary operators */
export type UnaryOp = 'Neg' | 'Not' | 'BitNot' | 'PreInc' | 'PreDec' | 'PostInc' | 'PostDec';

/** Match arm */
export interface MatchArm {
  pattern: Pattern;
  guard: Expression | null;
  body: Expression;
  span: Span;
}

/** Function call argument */
export interface Argument {
  name: string | null;
  value: Expression;
  span: Span;
}

/** Select arm */
export interface SelectArm {
  kind: SelectArmKind;
  body: Expression;
  span: Span;
}

export type SelectArmKind =
  | { type: 'Recv'; variable: string | null; channel: Expression }
  | { type: 'Send'; value: Expression; channel: Expression }
  | { type: 'Default' };

/** Pattern for match expressions */
export interface Pattern {
  kind: PatternKind;
  span: Span;
}

export type PatternKind =
  | { type: 'Wildcard' }
  | { type: 'Literal'; expr: Expression }
  | { type: 'Variable'; name: string }
  | { type: 'Tuple'; elements: Pattern[] }
  | { type: 'Struct'; name: string; fields: [string, Pattern][]; rest: boolean }
  | { type: 'Constructor'; name: string; fields: Pattern[] }
  | { type: 'Range'; start: Pattern; end: Pattern; inclusive: boolean }
  | { type: 'Or'; patterns: Pattern[] }
  | { type: 'Binding'; name: string; pattern: Pattern };

/** Helper to get display name for statement kind */
export function getStatementKindName(kind: StatementKind): string {
  return kind.type;
}

/** Helper to get display name for expression kind */
export function getExpressionKindName(kind: ExpressionKind): string {
  return kind.type;
}

/** Helper to format binary operator */
export function formatBinaryOp(op: BinaryOp): string {
  const ops: Record<BinaryOp, string> = {
    Add: '+', Sub: '-', Mul: '*', Div: '/', Mod: '%', Pow: '**',
    Eq: '==', Ne: '!=', Lt: '<', Le: '<=', Gt: '>', Ge: '>=',
    And: '&&', Or: '||',
    BitAnd: '&', BitOr: '|', BitXor: '^', Shl: '<<', Shr: '>>', UShr: '>>>',
    NullCoalesce: '??',
  };
  return ops[op];
}

/** Helper to format unary operator */
export function formatUnaryOp(op: UnaryOp): string {
  const ops: Record<UnaryOp, string> = {
    Neg: '-', Not: '!', BitNot: '~',
    PreInc: '++', PreDec: '--', PostInc: '++', PostDec: '--',
  };
  return ops[op];
}
