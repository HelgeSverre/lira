import type { Value } from '../../types/vm';
import { ValueDisplay } from './ValueDisplay';
import './LocalsView.css';

interface LocalsViewProps {
  locals: Value[];
}

export function LocalsView({ locals }: LocalsViewProps) {
  if (locals.length === 0) {
    return <div className="locals-empty">No local variables</div>;
  }

  return (
    <div className="locals-view">
      {locals.map((value, i) => (
        <div key={i} className="local-entry">
          <span className="local-slot">[{i}]</span>
          <ValueDisplay value={value} />
        </div>
      ))}
    </div>
  );
}
