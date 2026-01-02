/// <reference types="vite/client" />

declare module '*.css' {
  const content: { [className: string]: string };
  export default content;
}

declare module 'monaco-editor' {
  export * from 'monaco-editor/esm/vs/editor/editor.api';
}
