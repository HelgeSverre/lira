/**
 * ExpandableValue Component
 *
 * Renders structured values with expand/collapse for arrays, objects, and closures.
 * Provides rich inspection of VM values during debugging.
 */

import { useState, memo } from 'react';
import type { ValueJson } from '../../types/protocol';
import './ExpandableValue.css';

interface ExpandableValueProps {
  value: ValueJson;
  depth?: number;
  maxDepth?: number;
  initialExpanded?: boolean;
}

const MAX_ARRAY_PREVIEW = 5;
const MAX_OBJECT_PREVIEW = 3;
const DEFAULT_MAX_DEPTH = 10;

/**
 * Format a simple value (non-expandable) for display
 */
function formatSimpleValue(value: ValueJson): { text: string; className: string } {
  switch (value.type) {
    case 'Null':
      return { text: 'null', className: 'value-null' };
    case 'Bool':
      return { text: value.value ? 'true' : 'false', className: 'value-bool' };
    case 'Int':
      return { text: String(value.value), className: 'value-number' };
    case 'Float':
      return { text: String(value.value), className: 'value-number' };
    case 'String':
      return { text: `"${value.value}"`, className: 'value-string' };
    case 'Function':
      return { text: `<function@${value.codeOffset}>`, className: 'value-function' };
    case 'Fiber':
      return { text: `<fiber#${value.id}>`, className: 'value-fiber' };
    case 'Channel':
      return { text: `<channel#${value.id}>`, className: 'value-channel' };
    default:
      return { text: String(value), className: '' };
  }
}

/**
 * Check if a value is expandable (has children)
 */
function isExpandable(value: ValueJson): boolean {
  return value.type === 'Array' || value.type === 'Object' || value.type === 'Closure';
}

/**
 * Get a preview string for expandable values
 */
function getPreview(value: ValueJson): string {
  switch (value.type) {
    case 'Array':
      if (value.elements.length === 0) return '[]';
      return `Array(${value.elements.length})`;
    case 'Object': {
      const keys = Object.keys(value.fields);
      if (keys.length === 0) return '{}';
      const preview = keys.slice(0, MAX_OBJECT_PREVIEW).join(', ');
      const suffix = keys.length > MAX_OBJECT_PREVIEW ? ', ...' : '';
      return `{${preview}${suffix}}`;
    }
    case 'Closure':
      return `<closure@${value.codeOffset}>[${value.captures.length} captures]`;
    default:
      return '';
  }
}

export const ExpandableValue = memo(function ExpandableValue({
  value,
  depth = 0,
  maxDepth = DEFAULT_MAX_DEPTH,
  initialExpanded = false,
}: ExpandableValueProps) {
  const [expanded, setExpanded] = useState(initialExpanded || depth < 2);
  const [showAll, setShowAll] = useState(false);

  // Check depth limit
  if (depth >= maxDepth) {
    return <span className="value-truncated">[max depth]</span>;
  }

  // Handle non-expandable values
  if (!isExpandable(value)) {
    const { text, className } = formatSimpleValue(value);
    return <span className={`expandable-value ${className}`}>{text}</span>;
  }

  const toggle = () => setExpanded(!expanded);

  // Render expandable values
  if (value.type === 'Array') {
    const elements = value.elements;
    const displayElements = showAll ? elements : elements.slice(0, MAX_ARRAY_PREVIEW);
    const hasMore = elements.length > MAX_ARRAY_PREVIEW && !showAll;

    return (
      <div className="expandable-value expandable-container">
        <span className="expand-toggle" onClick={toggle}>
          <span className="expand-icon">{expanded ? '▼' : '▶'}</span>
          <span className="value-preview value-array">
            {expanded ? `Array(${elements.length})` : getPreview(value)}
          </span>
        </span>
        {expanded && (
          <div className="expand-children">
            {displayElements.map((element, index) => (
              <div key={index} className="expand-child">
                <span className="child-key">{index}:</span>
                <ExpandableValue
                  value={element}
                  depth={depth + 1}
                  maxDepth={maxDepth}
                />
              </div>
            ))}
            {hasMore && (
              <div className="expand-more">
                <button onClick={() => setShowAll(true)} className="show-more-btn">
                  Show {elements.length - MAX_ARRAY_PREVIEW} more...
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    );
  }

  if (value.type === 'Object') {
    const entries = Object.entries(value.fields);
    const displayEntries = showAll ? entries : entries.slice(0, MAX_OBJECT_PREVIEW);
    const hasMore = entries.length > MAX_OBJECT_PREVIEW && !showAll;

    return (
      <div className="expandable-value expandable-container">
        <span className="expand-toggle" onClick={toggle}>
          <span className="expand-icon">{expanded ? '▼' : '▶'}</span>
          <span className="value-preview value-object">
            {expanded ? `Object(${entries.length})` : getPreview(value)}
          </span>
        </span>
        {expanded && (
          <div className="expand-children">
            {displayEntries.map(([key, fieldValue]) => (
              <div key={key} className="expand-child">
                <span className="child-key">{key}:</span>
                <ExpandableValue
                  value={fieldValue}
                  depth={depth + 1}
                  maxDepth={maxDepth}
                />
              </div>
            ))}
            {hasMore && (
              <div className="expand-more">
                <button onClick={() => setShowAll(true)} className="show-more-btn">
                  Show {entries.length - MAX_OBJECT_PREVIEW} more...
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    );
  }

  if (value.type === 'Closure') {
    return (
      <div className="expandable-value expandable-container">
        <span className="expand-toggle" onClick={toggle}>
          <span className="expand-icon">{expanded ? '▼' : '▶'}</span>
          <span className="value-preview value-closure">
            {getPreview(value)}
          </span>
        </span>
        {expanded && (
          <div className="expand-children">
            <div className="expand-child">
              <span className="child-key">code_offset:</span>
              <span className="value-number">{value.codeOffset}</span>
            </div>
            {value.captures.length > 0 && (
              <div className="expand-child">
                <span className="child-key">captures:</span>
                <div className="expand-children">
                  {value.captures.map((capture, index) => (
                    <div key={index} className="expand-child">
                      <span className="child-key">[{index}]:</span>
                      <ExpandableValue
                        value={capture}
                        depth={depth + 1}
                        maxDepth={maxDepth}
                      />
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    );
  }

  return null;
});

/**
 * Helper to render a ValueJson inline (for compact display)
 */
export function formatValueInline(value: ValueJson): string {
  switch (value.type) {
    case 'Null':
      return 'null';
    case 'Bool':
      return value.value ? 'true' : 'false';
    case 'Int':
    case 'Float':
      return String(value.value);
    case 'String':
      return `"${value.value}"`;
    case 'Array':
      if (value.elements.length === 0) return '[]';
      if (value.elements.length <= 3) {
        return `[${value.elements.map(formatValueInline).join(', ')}]`;
      }
      return `[${value.elements.slice(0, 3).map(formatValueInline).join(', ')}, ...]`;
    case 'Object': {
      const keys = Object.keys(value.fields);
      if (keys.length === 0) return '{}';
      if (keys.length <= 2) {
        const items = keys.map(k => `${k}: ${formatValueInline(value.fields[k])}`);
        return `{${items.join(', ')}}`;
      }
      return `{${keys.slice(0, 2).join(', ')}, ...}`;
    }
    case 'Function':
      return `<function@${value.codeOffset}>`;
    case 'Closure':
      return `<closure@${value.codeOffset}>`;
    case 'Fiber':
      return `<fiber#${value.id}>`;
    case 'Channel':
      return `<channel#${value.id}>`;
    default:
      return '[unknown]';
  }
}
