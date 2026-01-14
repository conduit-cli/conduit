import { useState, useEffect } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Layout, ChatView } from './components';
import { WebSocketProvider, ThemeProvider } from './hooks';
import { useSessions, useWorkspaces } from './hooks';
import type { Workspace } from './types';

// Create a client
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5000,
      refetchOnWindowFocus: false,
    },
  },
});

function AppContent() {
  const { data: sessions = [] } = useSessions();
  const { data: workspaces = [] } = useWorkspaces();
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(null);

  // Auto-select first workspace if none selected
  useEffect(() => {
    if (!selectedWorkspaceId && workspaces.length > 0) {
      setSelectedWorkspaceId(workspaces[0].id);
    }
  }, [workspaces, selectedWorkspaceId]);

  // Find session for selected workspace
  const selectedSession = sessions.find((s) => s.workspace_id === selectedWorkspaceId) ?? null;

  const handleSelectWorkspace = (workspace: Workspace) => {
    setSelectedWorkspaceId(workspace.id);
  };

  const handleNewSession = () => {
    // TODO: Open new session dialog
    console.log('New session requested');
  };

  return (
    <Layout
      selectedWorkspaceId={selectedWorkspaceId}
      onSelectWorkspace={handleSelectWorkspace}
    >
      <ChatView session={selectedSession} onNewSession={handleNewSession} />
    </Layout>
  );
}

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <WebSocketProvider>
          <AppContent />
        </WebSocketProvider>
      </ThemeProvider>
    </QueryClientProvider>
  );
}

export default App;
