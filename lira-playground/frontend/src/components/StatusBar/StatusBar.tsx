import { useEditorStore } from '../../stores/editorStore';
import { useCompilerStore } from '../../stores/compilerStore';
import { useVmStore, selectFibers } from '../../stores/vmStore';
import './StatusBar.css';

export function StatusBar() {
  const { cursorPosition } = useEditorStore();
  const { status: compileStatus, errors } = useCompilerStore();
  const vmStore = useVmStore();
  const { executionStatus, exitCode, executionTime } = vmStore;
  const fibers = selectFibers(vmStore);

  const getStatusText = () => {
    switch (executionStatus) {
      case 'compiling':
        return 'Compiling...';
      case 'running':
        return 'Running';
      case 'paused':
        return 'Paused';
      case 'finished':
        return `Finished (exit: ${exitCode}, ${executionTime}ms)`;
      case 'error':
        return 'Error';
      default:
        if (compileStatus === 'compiling') return 'Compiling...';
        if (compileStatus === 'error') return `${errors.length} error(s)`;
        if (compileStatus === 'success') return 'Compiled';
        return 'Ready';
    }
  };

  const getStatusClass = () => {
    if (executionStatus === 'error' || compileStatus === 'error') return 'status-error';
    if (executionStatus === 'running') return 'status-running';
    if (executionStatus === 'finished') return 'status-success';
    if (compileStatus === 'success') return 'status-success';
    return '';
  };

  const activeFiberCount = fibers.filter(
    f => f.state.type !== 'Finished' && f.state.type !== 'Failed'
  ).length;

  return (
    <footer className="status-bar" data-testid="status-bar">
      <div className="status-bar-left">
        <span className={`status-indicator ${getStatusClass()}`} data-testid="status-indicator" data-status={executionStatus}>
          {getStatusText()}
        </span>
        {activeFiberCount > 0 && (
          <span className="fiber-count" data-testid="fiber-count">
            Fibers: {activeFiberCount}
          </span>
        )}
      </div>

      <div className="status-bar-right">
        <span className="cursor-position" data-testid="cursor-position">
          Ln {cursorPosition.line}, Col {cursorPosition.column}
        </span>
        <span className="language" data-testid="language-indicator">Lira</span>
      </div>
    </footer>
  );
}
