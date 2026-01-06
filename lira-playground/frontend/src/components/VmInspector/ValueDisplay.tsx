import type { Value } from '../../types/vm';
import { getValueTypeName } from '../../types/vm';
import './ValueDisplay.css';

interface ValueDisplayProps {
  value: Value;
  showType?: boolean;
}

export function ValueDisplay({ value, showType = true }: ValueDisplayProps) {
  const getValueClass = () => {
    switch (value.type) {
      case 'Null': return 'value-null';
      case 'Bool': return 'value-bool';
      case 'Int': return 'value-int';
      case 'Float': return 'value-float';
      case 'String': return 'value-string';
      case 'Array': return 'value-array';
      case 'Object': return 'value-object';
      case 'Function': return 'value-function';
      case 'Closure': return 'value-function';
      case 'Fiber': return 'value-fiber';
      case 'Channel': return 'value-channel';
    }
  };

  const renderValue = () => {
    switch (value.type) {
      case 'Null':
        return 'null';
      case 'Bool':
        return String(value.value);
      case 'Int':
        return String(value.value);
      case 'Float':
        return String(value.value);
      case 'String':
        return `"${value.value}"`;
      case 'Array':
        return `[${value.elements.length} elements]`;
      case 'Object':
        return `{${Object.keys(value.fields).length} fields}`;
      case 'Function':
        return `<fn@${value.codeOffset}>`;
      case 'Closure':
        return `<closure@${value.codeOffset}>`;
      case 'Fiber':
        return `<fiber#${value.id}>`;
      case 'Channel':
        return `<chan#${value.id}>`;
    }
  };

  return (
    <span className={`value ${getValueClass()}`}>
      <span className="value-content">{renderValue()}</span>
      {showType && (
        <span className="value-type">{getValueTypeName(value)}</span>
      )}
    </span>
  );
}
