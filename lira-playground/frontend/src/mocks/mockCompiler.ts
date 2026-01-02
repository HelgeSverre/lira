/**
 * Mock Lira Compiler
 *
 * Generates a plausible AST from Lira source code using regex-based parsing.
 * This is for UI development only - not a real compiler.
 */

import type {
  Program,
  Statement,
  StatementKind,
  Expression,
  Block,
  Pattern,
  TypeExpr,
  Parameter,
  Span,
} from '../types/ast';
import type { CompileError } from '../types/protocol';

interface CompileResult {
  ast: Program;
  errors: CompileError[];
}

export function mockCompile(source: string): CompileResult {
  const lines = source.split('\n');
  const statements: Statement[] = [];
  const errors: CompileError[] = [];

  let lineNum = 0;
  while (lineNum < lines.length) {
    const line = lines[lineNum];
    const trimmed = line.trim();

    // Skip empty lines and comments
    if (!trimmed || trimmed.startsWith('//')) {
      lineNum++;
      continue;
    }

    try {
      const result = parseLine(trimmed, lineNum + 1, lines, lineNum);
      if (result.statement) {
        statements.push(result.statement);
      }
      lineNum = result.nextLine;
    } catch (e) {
      errors.push({
        message: e instanceof Error ? e.message : String(e),
        line: lineNum + 1,
        column: 1,
        severity: 'error',
      });
      lineNum++;
    }
  }

  return {
    ast: { statements },
    errors,
  };
}

interface ParseResult {
  statement: Statement | null;
  nextLine: number;
}

function parseLine(trimmed: string, line: number, lines: string[], currentIdx: number): ParseResult {
  const span: Span = { line, column: 1 };

  // Function declaration
  const fnMatch = trimmed.match(/^(pub\s+)?fn\s+(\w+)\s*\(([^)]*)\)(\s*->\s*(\w+))?\s*\{?$/);
  if (fnMatch) {
    const isPublic = !!fnMatch[1];
    const name = fnMatch[2];
    const paramsStr = fnMatch[3];
    const returnType = fnMatch[5] || null;

    const params = parseParams(paramsStr, line);
    const bodyResult = parseBlock(lines, currentIdx + 1);

    const kind: StatementKind = {
      type: 'FnDecl',
      name,
      typeParams: [],
      params,
      returnType: returnType ? makeNamedType(returnType, span) : null,
      body: bodyResult.block,
      isPublic,
      isOverride: false,
    };

    return {
      statement: { kind, span },
      nextLine: bodyResult.nextLine,
    };
  }

  // Variable declaration (let/var)
  const varMatch = trimmed.match(/^(let|var)\s+(\w+)(\s*:\s*(\w+))?\s*=\s*(.+)$/);
  if (varMatch) {
    const mutable = varMatch[1] === 'var';
    const name = varMatch[2];
    const typeAnn = varMatch[4] || null;
    const initExpr = varMatch[5];

    const kind: StatementKind = {
      type: 'VarDecl',
      pattern: makeVariablePattern(name, span),
      mutable,
      typeAnn: typeAnn ? makeNamedType(typeAnn, span) : null,
      initializer: parseExpression(initExpr, line),
    };

    return {
      statement: { kind, span },
      nextLine: currentIdx + 1,
    };
  }

  // Const declaration
  const constMatch = trimmed.match(/^const\s+(\w+)(\s*:\s*(\w+))?\s*=\s*(.+)$/);
  if (constMatch) {
    const name = constMatch[1];
    const typeAnn = constMatch[3] || null;
    const initExpr = constMatch[4];

    const kind: StatementKind = {
      type: 'ConstDecl',
      name,
      typeAnn: typeAnn ? makeNamedType(typeAnn, span) : null,
      initializer: parseExpression(initExpr, line),
    };

    return {
      statement: { kind, span },
      nextLine: currentIdx + 1,
    };
  }

  // Struct declaration
  const structMatch = trimmed.match(/^struct\s+(\w+)\s*\{?$/);
  if (structMatch) {
    const name = structMatch[1];
    const bodyResult = parseStructBody(lines, currentIdx + 1);

    const kind: StatementKind = {
      type: 'StructDecl',
      name,
      typeParams: [],
      fields: bodyResult.fields,
      methods: [],
    };

    return {
      statement: { kind, span },
      nextLine: bodyResult.nextLine,
    };
  }

  // Enum declaration
  const enumMatch = trimmed.match(/^enum\s+(\w+)\s*\{?$/);
  if (enumMatch) {
    const name = enumMatch[1];
    const bodyResult = parseEnumBody(lines, currentIdx + 1);

    const kind: StatementKind = {
      type: 'EnumDecl',
      name,
      variants: bodyResult.variants,
    };

    return {
      statement: { kind, span },
      nextLine: bodyResult.nextLine,
    };
  }

  // Impl block
  const implMatch = trimmed.match(/^impl\s+(\w+)(\s+for\s+(\w+))?\s*\{?$/);
  if (implMatch) {
    const first = implMatch[1];
    const second = implMatch[3];
    const traitName = second ? first : null;
    const typeName = second || first;
    const bodyResult = parseBlock(lines, currentIdx + 1);

    const kind: StatementKind = {
      type: 'ImplDecl',
      traitName,
      typeName,
      typeParams: [],
      methods: bodyResult.block.statements,
    };

    return {
      statement: { kind, span },
      nextLine: bodyResult.nextLine,
    };
  }

  // If statement
  const ifMatch = trimmed.match(/^if\s+(.+)\s*\{?$/);
  if (ifMatch) {
    const condition = parseExpression(ifMatch[1], line);
    const thenResult = parseBlock(lines, currentIdx + 1);

    // Check for else
    let elseBranch: Block | null = null;
    let nextLine = thenResult.nextLine;

    if (nextLine < lines.length) {
      const elseLine = lines[nextLine].trim();
      if (elseLine.startsWith('else') || elseLine === '} else {') {
        const elseResult = parseBlock(lines, nextLine + 1);
        elseBranch = elseResult.block;
        nextLine = elseResult.nextLine;
      }
    }

    const kind: StatementKind = {
      type: 'If',
      condition,
      thenBranch: thenResult.block,
      elseBranch,
    };

    return {
      statement: { kind, span },
      nextLine,
    };
  }

  // While loop
  const whileMatch = trimmed.match(/^while\s+(.+)\s*\{?$/);
  if (whileMatch) {
    const condition = parseExpression(whileMatch[1], line);
    const bodyResult = parseBlock(lines, currentIdx + 1);

    const kind: StatementKind = {
      type: 'While',
      condition,
      body: bodyResult.block,
    };

    return {
      statement: { kind, span },
      nextLine: bodyResult.nextLine,
    };
  }

  // For loop
  const forMatch = trimmed.match(/^for\s+(\w+)\s+in\s+(.+)\s*\{?$/);
  if (forMatch) {
    const variable = forMatch[1];
    const iterable = parseExpression(forMatch[2], line);
    const bodyResult = parseBlock(lines, currentIdx + 1);

    const kind: StatementKind = {
      type: 'For',
      variable,
      iterable,
      body: bodyResult.block,
    };

    return {
      statement: { kind, span },
      nextLine: bodyResult.nextLine,
    };
  }

  // Loop
  if (trimmed === 'loop {' || trimmed === 'loop') {
    const bodyResult = parseBlock(lines, currentIdx + 1);

    const kind: StatementKind = {
      type: 'Loop',
      body: bodyResult.block,
    };

    return {
      statement: { kind, span },
      nextLine: bodyResult.nextLine,
    };
  }

  // Return statement
  const returnMatch = trimmed.match(/^return\s*(.*)$/);
  if (returnMatch) {
    const valueExpr = returnMatch[1].trim();
    const kind: StatementKind = {
      type: 'Return',
      value: valueExpr ? parseExpression(valueExpr, line) : null,
    };

    return {
      statement: { kind, span },
      nextLine: currentIdx + 1,
    };
  }

  // Break
  if (trimmed === 'break' || trimmed.startsWith('break ')) {
    const kind: StatementKind = {
      type: 'Break',
      value: null,
    };
    return { statement: { kind, span }, nextLine: currentIdx + 1 };
  }

  // Continue
  if (trimmed === 'continue') {
    const kind: StatementKind = { type: 'Continue' };
    return { statement: { kind, span }, nextLine: currentIdx + 1 };
  }

  // Import
  const importMatch = trimmed.match(/^import\s+(.+)$/);
  if (importMatch) {
    const path = importMatch[1].split('.');
    const kind: StatementKind = {
      type: 'Import',
      path,
      items: null,
    };
    return { statement: { kind, span }, nextLine: currentIdx + 1 };
  }

  // Skip closing braces
  if (trimmed === '}') {
    return { statement: null, nextLine: currentIdx + 1 };
  }

  // Expression statement (fallback)
  const expr = parseExpression(trimmed, line);
  const kind: StatementKind = {
    type: 'Expression',
    expr,
  };

  return {
    statement: { kind, span },
    nextLine: currentIdx + 1,
  };
}

function parseBlock(lines: string[], startIdx: number): { block: Block; nextLine: number } {
  const statements: Statement[] = [];
  let idx = startIdx;
  let braceDepth = 1;

  while (idx < lines.length && braceDepth > 0) {
    const line = lines[idx];
    const trimmed = line.trim();

    // Count braces
    for (const char of trimmed) {
      if (char === '{') braceDepth++;
      if (char === '}') braceDepth--;
    }

    if (braceDepth === 0) {
      idx++;
      break;
    }

    if (trimmed && !trimmed.startsWith('//')) {
      try {
        const result = parseLine(trimmed, idx + 1, lines, idx);
        if (result.statement) {
          statements.push(result.statement);
        }
        idx = result.nextLine;
      } catch {
        idx++;
      }
    } else {
      idx++;
    }
  }

  return {
    block: {
      statements,
      span: { line: startIdx + 1, column: 1 },
    },
    nextLine: idx,
  };
}

function parseParams(paramsStr: string, line: number): Parameter[] {
  if (!paramsStr.trim()) return [];

  return paramsStr.split(',').map((p, i) => {
    const parts = p.trim().split(':');
    const name = parts[0]?.trim() || `arg${i}`;
    const typeName = parts[1]?.trim() || 'any';

    return {
      name,
      typeAnn: makeNamedType(typeName, { line, column: 1 }),
      default: null,
      span: { line, column: 1 },
    };
  });
}

function parseStructBody(lines: string[], startIdx: number): { fields: any[]; nextLine: number } {
  const fields: any[] = [];
  let idx = startIdx;

  while (idx < lines.length) {
    const trimmed = lines[idx].trim();
    if (trimmed === '}') {
      idx++;
      break;
    }

    const fieldMatch = trimmed.match(/^(\w+)\s*:\s*(\w+),?$/);
    if (fieldMatch) {
      fields.push({
        name: fieldMatch[1],
        typeAnn: makeNamedType(fieldMatch[2], { line: idx + 1, column: 1 }),
        isPublic: false,
        isMutable: true,
        span: { line: idx + 1, column: 1 },
      });
    }
    idx++;
  }

  return { fields, nextLine: idx };
}

function parseEnumBody(lines: string[], startIdx: number): { variants: any[]; nextLine: number } {
  const variants: any[] = [];
  let idx = startIdx;

  while (idx < lines.length) {
    const trimmed = lines[idx].trim();
    if (trimmed === '}') {
      idx++;
      break;
    }

    const variantMatch = trimmed.match(/^(\w+)(\(([^)]*)\))?,?$/);
    if (variantMatch) {
      const fields = variantMatch[3]
        ? variantMatch[3].split(',').map((t) => makeNamedType(t.trim(), { line: idx + 1, column: 1 }))
        : [];

      variants.push({
        name: variantMatch[1],
        fields,
        span: { line: idx + 1, column: 1 },
      });
    }
    idx++;
  }

  return { variants, nextLine: idx };
}

function parseExpression(exprStr: string, line: number): Expression {
  const span: Span = { line, column: 1 };
  const trimmed = exprStr.trim();

  // Integer literal
  if (/^-?\d+$/.test(trimmed)) {
    return {
      kind: { type: 'IntLiteral', value: parseInt(trimmed, 10) },
      span,
    };
  }

  // Float literal
  if (/^-?\d+\.\d+$/.test(trimmed)) {
    return {
      kind: { type: 'FloatLiteral', value: parseFloat(trimmed) },
      span,
    };
  }

  // Boolean literal
  if (trimmed === 'true' || trimmed === 'false') {
    return {
      kind: { type: 'BoolLiteral', value: trimmed === 'true' },
      span,
    };
  }

  // Null literal
  if (trimmed === 'null') {
    return {
      kind: { type: 'Null' },
      span,
    };
  }

  // String literal
  if (/^"[^"]*"$/.test(trimmed)) {
    return {
      kind: { type: 'StringLiteral', value: trimmed.slice(1, -1) },
      span,
    };
  }

  // Character literal
  if (/^'.'$/.test(trimmed)) {
    return {
      kind: { type: 'CharLiteral', value: trimmed.slice(1, -1) },
      span,
    };
  }

  // Spawn expression
  if (trimmed.startsWith('spawn ')) {
    return {
      kind: {
        type: 'Spawn',
        expr: parseExpression(trimmed.slice(6), line),
      },
      span,
    };
  }

  // Enum variant (EnumName::Variant)
  const enumMatch = trimmed.match(/^(\w+)::(\w+)$/);
  if (enumMatch) {
    return {
      kind: {
        type: 'EnumVariant',
        enumName: enumMatch[1],
        variantName: enumMatch[2],
      },
      span,
    };
  }

  // Function call
  const callMatch = trimmed.match(/^(\w+)\s*\(([^)]*)\)$/);
  if (callMatch) {
    const callee: Expression = {
      kind: { type: 'Identifier', name: callMatch[1] },
      span,
    };
    const argsStr = callMatch[2];
    const args = argsStr
      ? argsStr.split(',').map((a) => ({
          name: null,
          value: parseExpression(a.trim(), line),
          span,
        }))
      : [];

    return {
      kind: {
        type: 'Call',
        callee,
        typeArgs: [],
        args,
      },
      span,
    };
  }

  // Method call (simple)
  const methodMatch = trimmed.match(/^(\w+)\.(\w+)\s*\(([^)]*)\)$/);
  if (methodMatch) {
    const receiver = parseExpression(methodMatch[1], line);
    const argsStr = methodMatch[3];
    const args = argsStr
      ? argsStr.split(',').map((a) => ({
          name: null,
          value: parseExpression(a.trim(), line),
          span,
        }))
      : [];

    return {
      kind: {
        type: 'MethodCall',
        receiver,
        method: methodMatch[2],
        typeArgs: [],
        args,
      },
      span,
    };
  }

  // Field access
  const fieldMatch = trimmed.match(/^(\w+)\.(\w+)$/);
  if (fieldMatch) {
    return {
      kind: {
        type: 'FieldAccess',
        object: parseExpression(fieldMatch[1], line),
        field: fieldMatch[2],
      },
      span,
    };
  }

  // Binary operators (simple)
  for (const op of ['+', '-', '*', '/', '==', '!=', '<=', '>=', '<', '>', '&&', '||']) {
    const idx = trimmed.lastIndexOf(op);
    if (idx > 0 && idx < trimmed.length - op.length) {
      const left = trimmed.slice(0, idx).trim();
      const right = trimmed.slice(idx + op.length).trim();
      if (left && right) {
        return {
          kind: {
            type: 'Binary',
            left: parseExpression(left, line),
            op: opToBinaryOp(op),
            right: parseExpression(right, line),
          },
          span,
        };
      }
    }
  }

  // Array literal
  if (trimmed.startsWith('[') && trimmed.endsWith(']')) {
    const inner = trimmed.slice(1, -1);
    const elements = inner
      ? inner.split(',').map((e) => parseExpression(e.trim(), line))
      : [];
    return {
      kind: { type: 'Array', elements },
      span,
    };
  }

  // Range expression
  const rangeMatch = trimmed.match(/^(\d+)\.\.(=?)(\d+)$/);
  if (rangeMatch) {
    return {
      kind: {
        type: 'Range',
        start: parseExpression(rangeMatch[1], line),
        end: parseExpression(rangeMatch[3], line),
        inclusive: rangeMatch[2] === '=',
      },
      span,
    };
  }

  // Default: identifier
  if (/^\w+$/.test(trimmed)) {
    return {
      kind: { type: 'Identifier', name: trimmed },
      span,
    };
  }

  // Fallback
  return {
    kind: { type: 'Identifier', name: trimmed },
    span,
  };
}

function makeNamedType(name: string, span: Span): TypeExpr {
  return {
    kind: { type: 'Named', name },
    span,
  };
}

function makeVariablePattern(name: string, span: Span): Pattern {
  return {
    kind: { type: 'Variable', name },
    span,
  };
}

function opToBinaryOp(op: string): any {
  const map: Record<string, string> = {
    '+': 'Add',
    '-': 'Sub',
    '*': 'Mul',
    '/': 'Div',
    '%': 'Mod',
    '==': 'Eq',
    '!=': 'Ne',
    '<': 'Lt',
    '<=': 'Le',
    '>': 'Gt',
    '>=': 'Ge',
    '&&': 'And',
    '||': 'Or',
  };
  return map[op] || 'Add';
}
