import { createContext, useContext } from 'react';

export interface FileViewerContextValue {
  openFile: (filePath: string, workspaceId: string) => void;
  currentWorkspaceId: string | null;
}

export const FileViewerContext = createContext<FileViewerContextValue | null>(null);

export function useFileViewer(): FileViewerContextValue | null {
  return useContext(FileViewerContext);
}
