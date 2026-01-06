import type { Program, Statement, Expression, Block } from '../../types/ast';
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
            {kind.pattern?.kind?.type === 'Variable' && (
              <span className="ast-name">{kind.pattern.kind.name}</span>
            )}
            {/* New structure: pattern might be directly a name or in a value field */}
            {varDecl.name && <span className="ast-name">{varDecl.name}</span>}
            {kind.initializer && (
              <ExpressionNode expression={kind.initializer} label="init" />
            )}
            {/* Handle value field for simpler AST structure */}
            {varDecl.value && !kind.initializer && (
              <ExpressionNode expression={varDecl.value} label="value" />
            )}
          </>
        );
      }

      case 'FnDecl':
        return (
          <>
            <span className="ast-keyword">fn</span>
            <span className="ast-name">{kind.name}</span>
            {kind.params && kind.params.length > 0 && (
              <span className="ast-info">({kind.params.length} params)</span>
            )}
            {kind.body && <BlockNode block={kind.body} label="body" />}
          </>
        );

      case 'StructDecl':
        return (
          <>
            <span className="ast-keyword">struct</span>
            <span className="ast-name">{kind.name}</span>
            {kind.fields && <span className="ast-info">({kind.fields.length} fields)</span>}
          </>
        );

      case 'EnumDecl':
        return (
          <>
            <span className="ast-keyword">enum</span>
            <span className="ast-name">{kind.name}</span>
            {kind.variants && <span className="ast-info">({kind.variants.length} variants)</span>}
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
            {kind.traitName && (
              <>
                <span className="ast-type">{kind.traitName}</span>
                <span className="ast-keyword">for</span>
              </>
            )}
            <span className="ast-type">{kind.typeName}</span>
          </>
        );

      default:
        return <span className="ast-type">{kind.type}</span>;
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
            <span className="ast-operator">{kind.op}</span>
            <ExpressionNode expression={kind.right} label="right" />
          </>
        );

      case 'Unary':
        return (
          <>
            <span className="ast-operator">{kind.op}</span>
            <ExpressionNode expression={kind.operand} />
          </>
        );

      case 'Call':
        return (
          <>
            <ExpressionNode expression={kind.callee} label="callee" />
            {kind.args && kind.args.length > 0 && (
              <span className="ast-info">({kind.args.length} args)</span>
            )}
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
            <ExpressionNode expression={kind.index} label="index" />
          </>
        );

      case 'Array':
        return <span className="ast-info">[{kind.elements?.length ?? 0} elements]</span>;

      case 'Match':
        return (
          <>
            <span className="ast-keyword">match</span>
            <ExpressionNode expression={kind.subject} label="subject" />
            {kind.arms && <span className="ast-info">({kind.arms.length} arms)</span>}
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
            <span className="ast-info">{kind.params?.length ?? 0} params</span>
            <span className="ast-operator">|</span>
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

      default:
        return <span className="ast-type">{kind.type}</span>;
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
