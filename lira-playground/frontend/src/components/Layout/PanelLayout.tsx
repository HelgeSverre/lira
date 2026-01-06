import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import { ReactNode } from 'react';
import './PanelLayout.css';

interface PanelLayoutProps {
  editor: ReactNode;
  ast: ReactNode;
  output: ReactNode;
}

export function PanelLayout({ editor, ast, output }: PanelLayoutProps) {
  return (
    <PanelGroup direction="horizontal" className="panel-layout">
      <Panel defaultSize={40} minSize={20} className="panel-wrapper">
        {editor}
      </Panel>

      <PanelResizeHandle className="resize-handle" />

      <Panel defaultSize={30} minSize={15} className="panel-wrapper">
        {ast}
      </Panel>

      <PanelResizeHandle className="resize-handle" />

      <Panel defaultSize={30} minSize={20} className="panel-wrapper">
        {output}
      </Panel>
    </PanelGroup>
  );
}
