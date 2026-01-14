import type { ReactNode } from 'react';
import { Sidebar } from './Sidebar';
import { Header } from './Header';
import type { Workspace } from '../types';

interface LayoutProps {
  children: ReactNode;
  selectedWorkspaceId?: string | null;
  onSelectWorkspace?: (workspace: Workspace) => void;
}

export function Layout({ children, selectedWorkspaceId, onSelectWorkspace }: LayoutProps) {
  return (
    <div className="flex h-dvh bg-background text-text">
      <Sidebar
        selectedWorkspaceId={selectedWorkspaceId}
        onSelectWorkspace={onSelectWorkspace}
      />
      <div className="flex flex-1 flex-col overflow-hidden">
        <Header />
        <main className="min-h-0 flex-1 overflow-hidden">{children}</main>
      </div>
    </div>
  );
}
