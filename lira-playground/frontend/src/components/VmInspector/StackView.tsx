import type { Value } from '../../types/vm';
import { ValueDisplay } from './ValueDisplay';
import './StackView.css';

interface StackViewProps {
  stack: Value[];
}

export function StackView({ stack }: StackViewProps) {
  if (stack.length === 0) {
    return <div className="stack-empty">Empty stack</div>;
  }

  // Show stack top-first (most recent at top)
  const reversed = [...stack].reverse();

  return (
    <div className="stack-view">
      {reversed.map((value, i) => (
        <div key={i} className="stack-entry">
          <span className="stack-index">{stack.length - 1 - i}</span>
          <ValueDisplay value={value} />
        </div>
      ))}
    </div>
  );
}
