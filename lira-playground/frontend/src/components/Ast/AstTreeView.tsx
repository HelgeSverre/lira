import type {
  Program,
  Statement,
  Expression,
  Block,
  TypeExpr,
  Pattern,
  Parameter,
  Argument,
  MatchArm,
  SelectArm,
  Field,
  EnumVariant as EnumVariantType,
  TypeParam,
} from '../../types/ast';
import { formatBinaryOp, formatUnaryOp } from '../../types/ast';
import { AstNode } from './AstNode';
import './AstTreeView.css';

interface AstTreeViewProps {
  program: Program;
}

export function AstTreeView({ program }: AstTreeViewProps) {
  return (
    <div className="ast-tree">
      <AstNode
        label="Program"
        nodeType="program"
        defaultExpanded
      >
        {program.statements.map((stmt, i) => (
          <StatementNode key={i} statement={stmt} />
        ))}
      </AstNode>
    </div>
  );
}

interface StatementNodeProps {
  statement: Statement;
}

function StatementNode({ statement }: StatementNodeProps) {
  // Defensive checks to prevent crashes
  if (!statement) {
    return null;
  }

  const { kind, span } = statement;

  if (!kind || typeof kind !== 'object') {
    return (
      <AstNode label="Unknown" nodeType="statement" span={span}>
        <span className="ast-error">Invalid statement</span>
      </AstNode>
    );
  }

  const renderContent = () => {
    try {
    switch (kind.type) {
      case 'VarDecl': {
        // Use type assertion for optional fields that may exist in different AST versions
        const varDecl = kind as typeof kind & { name?: string; value?: Expression };
        return (
          <>
            <span className="ast-keyword">{kind.mutable ? 'var' : 'let'}</span>
            {/* Handle different AST structures */}
            {kind.pattern && <PatternNode pattern={kind.pattern} />}
            {/* New structure: pattern might be directly a name */}
            {varDecl.name && <span className="ast-name">{varDecl.name}</span>}
            {kind.typeAnn && (
              <>
                <span className="ast-operator">: </span>
                <TypeExprNode type={kind.typeAnn} />
              </>
            )}
            {kind.initializer && (
              <>
                <span className="ast-operator"> = </span>
                <ExpressionNode expression={kind.initializer} label="init" />
              </>
            )}
            {/* Handle value field for simpler AST structure */}
            {varDecl.value && !kind.initializer && (
              <>
                <span className="ast-operator"> = </span>
                <ExpressionNode expression={varDecl.value} label="value" />
              </>
            )}
          </>
        );
      }

      case 'FnDecl':
        return (
          <>
            {kind.isPublic && <span className="ast-keyword">pub </span>}
            {kind.isOverride && <span className="ast-keyword">override </span>}
            <span className="ast-keyword">fn</span>
            <span className="ast-name">{kind.name}</span>
            {kind.typeParams && kind.typeParams.length > 0 && (
              <TypeParamsNode typeParams={kind.typeParams} />
            )}
            <ParameterListNode params={kind.params || []} />
            {kind.returnType && (
              <>
                <span className="ast-operator"> -&gt; </span>
                <TypeExprNode type={kind.returnType} />
              </>
            )}
            {kind.body && <BlockNode block={kind.body} label="body" />}
          </>
        );

      case 'StructDecl':
        return (
          <>
            <span className="ast-keyword">struct</span>
            <span className="ast-name">{kind.name}</span>
            {kind.typeParams && kind.typeParams.length > 0 && (
              <TypeParamsNode typeParams={kind.typeParams} />
            )}
            {kind.fields && kind.fields.length > 0 && (
              <AstNode label={`fields (${kind.fields.length})`} nodeType="fields">
                {kind.fields.map((field, i) => (
                  <FieldNode key={i} field={field} />
                ))}
              </AstNode>
            )}
            {kind.methods && kind.methods.length > 0 && (
              <AstNode label={`methods (${kind.methods.length})`} nodeType="methods">
                {kind.methods.map((method, i) => (
                  <StatementNode key={i} statement={method} />
                ))}
              </AstNode>
            )}
          </>
        );

      case 'EnumDecl':
        return (
          <>
            <span className="ast-keyword">enum</span>
            <span className="ast-name">{kind.name}</span>
            {kind.variants && kind.variants.length > 0 && (
              <AstNode label={`variants (${kind.variants.length})`} nodeType="variants">
                {kind.variants.map((variant, i) => (
                  <EnumVariantNode key={i} variant={variant} />
                ))}
              </AstNode>
            )}
          </>
        );

      case 'Expression':
        // Tuple variant: inner Expression fields (kind, span) are spread onto the StatementKind
        return <ExpressionNode expression={{ kind: kind.kind, span: kind.span }} />;

      case 'Return':
        // Tuple variant with Option: Some(expr) has spread fields, None has no expression
        return (
          <>
            <span className="ast-keyword">return</span>
            {kind.kind && <ExpressionNode expression={{ kind: kind.kind, span: kind.span! }} />}
          </>
        );

      case 'If':
        return (
          <>
            <span className="ast-keyword">if</span>
            <ExpressionNode expression={kind.condition} label="condition" />
            <BlockNode block={kind.thenBranch} label="then" />
            {kind.elseBranch && <BlockNode block={kind.elseBranch} label="else" />}
          </>
        );

      case 'While':
        return (
          <>
            <span className="ast-keyword">while</span>
            <ExpressionNode expression={kind.condition} label="condition" />
            <BlockNode block={kind.body} label="body" />
          </>
        );

      case 'For':
        return (
          <>
            <span className="ast-keyword">for</span>
            <span className="ast-name">{kind.variable}</span>
            <span className="ast-keyword">in</span>
            <ExpressionNode expression={kind.iterable} label="iterable" />
            <BlockNode block={kind.body} label="body" />
          </>
        );

      case 'Loop':
        return (
          <>
            <span className="ast-keyword">loop</span>
            <BlockNode block={kind.body} label="body" />
          </>
        );

      case 'Break':
        // Tuple variant with Option: Some(expr) has spread fields, None has no expression
        return (
          <>
            <span className="ast-keyword">break</span>
            {kind.kind && <ExpressionNode expression={{ kind: kind.kind, span: kind.span! }} />}
          </>
        );

      case 'Continue':
        return <span className="ast-keyword">continue</span>;

      case 'Import':
        return (
          <>
            <span className="ast-keyword">import</span>
            <span className="ast-string">{kind.path.join('.')}</span>
          </>
        );

      case 'ImplDecl':
        return (
          <>
            <span className="ast-keyword">impl</span>
            {kind.typeParams && kind.typeParams.length > 0 && (
              <TypeParamsNode typeParams={kind.typeParams} />
            )}
            {kind.traitName && (
              <>
                <span className="ast-type">{kind.traitName}</span>
                <span className="ast-keyword"> for </span>
              </>
            )}
            <span className="ast-type">{kind.typeName}</span>
            {kind.methods && kind.methods.length > 0 && (
              <AstNode label={`methods (${kind.methods.length})`} nodeType="methods">
                {kind.methods.map((method, i) => (
                  <StatementNode key={i} statement={method} />
                ))}
              </AstNode>
            )}
          </>
        );

      case 'ConstDecl':
        return (
          <>
            <span className="ast-keyword">const</span>
            <span className="ast-name">{kind.name}</span>
            {kind.typeAnn && (
              <>
                <span className="ast-operator">: </span>
                <TypeExprNode type={kind.typeAnn} />
              </>
            )}
            <span className="ast-operator"> = </span>
            <ExpressionNode expression={kind.initializer} label="value" />
          </>
        );

      case 'ClassDecl':
        return (
          <>
            <span className="ast-keyword">class</span>
            <span className="ast-name">{kind.name}</span>
            {kind.parent && (
              <>
                <span className="ast-keyword"> extends </span>
                <span className="ast-type">{kind.parent}</span>
              </>
            )}
            {kind.interfaces && kind.interfaces.length > 0 && (
              <>
                <span className="ast-keyword"> implements </span>
                <span className="ast-type">{kind.interfaces.join(', ')}</span>
              </>
            )}
            {kind.fields && kind.fields.length > 0 && (
              <AstNode label={`fields (${kind.fields.length})`} nodeType="fields">
                {kind.fields.map((field, i) => (
                  <FieldNode key={i} field={field} />
                ))}
              </AstNode>
            )}
            {kind.methods && kind.methods.length > 0 && (
              <AstNode label={`methods (${kind.methods.length})`} nodeType="methods">
                {kind.methods.map((method, i) => (
                  <StatementNode key={i} statement={method} />
                ))}
              </AstNode>
            )}
          </>
        );

      case 'InterfaceDecl':
        return (
          <>
            <span className="ast-keyword">interface</span>
            <span className="ast-name">{kind.name}</span>
            {kind.methods && kind.methods.length > 0 && (
              <AstNode label={`methods (${kind.methods.length})`} nodeType="methods">
                {kind.methods.map((method, i) => (
                  <AstNode key={i} label={method.name} nodeType="method" span={method.span}>
                    <span className="ast-name">{method.name}</span>
                    <ParameterListNode params={method.params} />
                    {method.returnType && (
                      <>
                        <span className="ast-operator"> -&gt; </span>
                        <TypeExprNode type={method.returnType} />
                      </>
                    )}
                  </AstNode>
                ))}
              </AstNode>
            )}
          </>
        );

      case 'TypeAlias':
        return (
          <>
            <span className="ast-keyword">type</span>
            <span className="ast-name">{kind.name}</span>
            <span className="ast-operator"> = </span>
            <TypeExprNode type={kind.typeExpr} />
          </>
        );

      case 'Use':
        return (
          <>
            <span className="ast-keyword">use</span>
            <span className="ast-type">{kind.path.join('::')}</span>
            {kind.alias && (
              <>
                <span className="ast-keyword"> as </span>
                <span className="ast-name">{kind.alias}</span>
              </>
            )}
            {kind.items && kind.items.length > 0 && (
              <>
                <span className="ast-operator">::{'{'}</span>
                <span className="ast-name">{kind.items.join(', ')}</span>
                <span className="ast-operator">{'}'}</span>
              </>
            )}
          </>
        );

      case 'TraitDecl':
        return (
          <>
            {kind.isPublic && <span className="ast-keyword">pub </span>}
            <span className="ast-keyword">trait</span>
            <span className="ast-name">{kind.name}</span>
            {kind.typeParams && kind.typeParams.length > 0 && (
              <TypeParamsNode typeParams={kind.typeParams} />
            )}
            {kind.methods && kind.methods.length > 0 && (
              <AstNode label={`methods (${kind.methods.length})`} nodeType="methods">
                {kind.methods.map((method, i) => (
                  <AstNode key={i} label={method.name} nodeType="method" span={method.span}>
                    {method.hasSelf && <span className="ast-keyword">self </span>}
                    <span className="ast-name">{method.name}</span>
                    <ParameterListNode params={method.params} />
                    {method.returnType && (
                      <>
                        <span className="ast-operator"> -&gt; </span>
                        <TypeExprNode type={method.returnType} />
                      </>
                    )}
                    {method.defaultImpl && <BlockNode block={method.defaultImpl} label="default" />}
                  </AstNode>
                ))}
              </AstNode>
            )}
          </>
        );

      case 'Block':
        return <BlockNode block={kind.block} label="block" />;

      default: {
        // Handle any unrecognized statement types gracefully
        const unknownKind = kind as { type?: string };
        return <span className="ast-type">{unknownKind.type ?? 'Unknown'}</span>;
      }
    }
    } catch (error) {
      console.error('Error rendering statement:', kind, error);
      return <span className="ast-error">Error rendering: {String(error)}</span>;
    }
  };

  return (
    <AstNode
      label={kind.type || 'Unknown'}
      nodeType="statement"
      span={span}
    >
      {renderContent()}
    </AstNode>
  );
}

interface ExpressionNodeProps {
  expression: Expression;
  label?: string;
}

function ExpressionNode({ expression, label }: ExpressionNodeProps) {
  if (!expression || !expression.kind) {
    return null;
  }
  const { kind, span } = expression;

  const renderContent = () => {
    switch (kind.type) {
      case 'IntLiteral':
        return <span className="ast-number">{kind.value}</span>;

      case 'FloatLiteral':
        return <span className="ast-number">{kind.value}</span>;

      case 'StringLiteral':
        return <span className="ast-string">"{kind.value}"</span>;

      case 'CharLiteral':
        return <span className="ast-string">'{kind.value}'</span>;

      case 'BoolLiteral':
        return <span className="ast-keyword">{String(kind.value)}</span>;

      case 'Null':
        return <span className="ast-keyword">null</span>;

      case 'Identifier':
        return <span className="ast-name">{kind.name}</span>;

      case 'Binary':
        return (
          <>
            <ExpressionNode expression={kind.left} label="left" />
            <span className="ast-operator">{formatBinaryOp(kind.op)}</span>
            <ExpressionNode expression={kind.right} label="right" />
          </>
        );

      case 'Unary':
        return (
          <>
            <span className="ast-operator">{formatUnaryOp(kind.op)}</span>
            <ExpressionNode expression={kind.operand} />
          </>
        );

      case 'Call':
        return (
          <>
            <ExpressionNode expression={kind.callee} label="callee" />
            {kind.typeArgs && kind.typeArgs.length > 0 && (
              <>
                <span className="ast-operator">&lt;</span>
                {kind.typeArgs.map((typeArg, i) => (
                  <span key={i}>
                    {i > 0 && <span className="ast-operator">, </span>}
                    <TypeExprNode type={typeArg} />
                  </span>
                ))}
                <span className="ast-operator">&gt;</span>
              </>
            )}
            <ArgumentListNode args={kind.args || []} />
          </>
        );

      case 'FieldAccess':
        return (
          <>
            <ExpressionNode expression={kind.object} label="object" />
            <span className="ast-operator">.</span>
            <span className="ast-name">{kind.field}</span>
          </>
        );

      case 'Index':
        return (
          <>
            <ExpressionNode expression={kind.object} label="object" />
            <span className="ast-operator">[</span>
            <ExpressionNode expression={kind.index} label="index" />
            <span className="ast-operator">]</span>
          </>
        );

      case 'Array':
        return (
          <>
            <span className="ast-operator">[</span>
            {kind.elements && kind.elements.length > 0 ? (
              <AstNode label={`${kind.elements.length} elements`} nodeType="elements">
                {kind.elements.map((elem, i) => (
                  <ExpressionNode key={i} expression={elem} label={`[${i}]`} />
                ))}
              </AstNode>
            ) : (
              <span className="ast-info">empty</span>
            )}
            <span className="ast-operator">]</span>
          </>
        );

      case 'Map':
        return (
          <>
            <span className="ast-operator">{'{'}</span>
            {kind.entries && kind.entries.length > 0 ? (
              <AstNode label={`${kind.entries.length} entries`} nodeType="entries">
                {kind.entries.map(([key, value], i) => (
                  <AstNode key={i} label={`entry ${i + 1}`} nodeType="entry">
                    <ExpressionNode expression={key} label="key" />
                    <span className="ast-operator">: </span>
                    <ExpressionNode expression={value} label="value" />
                  </AstNode>
                ))}
              </AstNode>
            ) : (
              <span className="ast-info">empty</span>
            )}
            <span className="ast-operator">{'}'}</span>
          </>
        );

      case 'Tuple':
        return (
          <>
            <span className="ast-operator">(</span>
            {kind.elements && kind.elements.length > 0 ? (
              <AstNode label={`${kind.elements.length} elements`} nodeType="elements">
                {kind.elements.map((elem, i) => (
                  <ExpressionNode key={i} expression={elem} label={`${i}`} />
                ))}
              </AstNode>
            ) : (
              <span className="ast-info">empty</span>
            )}
            <span className="ast-operator">)</span>
          </>
        );

      case 'StructLiteral':
        return (
          <>
            {kind.name && <span className="ast-type">{kind.name}</span>}
            <span className="ast-operator"> {'{'}</span>
            {kind.fields && kind.fields.length > 0 && (
              <AstNode label={`${kind.fields.length} fields`} nodeType="fields">
                {kind.fields.map(([fieldName, fieldValue], i) => (
                  <AstNode key={i} label={fieldName} nodeType="field">
                    <span className="ast-name">{fieldName}</span>
                    <span className="ast-operator">: </span>
                    <ExpressionNode expression={fieldValue} />
                  </AstNode>
                ))}
              </AstNode>
            )}
            <span className="ast-operator">{'}'}</span>
          </>
        );

      case 'Match':
        return (
          <>
            <span className="ast-keyword">match</span>
            <ExpressionNode expression={kind.subject} label="subject" />
            {kind.arms && kind.arms.length > 0 && (
              <AstNode label={`${kind.arms.length} arms`} nodeType="arms">
                {kind.arms.map((arm, i) => (
                  <MatchArmNode key={i} arm={arm} index={i} />
                ))}
              </AstNode>
            )}
          </>
        );

      case 'Spawn':
        return (
          <>
            <span className="ast-keyword">spawn</span>
            <ExpressionNode expression={kind.expr} />
          </>
        );

      case 'Lambda':
        return (
          <>
            <span className="ast-operator">|</span>
            {kind.params && kind.params.length > 0 && (
              <ParameterListNode params={kind.params} />
            )}
            <span className="ast-operator">|</span>
            <ExpressionNode expression={kind.body} label="body" />
          </>
        );

      case 'IfExpr':
        return (
          <>
            <span className="ast-keyword">if</span>
            <ExpressionNode expression={kind.condition} label="condition" />
            <ExpressionNode expression={kind.thenExpr} label="then" />
            <ExpressionNode expression={kind.elseExpr} label="else" />
          </>
        );

      case 'Range':
        return (
          <>
            {kind.start && <ExpressionNode expression={kind.start} label="start" />}
            <span className="ast-operator">{kind.inclusive ? '..=' : '..'}</span>
            {kind.end && <ExpressionNode expression={kind.end} label="end" />}
          </>
        );

      case 'Cast':
        return (
          <>
            <ExpressionNode expression={kind.expr} />
            <span className="ast-keyword"> as </span>
            <TypeExprNode type={kind.typeExpr} />
          </>
        );

      case 'TypeCheck':
        return (
          <>
            <ExpressionNode expression={kind.expr} />
            <span className="ast-keyword"> is </span>
            <TypeExprNode type={kind.typeExpr} />
          </>
        );

      case 'Assign':
        return (
          <>
            <ExpressionNode expression={kind.target} label="target" />
            <span className="ast-operator"> = </span>
            <ExpressionNode expression={kind.value} label="value" />
          </>
        );

      case 'CompoundAssign':
        return (
          <>
            <ExpressionNode expression={kind.target} label="target" />
            <span className="ast-operator"> {formatBinaryOp(kind.op)}= </span>
            <ExpressionNode expression={kind.value} label="value" />
          </>
        );

      case 'Block':
        return <BlockNode block={kind.block} label="block" />;

      case 'Select':
        return (
          <>
            <span className="ast-keyword">select</span>
            {kind.arms && kind.arms.length > 0 && (
              <AstNode label={`${kind.arms.length} arms`} nodeType="arms">
                {kind.arms.map((arm, i) => (
                  <SelectArmNode key={i} arm={arm} index={i} />
                ))}
              </AstNode>
            )}
          </>
        );

      case 'EnumVariant':
        return (
          <>
            <span className="ast-type">{kind.enumName}</span>
            <span className="ast-operator">::</span>
            <span className="ast-name">{kind.variantName}</span>
          </>
        );

      case 'Path':
        return <span className="ast-type">{kind.segments.join('::')}</span>;

      case 'Try':
        return (
          <>
            <ExpressionNode expression={kind.expr} />
            <span className="ast-operator">?</span>
          </>
        );

      case 'MethodCall':
        return (
          <>
            <ExpressionNode expression={kind.receiver} label="receiver" />
            <span className="ast-operator">.</span>
            <span className="ast-name">{kind.method}</span>
            {kind.typeArgs && kind.typeArgs.length > 0 && (
              <>
                <span className="ast-operator">&lt;</span>
                {kind.typeArgs.map((typeArg, i) => (
                  <span key={i}>
                    {i > 0 && <span className="ast-operator">, </span>}
                    <TypeExprNode type={typeArg} />
                  </span>
                ))}
                <span className="ast-operator">&gt;</span>
              </>
            )}
            <ArgumentListNode args={kind.args || []} />
          </>
        );

      case 'OptionalAccess':
        return (
          <>
            <ExpressionNode expression={kind.object} label="object" />
            <span className="ast-operator">?.</span>
            <span className="ast-name">{kind.field}</span>
          </>
        );

      default: {
        // Handle any unrecognized expression types gracefully
        const unknownKind = kind as { type?: string };
        return <span className="ast-type">{unknownKind.type ?? 'Unknown'}</span>;
      }
    }
  };

  const nodeLabel = label ? `${label}: ${kind.type}` : kind.type;

  return (
    <AstNode
      label={nodeLabel}
      nodeType="expression"
      span={span}
    >
      {renderContent()}
    </AstNode>
  );
}

interface BlockNodeProps {
  block: Block;
  label: string;
}

function BlockNode({ block, label }: BlockNodeProps) {
  if (!block) {
    return null;
  }
  return (
    <AstNode
      label={`${label}: Block`}
      nodeType="block"
      span={block.span}
    >
      {block.statements?.map((stmt, i) => (
        <StatementNode key={i} statement={stmt} />
      ))}
    </AstNode>
  );
}

// ============================================================================
// TypeExpr Rendering
// ============================================================================

interface TypeExprNodeProps {
  type: TypeExpr;
  label?: string;
}

function TypeExprNode({ type, label }: TypeExprNodeProps) {
  if (!type || !type.kind) {
    return null;
  }

  const renderType = (): React.ReactNode => {
    const { kind } = type;
    switch (kind.type) {
      case 'Named':
        return <span className="ast-type">{kind.name}</span>;

      case 'Generic':
        return (
          <>
            <span className="ast-type">{kind.name}</span>
            <span className="ast-operator">&lt;</span>
            {kind.args.map((arg, i) => (
              <span key={i}>
                {i > 0 && <span className="ast-operator">, </span>}
                <TypeExprNode type={arg} />
              </span>
            ))}
            <span className="ast-operator">&gt;</span>
          </>
        );

      case 'Optional':
        return (
          <>
            <TypeExprNode type={kind.inner} />
            <span className="ast-operator">?</span>
          </>
        );

      case 'Function':
        return (
          <>
            <span className="ast-operator">(</span>
            {kind.params.map((param, i) => (
              <span key={i}>
                {i > 0 && <span className="ast-operator">, </span>}
                <TypeExprNode type={param} />
              </span>
            ))}
            <span className="ast-operator">) -&gt; </span>
            <TypeExprNode type={kind.returnType} />
          </>
        );

      case 'Tuple':
        return (
          <>
            <span className="ast-operator">(</span>
            {kind.elements.map((elem, i) => (
              <span key={i}>
                {i > 0 && <span className="ast-operator">, </span>}
                <TypeExprNode type={elem} />
              </span>
            ))}
            <span className="ast-operator">)</span>
          </>
        );

      case 'Array':
        return (
          <>
            <span className="ast-operator">[</span>
            <TypeExprNode type={kind.element} />
            <span className="ast-operator">]</span>
          </>
        );

      case 'Result':
        return (
          <>
            <span className="ast-type">Result</span>
            <span className="ast-operator">&lt;</span>
            <TypeExprNode type={kind.okType} />
            <span className="ast-operator">, </span>
            <TypeExprNode type={kind.errType} />
            <span className="ast-operator">&gt;</span>
          </>
        );

      case 'Path':
        return <span className="ast-type">{kind.segments.join('::')}</span>;

      default:
        return <span className="ast-type">{(kind as { type: string }).type}</span>;
    }
  };

  if (label) {
    return (
      <span className="ast-type-expr">
        <span className="ast-label">{label}: </span>
        {renderType()}
      </span>
    );
  }

  return <>{renderType()}</>;
}

// ============================================================================
// Pattern Rendering
// ============================================================================

interface PatternNodeProps {
  pattern: Pattern;
  label?: string;
}

function PatternNode({ pattern, label }: PatternNodeProps) {
  if (!pattern || !pattern.kind) {
    return null;
  }

  const renderPattern = (): React.ReactNode => {
    const { kind } = pattern;
    switch (kind.type) {
      case 'Wildcard':
        return <span className="ast-keyword">_</span>;

      case 'Literal':
        return <ExpressionNode expression={kind.expr} />;

      case 'Variable':
        return <span className="ast-name">{kind.name}</span>;

      case 'Tuple':
        return (
          <>
            <span className="ast-operator">(</span>
            {kind.elements.map((elem, i) => (
              <span key={i}>
                {i > 0 && <span className="ast-operator">, </span>}
                <PatternNode pattern={elem} />
              </span>
            ))}
            <span className="ast-operator">)</span>
          </>
        );

      case 'Struct':
        return (
          <>
            <span className="ast-type">{kind.name}</span>
            <span className="ast-operator"> {'{'} </span>
            {kind.fields.map(([fieldName, fieldPattern], i) => (
              <span key={i}>
                {i > 0 && <span className="ast-operator">, </span>}
                <span className="ast-name">{fieldName}</span>
                <span className="ast-operator">: </span>
                <PatternNode pattern={fieldPattern} />
              </span>
            ))}
            {kind.rest && <span className="ast-operator">, ..</span>}
            <span className="ast-operator"> {'}'}</span>
          </>
        );

      case 'Constructor':
        return (
          <>
            <span className="ast-type">{kind.name}</span>
            <span className="ast-operator">(</span>
            {kind.fields.map((field, i) => (
              <span key={i}>
                {i > 0 && <span className="ast-operator">, </span>}
                <PatternNode pattern={field} />
              </span>
            ))}
            <span className="ast-operator">)</span>
          </>
        );

      case 'Range':
        return (
          <>
            <PatternNode pattern={kind.start} />
            <span className="ast-operator">{kind.inclusive ? '..=' : '..'}</span>
            <PatternNode pattern={kind.end} />
          </>
        );

      case 'Or':
        return (
          <>
            {kind.patterns.map((pat, i) => (
              <span key={i}>
                {i > 0 && <span className="ast-operator"> | </span>}
                <PatternNode pattern={pat} />
              </span>
            ))}
          </>
        );

      case 'Binding':
        return (
          <>
            <span className="ast-name">{kind.name}</span>
            <span className="ast-operator"> @ </span>
            <PatternNode pattern={kind.pattern} />
          </>
        );

      default:
        return <span className="ast-type">{(kind as { type: string }).type}</span>;
    }
  };

  if (label) {
    return (
      <AstNode label={`${label}: Pattern`} nodeType="pattern" span={pattern.span}>
        {renderPattern()}
      </AstNode>
    );
  }

  return <>{renderPattern()}</>;
}

// ============================================================================
// Parameter and Argument List Rendering
// ============================================================================

interface ParameterListNodeProps {
  params: Parameter[];
}

function ParameterListNode({ params }: ParameterListNodeProps) {
  if (!params || params.length === 0) {
    return <span className="ast-info">()</span>;
  }

  return (
    <AstNode label={`params (${params.length})`} nodeType="params">
      {params.map((param, i) => (
        <AstNode key={i} label={param.name} nodeType="param" span={param.span}>
          <span className="ast-name">{param.name}</span>
          <span className="ast-operator">: </span>
          <TypeExprNode type={param.typeAnn} />
          {param.default && (
            <>
              <span className="ast-operator"> = </span>
              <ExpressionNode expression={param.default} />
            </>
          )}
        </AstNode>
      ))}
    </AstNode>
  );
}

interface ArgumentListNodeProps {
  args: Argument[];
}

function ArgumentListNode({ args }: ArgumentListNodeProps) {
  if (!args || args.length === 0) {
    return <span className="ast-info">()</span>;
  }

  return (
    <AstNode label={`args (${args.length})`} nodeType="args">
      {args.map((arg, i) => (
        <AstNode
          key={i}
          label={arg.name ? `${arg.name}:` : `arg ${i + 1}`}
          nodeType="arg"
          span={arg.span}
        >
          {arg.name && (
            <>
              <span className="ast-name">{arg.name}</span>
              <span className="ast-operator">: </span>
            </>
          )}
          <ExpressionNode expression={arg.value} />
        </AstNode>
      ))}
    </AstNode>
  );
}

// ============================================================================
// Match Arm Rendering
// ============================================================================

interface MatchArmNodeProps {
  arm: MatchArm;
  index: number;
}

function MatchArmNode({ arm, index }: MatchArmNodeProps) {
  return (
    <AstNode label={`arm ${index + 1}`} nodeType="match-arm" span={arm.span}>
      <PatternNode pattern={arm.pattern} label="pattern" />
      {arm.guard && <ExpressionNode expression={arm.guard} label="guard" />}
      <ExpressionNode expression={arm.body} label="body" />
    </AstNode>
  );
}

// ============================================================================
// Select Arm Rendering
// ============================================================================

interface SelectArmNodeProps {
  arm: SelectArm;
  index: number;
}

function SelectArmNode({ arm, index }: SelectArmNodeProps) {
  const renderArmKind = () => {
    const { kind } = arm;
    switch (kind.type) {
      case 'Recv':
        return (
          <>
            <span className="ast-keyword">recv</span>
            {kind.variable && <span className="ast-name"> {kind.variable}</span>}
            <span className="ast-operator"> &lt;- </span>
            <ExpressionNode expression={kind.channel} />
          </>
        );
      case 'Send':
        return (
          <>
            <ExpressionNode expression={kind.channel} />
            <span className="ast-operator"> &lt;- </span>
            <ExpressionNode expression={kind.value} />
          </>
        );
      case 'Default':
        return <span className="ast-keyword">default</span>;
      default:
        return <span className="ast-type">{(kind as { type: string }).type}</span>;
    }
  };

  return (
    <AstNode label={`arm ${index + 1}`} nodeType="select-arm" span={arm.span}>
      {renderArmKind()}
      <ExpressionNode expression={arm.body} label="body" />
    </AstNode>
  );
}

// ============================================================================
// Type Parameters Rendering
// ============================================================================

interface TypeParamsNodeProps {
  typeParams: TypeParam[];
}

function TypeParamsNode({ typeParams }: TypeParamsNodeProps) {
  if (!typeParams || typeParams.length === 0) {
    return null;
  }

  return (
    <>
      <span className="ast-operator">&lt;</span>
      {typeParams.map((tp, i) => (
        <span key={i}>
          {i > 0 && <span className="ast-operator">, </span>}
          <span className="ast-type">{tp.name}</span>
          {tp.bounds && tp.bounds.length > 0 && (
            <>
              <span className="ast-operator">: </span>
              <span className="ast-type">{tp.bounds.join(' + ')}</span>
            </>
          )}
        </span>
      ))}
      <span className="ast-operator">&gt;</span>
    </>
  );
}

// ============================================================================
// Field Rendering (for structs/classes)
// ============================================================================

interface FieldNodeProps {
  field: Field;
}

function FieldNode({ field }: FieldNodeProps) {
  return (
    <AstNode label={field.name} nodeType="field" span={field.span}>
      {field.isPublic && <span className="ast-keyword">pub </span>}
      {field.isMutable && <span className="ast-keyword">mut </span>}
      <span className="ast-name">{field.name}</span>
      <span className="ast-operator">: </span>
      <TypeExprNode type={field.typeAnn} />
    </AstNode>
  );
}

// ============================================================================
// Enum Variant Rendering
// ============================================================================

interface EnumVariantNodeProps {
  variant: EnumVariantType;
}

function EnumVariantNode({ variant }: EnumVariantNodeProps) {
  return (
    <AstNode label={variant.name} nodeType="variant" span={variant.span}>
      <span className="ast-name">{variant.name}</span>
      {variant.fields && variant.fields.length > 0 && (
        <>
          <span className="ast-operator">(</span>
          {variant.fields.map((field, i) => (
            <span key={i}>
              {i > 0 && <span className="ast-operator">, </span>}
              <TypeExprNode type={field} />
            </span>
          ))}
          <span className="ast-operator">)</span>
        </>
      )}
    </AstNode>
  );
}
