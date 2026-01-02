import { useCompilerStore } from '../../stores/compilerStore';
import { AstTreeView } from './AstTreeView';
import './AstPanel.css';

export function AstPanel() {
  const { ast, status, errors } = useCompilerStore();

  return (
    <div className="panel ast-panel">
      <div className="panel-header">
        <span>AST</span>
        {ast && (
          <span className="node-count">
            {ast.statements.length} statements
          </span>
        )}
      </div>
      <div className="panel-content ast-content">
        {status === 'idle' && (
          <div className="ast-placeholder">
            Click "Compile" to see the AST
          </div>
        )}
        {status === 'compiling' && (
          <div className="ast-placeholder">
            Compiling...
          </div>
        )}
        {status === 'error' && (
          <div className="ast-error">
            <div className="error-title">Compilation Errors</div>
            {errors.map((error, i) => (
              <div key={i} className="error-item">
                {error.line && <span className="error-location">Line {error.line}: </span>}
                <span className="error-message">{error.message}</span>
              </div>
            ))}
          </div>
        )}
        {status === 'success' && ast && (
          <AstTreeView program={ast} />
        )}
      </div>
    </div>
  );
}
