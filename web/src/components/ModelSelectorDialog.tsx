import { useEffect, useRef, useState, useMemo } from 'react';
import { X, Loader2, Search, Check } from 'lucide-react';
import { useModels } from '../hooks';
import type { ModelInfo } from '../types';
import { cn } from '../lib/cn';

interface ModelSelectorDialogProps {
  isOpen: boolean;
  onClose: () => void;
  currentModel: string | null;
  agentType: 'claude' | 'codex' | 'gemini';
  onSelect: (modelId: string, newAgentType: 'claude' | 'codex' | 'gemini') => void;
  isUpdating?: boolean;
}

export function ModelSelectorDialog({
  isOpen,
  onClose,
  currentModel,
  agentType,
  onSelect,
  isUpdating = false,
}: ModelSelectorDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const { data: modelsData, isLoading } = useModels();

  // Filter models based on search query - show all agent types
  const filteredGroups = useMemo(() => {
    if (!modelsData?.groups) return [];

    const query = searchQuery.toLowerCase().trim();

    // Show all models, filtered by search query
    return modelsData.groups
      .map((group) => ({
        ...group,
        models: group.models.filter((model) => {
          if (!query) return true;
          return (
            model.display_name.toLowerCase().includes(query) ||
            model.id.toLowerCase().includes(query) ||
            model.description.toLowerCase().includes(query)
          );
        }),
      }))
      .filter((group) => group.models.length > 0);
  }, [modelsData, searchQuery]);

  // Flatten models for keyboard navigation
  const flatModels = useMemo(() => {
    const models: { model: ModelInfo; groupAgentType: 'claude' | 'codex' | 'gemini' }[] = [];
    filteredGroups.forEach((group) => {
      group.models.forEach((model) => {
        models.push({ model, groupAgentType: group.agent_type as 'claude' | 'codex' | 'gemini' });
      });
    });
    return models;
  }, [filteredGroups]);

  // Handle dialog open/close
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    if (isOpen) {
      dialog.showModal();
      setSearchQuery('');
      setSelectedIndex(0);
      // Focus search input after dialog opens
      setTimeout(() => searchInputRef.current?.focus(), 50);
    } else {
      dialog.close();
    }
  }, [isOpen]);

  // Handle escape key
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    const handleCancel = (e: Event) => {
      e.preventDefault();
      if (!isUpdating) {
        onClose();
      }
    };

    dialog.addEventListener('cancel', handleCancel);
    return () => dialog.removeEventListener('cancel', handleCancel);
  }, [onClose, isUpdating]);

  // Reset selected index when search changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [searchQuery]);

  // Keyboard navigation
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex((prev) => Math.min(prev + 1, flatModels.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex((prev) => Math.max(prev - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (flatModels[selectedIndex]) {
        handleSelect(flatModels[selectedIndex].model.id, flatModels[selectedIndex].groupAgentType);
      }
    }
  };

  const handleSelect = (modelId: string, modelAgentType: 'claude' | 'codex' | 'gemini') => {
    if (isUpdating) return;
    onSelect(modelId, modelAgentType);
  };

  const handleBackdropClick = (e: React.MouseEvent<HTMLDialogElement>) => {
    if (e.target === dialogRef.current && !isUpdating) {
      onClose();
    }
  };

  // Get current flat index for a model
  let currentFlatIndex = 0;

  return (
    <dialog
      ref={dialogRef}
      onClick={handleBackdropClick}
      onKeyDown={handleKeyDown}
      className="m-auto max-h-[600px] w-full max-w-lg rounded-xl border border-border bg-surface p-0 shadow-xl backdrop:bg-black/50"
    >
      <div className="flex max-h-[600px] flex-col">
        {/* Header */}
        <div className="flex shrink-0 items-center justify-between border-b border-border px-6 py-4">
          <h2 className="text-lg font-semibold text-text">Select Model</h2>
          <button
            onClick={onClose}
            disabled={isUpdating}
            className="rounded-md p-1 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
            aria-label="Close dialog"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Search input */}
        <div className="shrink-0 border-b border-border px-6 py-3">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
            <input
              ref={searchInputRef}
              type="text"
              placeholder="Search models..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-full rounded-lg border border-border bg-surface-elevated py-2 pl-10 pr-4 text-sm text-text placeholder-text-muted focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
            />
          </div>
        </div>

        {/* Model list */}
        <div className="min-h-0 flex-1 overflow-y-auto">
          {isLoading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="h-6 w-6 animate-spin text-text-muted" />
            </div>
          ) : filteredGroups.length === 0 ? (
            <div className="py-12 text-center text-text-muted">
              No models found
            </div>
          ) : (
            <div className="py-2">
              {filteredGroups.map((group) => (
                <div key={group.agent_type} className="mb-2">
                  {/* Group header */}
                  <div className="px-6 py-2 text-xs font-medium uppercase tracking-wider text-text-muted">
                    <span className="mr-2">{group.icon}</span>
                    {group.section_title}
                  </div>
                  {/* Models in group */}
                  {group.models.map((model) => {
                    const isSelected = model.id === currentModel && model.agent_type === agentType;
                    const isHighlighted = currentFlatIndex === selectedIndex;
                    const flatIndex = currentFlatIndex;
                    const groupAgentType = group.agent_type as 'claude' | 'codex' | 'gemini';
                    currentFlatIndex++;

                    return (
                      <button
                        key={`${group.agent_type}-${model.id}`}
                        onClick={() => handleSelect(model.id, groupAgentType)}
                        onMouseEnter={() => setSelectedIndex(flatIndex)}
                        disabled={isUpdating}
                        className={cn(
                          'flex w-full items-center justify-between px-6 py-2.5 text-left transition-colors',
                          isHighlighted && 'bg-surface-elevated',
                          !isHighlighted && 'hover:bg-surface-elevated/50',
                          isUpdating && 'cursor-not-allowed opacity-50'
                        )}
                      >
                        <div className="flex flex-col">
                          <div className="flex items-center gap-2">
                            <span className="font-medium text-text">
                              {model.display_name}
                            </span>
                            {model.is_default && (
                              <span className="rounded bg-text-muted/20 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted">
                                DEFAULT
                              </span>
                            )}
                          </div>
                          <span className="text-xs text-text-muted">
                            {model.description}
                          </span>
                        </div>
                        {isSelected && (
                          <Check className="h-5 w-5 shrink-0 text-accent" />
                        )}
                      </button>
                    );
                  })}
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex shrink-0 justify-end gap-3 border-t border-border px-6 py-4">
          <button
            onClick={onClose}
            disabled={isUpdating}
            className="rounded-lg px-4 py-2 text-sm font-medium text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={() => {
              if (flatModels[selectedIndex]) {
                handleSelect(flatModels[selectedIndex].model.id, flatModels[selectedIndex].groupAgentType);
              }
            }}
            disabled={isUpdating || flatModels.length === 0}
            className={cn(
              'flex items-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-70',
              isUpdating && 'cursor-wait'
            )}
          >
            {isUpdating && <Loader2 className="h-4 w-4 animate-spin" />}
            {isUpdating ? 'Updating...' : 'Select Model'}
          </button>
        </div>
      </div>
    </dialog>
  );
}
