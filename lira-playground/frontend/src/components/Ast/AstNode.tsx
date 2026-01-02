import { useState, ReactNode } from 'react';
import type { Span } from '../../types/ast';
import { useUiStore } from '../../stores/uiStore';

interface AstNodeProps {
  label: string;
  nodeType: 'program' | 'statement' | 'expression' | 'block' | 'other';
  span?: Span;
  children?: ReactNode;
  defaultExpanded?: boolean;
}

export function AstNode({
  label,
  nodeType,
  span,
  children,
  defaultExpanded = false,
}: AstNodeProps) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  const { showSpanInfo } = useUiStore();

  const hasChildren = children !== undefined && children !== null;
  const isExpandable = hasChildren;

  const handleClick = () => {
    if (isExpandable) {
      setExpanded(!expanded);
    }
  };

  const getNodeClass = () => {
    switch (nodeType) {
      case 'program': return 'node-program';
      case 'statement': return 'node-statement';
      case 'expression': return 'node-expression';
      case 'block': return 'node-block';
      default: return '';
    }
  };

  return (
    <div className="tree-node">
      <div
        className={`tree-node-header ${getNodeClass()}`}
        onClick={handleClick}
      >
        <span className="tree-node-toggle">
          {isExpandable && (
            <span className={`toggle-icon ${expanded ? 'expanded' : ''}`}>
              {expanded ? '▼' : '▶'}
            </span>
          )}
        </span>
        <span className={`tree-node-label ${getNodeClass()}`}>
          {label}
        </span>
        {showSpanInfo && span && (
          <span className="tree-node-span">
            [{span.line}:{span.column}]
          </span>
        )}
      </div>
      {expanded && hasChildren && (
        <div className="tree-node-children">
          {children}
        </div>
      )}
    </div>
  );
}
