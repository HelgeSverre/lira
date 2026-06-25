// react-resizable-panels v4 renamed PanelGroup -> Group, PanelResizeHandle ->
// Separator, and the group's `direction` prop -> `orientation`. `defaultSize`/
// `minSize` are still percentages.
import { Panel, Group, Separator } from 'react-resizable-panels';
import { ReactNode } from 'react';
import './PanelLayout.css';

interface PanelLayoutProps {
  editor: ReactNode;
  ast: ReactNode;
  output: ReactNode;
}

export function PanelLayout({ editor, ast, output }: PanelLayoutProps) {
  return (
    <Group orientation="horizontal" className="panel-layout">
      <Panel defaultSize={40} minSize={20} className="panel-wrapper">
        {editor}
      </Panel>

      <Separator className="resize-handle" />

      <Panel defaultSize={30} minSize={15} className="panel-wrapper">
        {ast}
      </Panel>

      <Separator className="resize-handle" />

      <Panel defaultSize={30} minSize={20} className="panel-wrapper">
        {output}
      </Panel>
    </Group>
  );
}
